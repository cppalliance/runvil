//! The coroutine-protocol shim layer: per-VM Lua yield wrappers for the
//! suspending Engine calls.
//!
//! Yield cannot cross the C boundary, so `models.infer`, `call`,
//! `tools.call`, the `tasks` namespace, and `tasks.fanout` are Lua shims
//! (source in the `__impl_*.lua` files beside this one) that
//! `coroutine.yield` a request table and interpret the resume values as the
//! `(ok, result)` envelope; coroutine driving itself
//! (`Thread::create`/`resume`) is pure Rust in the scheduler. Every shim
//! yields through one wrapper, which first records the author's line as the
//! VM's call line. Each chunk compiles through
//! [`LuaProgram::compile_internal`], whose doc states the embed, `@`-name,
//! and compile-once rules.
//!
//! An adhoc tool call is a two-yield handshake inside the calling block
//! coroutine. The `tools.call` shim yields `tool_call`; for an adhoc tool
//! the driver resumes it with the handler instead of a result. The shim
//! calls the handler under a yield-safe `pcall` with `jump` refused, so
//! the handler's own suspending calls are ordinary yields of the same
//! coroutine, then yields `adhoc_tool_done` with the outcome for the
//! driver to report, and finally returns the text or raises.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use mlua::{Function, Lua, StdLib, Table, Value};
use runvil_model_client::model::ModelBinding;

use crate::engine_globals_store::route_store_to_shims;
use crate::error::{self, Error, Result, TRACEBACK_HEADING, make_lua_error as map_lua};
use crate::error_value::{install_error_value, normalized, raised_from, retained_error};
use crate::hardening::CancelHook;
use crate::models::{LuaModelHandle, selection_poisoned};
use crate::program::LuaProgram;
use crate::var::var_snapshot_table;

/// The shim chunk's name.
const SHIM_CHUNK_NAME: &str = "@crates/runvil-internal/lua/src/__impl_coro.lua";

/// The shim chunk's source.
const SHIM_SOURCE: &str = include_str!("__impl_coro.lua");

/// The `tasks` namespace chunk's name.
const TASKS_CHUNK_NAME: &str = "@crates/runvil-internal/lua/src/__impl_tasks.lua";

/// The `tasks` namespace source: spawn, the waits, the checks, note, and
/// cancel, in a chunk of its own beside the shim chunk. It runs over the
/// shim chunk's failure helpers.
const TASKS_SOURCE: &str = include_str!("__impl_tasks.lua");

/// The `fanout` chunk's name.
const FANOUT_CHUNK_NAME: &str = "@crates/runvil-internal/lua/src/__impl_fanout.lua";

/// The `fanout` shim source: Lua over the task protocol (`spawn`,
/// `join_any`, `cancel`), in a chunk of its own beside the shim chunk. It
/// runs over the shim chunk's failure helpers plus
/// the collection enumerator and the item renderer; the scheduler's
/// admission limit, not a captured cap, bounds how many arms run at once.
const FANOUT_SOURCE: &str = include_str!("__impl_fanout.lua");

/// The name of the chunk that builds the shims' recording `yield`.
const YIELD_CHUNK_NAME: &str = "@crates/runvil-internal/lua/src/coro.rs";

/// The source of the shims' recording `yield`, over the `record` capture
/// and the coroutine library's own `yield`.
const YIELD_SOURCE: &str =
    "local record, raw_yield = ...\nreturn function(...) record() return raw_yield(...) end";

/// The registry key for the shim's model-issued `tool_call` form, which
/// test builds stash for `install_model_tool_call_shim` to install as
/// `tools.call_as_model`, so a fixture section can drive the driver's
/// `call_id` path.
#[cfg(any(test, feature = "test-support"))]
pub(crate) const MODEL_TOOL_CALL_REGISTRY: &str = "runvil.impl_coro.model_tool_call";

/// The registry key for the shim's store function table, stashed by the
/// shim install so the executor can install the store yield shims onto a
/// VM's `store` table after the Pack preludes install.
const STORE_REGISTRY: &str = "runvil.impl_coro.store";

/// The registry key for the shim's block guard, stashed by the shim
/// install so [`block_guard`] can wrap every block coroutine the VM starts.
const GUARD_REGISTRY: &str = "runvil.impl_coro.guard";

/// The registry key of the last value a guarded block raised, written by
/// the guard's `stash_failure` capture from the message handler at the
/// raise point and taken by [`take_failure`] when the failure reaches the
/// Engine.
const FAILURE_REGISTRY: &str = "runvil.impl_coro.failure";

/// The registry key of the traceback recorded beside the stashed failure:
/// the coroutine's stack at the raise point, before the guard's `xpcall`
/// unwinds the block's frames. The guard's re-raise happens after that
/// unwinding, so the traceback mlua appends to the re-raised error shows
/// only the guard's own frame; this one holds the author's.
const FAILURE_TRACEBACK_REGISTRY: &str = "runvil.impl_coro.failure_traceback";

/// The shim program.
static SHIM_PROGRAM: LazyLock<Result<LuaProgram>> =
    LazyLock::new(|| LuaProgram::compile_internal(SHIM_SOURCE, SHIM_CHUNK_NAME));

/// The `tasks` namespace program, loaded after the shim chunk.
static TASKS_PROGRAM: LazyLock<Result<LuaProgram>> =
    LazyLock::new(|| LuaProgram::compile_internal(TASKS_SOURCE, TASKS_CHUNK_NAME));

/// The `fanout` program, loaded after the shim chunk.
static FANOUT_PROGRAM: LazyLock<Result<LuaProgram>> =
    LazyLock::new(|| LuaProgram::compile_internal(FANOUT_SOURCE, FANOUT_CHUNK_NAME));

/// What the guard stashed for the last failure of a guarded block.
#[derive(Debug, Default)]
pub(crate) struct StashedFailure {
    /// The raised value read back as the universal error it stands for
    /// when it is a structured error table built on this VM: the error a
    /// table was rendered from, or the one its fields hold; `None` when it
    /// was a plain string, an author's own table, or a Rust callback's
    /// wrapped failure.
    pub(crate) raised: Option<Error>,
    /// The raised value itself when it was a table, which identifies the
    /// raise of one particular table.
    table: Option<Table>,
    /// The coroutine's traceback at the raise point, `stack traceback:`
    /// heading included, when the handler recorded one.
    traceback: Option<String>,
}

impl StashedFailure {
    /// Whether the block raised `table` itself, not merely a table with
    /// the same contents.
    pub(crate) fn raised_table_is(&self, table: &Table) -> bool {
        self.table
            .as_ref()
            .is_some_and(|raised| raised.to_pointer() == table.to_pointer())
    }

    /// Restores the raise as the line mapper should read it onto a
    /// Lua-raised failure.
    ///
    /// mlua appends the coroutine's traceback to a runtime error when the
    /// coroutine dies, but the guard's re-raise is what kills it, so that
    /// traceback shows the guard's frame and nothing of the block's. When
    /// this stash recorded the real one, the appended tail is replaced with
    /// it, so the line mapper sees the author's frames. A raised error
    /// table renders as its prose, so the text before the tail becomes the
    /// message of the error the table stands for. A Rust callback's
    /// wrapped failure has its own traceback and is left untouched.
    pub(crate) fn restore_traceback<'e>(
        &self,
        error: &'e mlua::Error,
    ) -> std::borrow::Cow<'e, mlua::Error> {
        let mlua::Error::RuntimeError(message) = error else {
            return std::borrow::Cow::Borrowed(error);
        };
        if self.traceback.is_none() && self.raised.is_none() {
            return std::borrow::Cow::Borrowed(error);
        }
        let (head, tail) = message
            .rfind(TRACEBACK_HEADING)
            .map_or((message.as_str(), ""), |at| message.split_at(at));
        let head = self
            .raised
            .as_ref()
            .map_or(head, |raised| raised.message.as_str());
        let restored = match &self.traceback {
            Some(traceback) => format!("{head}\n{traceback}"),
            None => format!("{head}{tail}"),
        };
        std::borrow::Cow::Owned(mlua::Error::RuntimeError(restored))
    }
}

/// Installs the yield shims on a VM whose Engine globals already exist.
///
/// The coroutine standard library loads for the shim's `yield` capture (a
/// test VM that never installs the shims keeps exactly
/// `STRING | TABLE | MATH | UTF8`); the `coroutine` global is stripped again
/// before returning, so author code cannot yield directly and a hand-rolled
/// yield fails the driver's strict validation. All three chunks receive the
/// [`recording_yield`] over it, so every yield records its call line. The
/// `models` and `tools` tables are passed to the shim chunk as arguments, so
/// the chunk never reads a global; the chunk shims `models.infer` and installs
/// `tools.call`, and the `call` shim comes back for the Engine to install as
/// a global. The `tasks` namespace is a second chunk, run over the same
/// `yield` and `var_snapshot` captures plus the shim chunk's returned failure
/// helpers, and installed as the `tasks` global; `fanout` is a third, run
/// over the same captures plus the collection enumerator and the item
/// renderer, and installed on the `tasks` table as `tasks.fanout`. The
/// `models.loop` and `models.compact` shims are installed on the `models`
/// table.
/// `selection`, the chain's current model, is what the `fanout` chunk's
/// `get_selection` and `set_selection` captures read and write.
/// `max_tool_iterations`, the run's resolved round cap, the `compactors`
/// table, and `loop_running`, the VM's flag that refuses a loop started
/// while one runs, go to [`loop_begin`](crate::models_loop::loop_begin),
/// which runs the loop's rules behind the shim's trampoline, and the flag
/// also goes to [`compact_begin`](crate::models_loop::compact_begin).
///
/// Three further captures give the chunk the structured error shape:
/// `error_value(kind, fields)` builds the `{ kind, message, ... }` table
/// every failure takes on its way to author code, `stash_failure` lets
/// the block guard record a raised value for [`take_failure`] before mlua
/// stringifies it, and `normalize_failure` rewrites a Rust callback's
/// raised failure into the same table. The chunk's `pcall` and `xpcall`
/// replacements, which run every caught value through that capture, are
/// installed over the base library's globals here, so an Engine function that
/// fails directly from Rust reaches author code in the one shape. The next
/// two captures, `enter_adhoc_handler` and `leave_adhoc_handler`, count up
/// and down on `adhoc_handler_depth`, the counter the VM's `jump` reads, so
/// `jump` refuses while an adhoc tool's handler runs, whatever reference
/// the handler calls it through. Then `cancel_requested` reads the
/// cancel flag of `cancel_hook`, so the chunk's protected calls
/// raise a failure caught under cancellation again instead of returning
/// it, and the hook's abort always reaches the block guard. Next,
/// `is_model_handle` reports whether a value is a model handle, so the
/// `infer` and `loop` entries read a leading handle before any yield, and
/// the last
/// two, `loop_begin` and `compact_begin`, start each loop and compaction
/// call for the trampoline.
///
/// # Errors
/// Returns a `LuaRuntime` failure if the coroutine library, the shim chunk, or any
/// install step fails.
pub(crate) fn install_shims(
    lua: &Lua,
    max_tool_iterations: usize,
    selection: &Arc<Mutex<Option<ModelBinding>>>,
    adhoc_handler_depth: &Arc<AtomicU32>,
    loop_running: &Arc<AtomicBool>,
    cancel_hook: &CancelHook,
) -> Result<()> {
    lua.load_std_libs(StdLib::COROUTINE).map_err(map_lua)?;
    let globals = lua.globals();
    let coroutine: Table = globals.raw_get("coroutine").map_err(map_lua)?;
    let yield_fn = recording_yield(lua, coroutine.raw_get("yield").map_err(map_lua)?)?;
    let var_snapshot = lua
        .create_function(|lua, ()| var_snapshot_table(lua).map_err(mlua::Error::external))
        .map_err(map_lua)?;
    let models: Table = globals.raw_get("models").map_err(map_lua)?;
    let tools: Table = globals.raw_get("tools").map_err(map_lua)?;
    let compactors: Table = globals.raw_get("compactors").map_err(map_lua)?;
    let error_value = install_error_value(lua).map_err(map_lua)?;
    let stash_failure = stash_failure_capture(lua)?;
    let normalize_failure = lua.create_function(normalized).map_err(map_lua)?;
    let (enter_adhoc_handler, leave_adhoc_handler) =
        adhoc_handler_captures(lua, adhoc_handler_depth)?;
    let hook = cancel_hook.clone();
    let cancel_requested = lua
        .create_function(move |_, ()| Ok(hook.is_canceled()))
        .map_err(map_lua)?;
    let is_model_handle = lua
        .create_function(|_, value: Value| Ok(crate::models::is_handle(&value)))
        .map_err(map_lua)?;
    let loop_begin = crate::models_loop::loop_begin(
        lua,
        max_tool_iterations,
        compactors,
        loop_running,
        cancel_hook,
    )?;
    let compact_begin = crate::models_loop::compact_begin(lua, loop_running, cancel_hook)?;
    let program = SHIM_PROGRAM.as_ref().map_err(Clone::clone)?;
    let shims: Table = program
        .load(lua)?
        .call((
            yield_fn.clone(),
            var_snapshot.clone(),
            models.clone(),
            tools,
            error_value,
            stash_failure,
            normalize_failure,
            enter_adhoc_handler,
            leave_adhoc_handler,
            cancel_requested,
            is_model_handle.clone(),
            loop_begin,
            compact_begin,
        ))
        .map_err(map_lua)?;
    let guard: Function = shims.raw_get("guard").map_err(map_lua)?;
    lua.set_named_registry_value(GUARD_REGISTRY, guard)
        .map_err(map_lua)?;
    for name in ["pcall", "xpcall"] {
        let protected: Function = shims.raw_get(name).map_err(map_lua)?;
        globals.raw_set(name, protected).map_err(map_lua)?;
    }
    let call: Function = shims.raw_get("call").map_err(map_lua)?;
    globals.raw_set("call", call).map_err(map_lua)?;
    let helpers: Table = shims.raw_get("helpers").map_err(map_lua)?;
    let tasks: Table = TASKS_PROGRAM
        .as_ref()
        .map_err(Clone::clone)?
        .load(lua)?
        .call((yield_fn.clone(), var_snapshot.clone(), helpers.clone()))
        .map_err(map_lua)?;
    globals.raw_set("tasks", &tasks).map_err(map_lua)?;
    let fanout = fanout_shim(
        lua,
        yield_fn,
        var_snapshot,
        helpers,
        selection,
        is_model_handle,
    )?;
    tasks.raw_set("fanout", fanout).map_err(map_lua)?;
    stash_shims(lua, &shims)?;
    for name in ["loop", "compact"] {
        let entry: Function = shims.raw_get(name).map_err(map_lua)?;
        models.raw_set(name, entry).map_err(map_lua)?;
    }
    globals.raw_set("coroutine", Value::Nil).map_err(map_lua)?;
    Ok(())
}

/// Builds the `yield` every shim chunk receives: a Lua function that
/// records the author's line as the VM's call line, the line of the
/// innermost author frame or `None`, and then yields through `raw_yield`.
/// A Rust callback cannot yield, so only the recording runs in Rust.
fn recording_yield(lua: &Lua, raw_yield: Function) -> Result<Function> {
    let record = lua
        .create_function(|lua, ()| {
            error::set_call_line(lua, error::author_line(lua));
            Ok(())
        })
        .map_err(map_lua)?;
    lua.load(YIELD_SOURCE)
        .set_name(YIELD_CHUNK_NAME)
        .call((record, raw_yield))
        .map_err(map_lua)
}

/// Runs the `fanout` chunk over the shim chunk's `yield` and `var_snapshot`
/// captures and failure `helpers`, plus the collection enumerator, the item
/// renderer, the two captures over the chain's current model `selection`,
/// and the shim chunk's `is_model_handle`, and returns the shim installed as
/// `tasks.fanout`.
fn fanout_shim(
    lua: &Lua,
    yield_fn: Function,
    var_snapshot: Function,
    helpers: Table,
    selection: &Arc<Mutex<Option<ModelBinding>>>,
    is_model_handle: Function,
) -> Result<Function> {
    // The enumerator answers `(members)` or `(nil, message)` so the shim
    // raises the message as its own call error, exactly as the other
    // author-argument failures surface at the call site.
    let collection_members = lua
        .create_function(|lua, collection: Value| {
            match crate::collection::collection_members(lua, &collection) {
                Ok(members) => Ok((Value::Table(members), Value::Nil)),
                Err(error) => Ok((
                    Value::Nil,
                    Value::String(lua.create_string(error.to_string())?),
                )),
            }
        })
        .map_err(map_lua)?;
    let render_item = lua
        .create_function(|lua, item: Value| {
            crate::collection::render_item_value(lua, item).map_err(mlua::Error::external)
        })
        .map_err(map_lua)?;
    let (get_selection, set_selection) = selection_captures(lua, selection)?;
    FANOUT_PROGRAM
        .as_ref()
        .map_err(Clone::clone)?
        .load(lua)?
        .call((
            yield_fn,
            var_snapshot,
            helpers,
            collection_members,
            render_item,
            get_selection,
            set_selection,
            is_model_handle,
        ))
        .map_err(map_lua)
}

/// Builds the `fanout` chunk's `get_selection()` and `set_selection(value)`
/// captures over the chain's current model `selection`.
///
/// `get_selection` returns the current model's handle, or nil when the
/// caller supplied no model. `set_selection` stores the binding of a model
/// handle, clears the selection on nil, and refuses any other value. Both
/// are passed to the chunk and are not reachable from author code.
fn selection_captures(
    lua: &Lua,
    selection: &Arc<Mutex<Option<ModelBinding>>>,
) -> Result<(Function, Function)> {
    let read = Arc::clone(selection);
    let get = lua
        .create_function(move |lua, ()| -> mlua::Result<Value> {
            let held = read.lock().map_err(|_| selection_poisoned())?.clone();
            match held {
                Some(binding) => lua
                    .create_userdata(LuaModelHandle::from_binding(binding))
                    .map(Value::UserData),
                None => Ok(Value::Nil),
            }
        })
        .map_err(map_lua)?;
    let write = Arc::clone(selection);
    let set = lua
        .create_function(move |_, value: Value| -> mlua::Result<()> {
            let binding = match &value {
                Value::Nil => None,
                Value::UserData(userdata) if userdata.is::<LuaModelHandle>() => {
                    Some(userdata.borrow::<LuaModelHandle>()?.binding().clone())
                }
                other => {
                    return Err(mlua::Error::external(format!(
                        "set_selection takes a model handle or nil, got {}",
                        crate::models::lua_type_name(other)
                    )));
                }
            };
            *write.lock().map_err(|_| selection_poisoned())? = binding;
            Ok(())
        })
        .map_err(map_lua)?;
    Ok((get, set))
}

/// Stashes the shim chunk's `store` under its registry key, for the store
/// install that reads it after the Pack preludes install, and in test
/// builds its model-issued `tool_call` form under
/// `MODEL_TOOL_CALL_REGISTRY`.
fn stash_shims(lua: &Lua, shims: &Table) -> Result<()> {
    #[cfg(any(test, feature = "test-support"))]
    {
        let model_tool_call: Function = shims.raw_get("model_tool_call").map_err(map_lua)?;
        lua.set_named_registry_value(MODEL_TOOL_CALL_REGISTRY, model_tool_call)
            .map_err(map_lua)?;
    }
    let store: Table = shims.raw_get("store").map_err(map_lua)?;
    lua.set_named_registry_value(STORE_REGISTRY, store)
        .map_err(map_lua)
}

/// Builds the shim chunk's `enter_adhoc_handler()` and `leave_adhoc_handler()`
/// captures over `depth`: one step up and one step down, so nested
/// handlers balance.
fn adhoc_handler_captures(lua: &Lua, depth: &Arc<AtomicU32>) -> Result<(Function, Function)> {
    let entered = Arc::clone(depth);
    let enter = lua
        .create_function(move |_, ()| {
            entered.fetch_add(1, Ordering::Relaxed);
            Ok(())
        })
        .map_err(map_lua)?;
    let left = Arc::clone(depth);
    let leave = lua
        .create_function(move |_, ()| {
            left.fetch_sub(1, Ordering::Relaxed);
            Ok(())
        })
        .map_err(map_lua)?;
    Ok((enter, leave))
}

/// Builds the shim chunk's `stash_failure(failure)` capture: records the
/// block's raised value and its raise-point traceback for [`take_failure`].
fn stash_failure_capture(lua: &Lua) -> Result<Function> {
    lua.create_function(|lua, failure: Value| {
        // Called from the guard's message handler, so the failing
        // frames are still on this coroutine's stack: level 1 starts
        // the traceback at the handler, above this capture's own frame.
        let traceback = lua.traceback(None, 1)?;
        lua.set_named_registry_value(FAILURE_TRACEBACK_REGISTRY, traceback)?;
        lua.set_named_registry_value(FAILURE_REGISTRY, failure)
    })
    .map_err(map_lua)
}

/// Returns the shim's block guard for a VM whose shim install already ran.
///
/// The Engine creates every block coroutine from the guard and resumes it
/// first with the block function: the guard runs the block under `xpcall`
/// (yields pass through), stashes a raised value and the raise-point
/// traceback for [`take_failure`] from the message handler, and re-raises
/// the same value, so a shim's structured error table reaches the Engine
/// intact instead of only as mlua's stringification.
///
/// # Errors
/// Returns a `LuaRuntime` failure if the shim install never ran on this VM.
pub(crate) fn block_guard(lua: &Lua) -> Result<Function> {
    lua.named_registry_value(GUARD_REGISTRY).map_err(map_lua)
}

/// Takes what the guard stashed for the last failure of a guarded block.
/// When a failure was stashed, both slots are cleared, so a later failure
/// never sees a stale value; when none was, the result is the default
/// (nothing raised, no traceback).
///
/// # Errors
/// Returns a `LuaRuntime` failure if a registry slot cannot be read or
/// cleared.
pub(crate) fn take_failure(lua: &Lua) -> Result<StashedFailure> {
    let failure: Value = lua
        .named_registry_value(FAILURE_REGISTRY)
        .map_err(map_lua)?;
    let traceback: Value = lua
        .named_registry_value(FAILURE_TRACEBACK_REGISTRY)
        .map_err(map_lua)?;
    if matches!(failure, Value::Nil) {
        return Ok(StashedFailure::default());
    }
    lua.set_named_registry_value(FAILURE_REGISTRY, Value::Nil)
        .map_err(map_lua)?;
    lua.set_named_registry_value(FAILURE_TRACEBACK_REGISTRY, Value::Nil)
        .map_err(map_lua)?;
    let traceback = match traceback {
        Value::String(text) => Some(text.to_str().map_err(map_lua)?.to_owned()),
        _ => None,
    };
    let raised = match retained_error(lua, &failure).map_err(map_lua)? {
        Some(retained) => Some(retained),
        None => raised_from(lua, &failure).map_err(map_lua)?,
    };
    let table = match failure {
        Value::Table(table) => Some(table),
        _ => None,
    };
    Ok(StashedFailure {
        raised,
        table,
        traceback,
    })
}

/// Installs the store yield shims onto a VM's `store` table, replacing the
/// dispatchers installed with the other Engine globals, and switches those
/// dispatchers to the shims too, so a store function a Pack prelude
/// captured before this call (`local write = store.write`) yields as well.
/// Every store operation, through whichever reference the prompt holds,
/// then suspends the block as a leaf yield.
/// [`Request::Store`](crate::Request::Store) states why every backend takes
/// this path. The Harness performs each one as an `Effect::Vfs`, and a
/// claims-model conflict through any of them ends the run with a
/// make_determinism_error violation that `pcall` cannot catch.
///
/// The executor's section setup and live H1 setup are the only callers.
///
/// # Errors
/// Returns a `LuaRuntime` failure if the shim install never ran on this VM, the
/// `store` table is absent, or the install fails.
pub fn install_store_shims(lua: &Lua) -> Result<()> {
    let shims: Table = lua.named_registry_value(STORE_REGISTRY).map_err(map_lua)?;
    let store: Table = lua.globals().raw_get("store").map_err(map_lua)?;
    // `pairs` order is unspecified; every iteration only assigns one named
    // function into `store`, so the resulting table does not depend on it.
    for pair in shims.pairs::<String, Function>() {
        let (name, function) = pair.map_err(map_lua)?;
        store.raw_set(name, function).map_err(map_lua)?;
    }
    route_store_to_shims(lua, &shims)
}

#[cfg(test)]
mod tests {
    //! Tests that the shim chunk names stay pointed at the shim files, and
    //! that the one shim install puts `models.loop` and `models.compact` on
    //! `models`.

    use std::sync::{Arc, Mutex};

    use mlua::{Lua, Value};
    use runvil_model_client::model::{ModelBinding, ModelId, ModelInvocation};

    use super::{
        FANOUT_CHUNK_NAME, FANOUT_SOURCE, SHIM_CHUNK_NAME, SHIM_SOURCE, TASKS_CHUNK_NAME,
        TASKS_SOURCE, selection_captures,
    };
    use crate::models::LuaModelHandle;
    use crate::test_vm::TestVm;
    use crate::tests::assert_chunk_name_resolves;

    #[test]
    fn the_shim_chunk_name_resolves_to_the_shim_file() {
        assert_chunk_name_resolves("SHIM_CHUNK_NAME", SHIM_CHUNK_NAME, SHIM_SOURCE);
    }

    #[test]
    fn the_tasks_chunk_name_resolves_to_the_tasks_file() {
        assert_chunk_name_resolves("TASKS_CHUNK_NAME", TASKS_CHUNK_NAME, TASKS_SOURCE);
    }

    #[test]
    fn the_fanout_chunk_name_resolves_to_the_fanout_file() {
        assert_chunk_name_resolves("FANOUT_CHUNK_NAME", FANOUT_CHUNK_NAME, FANOUT_SOURCE);
    }

    #[test]
    fn the_yield_shim_install_puts_models_loop_and_models_compact_on_models() {
        let vm = TestVm::new().shims().build();
        let (looped, compacted): (String, String) =
            vm.eval("return type(models.loop), type(models.compact)");
        assert_eq!(
            (looped.as_str(), compacted.as_str()),
            ("function", "function"),
            "the coroutine shim install alone leaves both shims on `models`"
        );
    }

    /// A binding labelled `label`, over the model `model`.
    fn binding(label: &str, model: &str) -> ModelBinding {
        ModelBinding::new(
            label,
            "A general model for tests",
            ModelId::gateway(model).expect("a static model id is valid"),
            ModelInvocation::new(),
            std::num::NonZeroU32::new(8192).expect("8192 is non-zero"),
        )
    }

    /// The label of the handle `value` holds, when it holds one.
    fn handle_label(value: &Value) -> Option<String> {
        match value {
            Value::UserData(userdata) => userdata
                .borrow::<LuaModelHandle>()
                .ok()
                .map(|handle| handle.binding().alias().to_owned()),
            _ => None,
        }
    }

    #[test]
    fn get_selection_returns_the_current_handle_or_nil() {
        let lua = Lua::new();
        let selection = Arc::new(Mutex::new(None));
        let (get, _) = selection_captures(&lua, &selection).expect("the captures build");

        let empty: Value = get.call(()).expect("a read of an empty selection succeeds");
        assert!(matches!(empty, Value::Nil), "no model reads as nil");

        *selection.lock().expect("the selection is not poisoned") = Some(binding("strong", "m2"));
        let current: Value = get.call(()).expect("a read of a held selection succeeds");
        assert_eq!(handle_label(&current).as_deref(), Some("strong"));
    }

    #[test]
    fn set_selection_stores_a_handle_clears_on_nil_and_refuses_anything_else() {
        let lua = Lua::new();
        let selection = Arc::new(Mutex::new(Some(binding("default", "m1"))));
        let (_, set) = selection_captures(&lua, &selection).expect("the captures build");
        let label = || {
            selection
                .lock()
                .expect("the selection is not poisoned")
                .as_ref()
                .map(|held| held.alias().to_owned())
        };

        let handle = lua
            .create_userdata(LuaModelHandle::from_binding(binding("fast", "m3")))
            .expect("a handle builds");
        set.call::<()>(handle).expect("a handle is stored");
        assert_eq!(label().as_deref(), Some("fast"));

        set.call::<()>(Value::Nil)
            .expect("nil clears the selection");
        assert_eq!(label(), None);

        let error = set
            .call::<()>(5)
            .expect_err("a number is neither a handle nor nil");
        assert!(
            error
                .to_string()
                .contains("set_selection takes a model handle or nil, got number"),
            "the refusal names the Lua type: {error}"
        );
        assert_eq!(label(), None, "a refused write leaves the selection alone");
    }
}
