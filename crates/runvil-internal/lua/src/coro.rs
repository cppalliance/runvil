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

#[path = "coro-failure.rs"]
mod failure;

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use mlua::{Function, Lua, StdLib, Table, Value};
use runvil_model_client::model::ModelBinding;

use crate::engine_globals::route_store_to_shims;
use crate::error::{self, Result};
use crate::error_value::{install_error_value, install_normalize_failure};
use crate::hardening::CancelHook;
use crate::models::{LuaModelHandle, selection_poisoned};
use crate::program::LuaProgram;
use crate::var::var_snapshot_table;

use failure::stash_failure_capture;
pub(crate) use failure::{StashedFailure, take_failure};

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

/// The shim program.
static SHIM_PROGRAM: LazyLock<Result<LuaProgram>> =
    LazyLock::new(|| LuaProgram::compile_internal(SHIM_SOURCE, SHIM_CHUNK_NAME));

/// The `tasks` namespace program, loaded after the shim chunk.
static TASKS_PROGRAM: LazyLock<Result<LuaProgram>> =
    LazyLock::new(|| LuaProgram::compile_internal(TASKS_SOURCE, TASKS_CHUNK_NAME));

/// The `fanout` program, loaded after the shim chunk.
static FANOUT_PROGRAM: LazyLock<Result<LuaProgram>> =
    LazyLock::new(|| LuaProgram::compile_internal(FANOUT_SOURCE, FANOUT_CHUNK_NAME));

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
    lua.load_std_libs(StdLib::COROUTINE).map_err(error::lua)?;
    let globals = lua.globals();
    let coroutine: Table = globals.raw_get("coroutine").map_err(error::lua)?;
    let yield_fn = recording_yield(lua, coroutine.raw_get("yield").map_err(error::lua)?)?;
    let var_snapshot = lua
        .create_function(|lua, ()| var_snapshot_table(lua).map_err(mlua::Error::external))
        .map_err(error::lua)?;
    let models: Table = globals.raw_get("models").map_err(error::lua)?;
    let tools: Table = globals.raw_get("tools").map_err(error::lua)?;
    let compactors: Table = globals.raw_get("compactors").map_err(error::lua)?;
    let error_value = install_error_value(lua).map_err(error::lua)?;
    let stash_failure = stash_failure_capture(lua)?;
    let normalize_failure = install_normalize_failure(lua).map_err(error::lua)?;
    let (enter_adhoc_handler, leave_adhoc_handler) =
        adhoc_handler_captures(lua, adhoc_handler_depth)?;
    let hook = cancel_hook.clone();
    let cancel_requested = lua
        .create_function(move |_, ()| Ok(hook.is_canceled()))
        .map_err(error::lua)?;
    let is_model_handle = lua
        .create_function(|_, value: Value| Ok(crate::models::is_handle(&value)))
        .map_err(error::lua)?;
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
        .map_err(error::lua)?;
    let guard: Function = shims.raw_get("guard").map_err(error::lua)?;
    lua.set_named_registry_value(GUARD_REGISTRY, guard)
        .map_err(error::lua)?;
    for name in ["pcall", "xpcall"] {
        let protected: Function = shims.raw_get(name).map_err(error::lua)?;
        globals.raw_set(name, protected).map_err(error::lua)?;
    }
    let call: Function = shims.raw_get("call").map_err(error::lua)?;
    globals.raw_set("call", call).map_err(error::lua)?;
    let helpers: Table = shims.raw_get("helpers").map_err(error::lua)?;
    let tasks: Table = TASKS_PROGRAM
        .as_ref()
        .map_err(Clone::clone)?
        .load(lua)?
        .call((yield_fn.clone(), var_snapshot.clone(), helpers.clone()))
        .map_err(error::lua)?;
    globals.raw_set("tasks", &tasks).map_err(error::lua)?;
    let fanout = fanout_shim(
        lua,
        yield_fn,
        var_snapshot,
        helpers,
        selection,
        is_model_handle,
    )?;
    tasks.raw_set("fanout", fanout).map_err(error::lua)?;
    stash_shims(lua, &shims)?;
    for name in ["loop", "compact"] {
        let entry: Function = shims.raw_get(name).map_err(error::lua)?;
        models.raw_set(name, entry).map_err(error::lua)?;
    }
    globals
        .raw_set("coroutine", Value::Nil)
        .map_err(error::lua)?;
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
        .map_err(error::lua)?;
    lua.load(YIELD_SOURCE)
        .set_name(YIELD_CHUNK_NAME)
        .call((record, raw_yield))
        .map_err(error::lua)
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
        .map_err(error::lua)?;
    let render_item = lua
        .create_function(|lua, item: Value| {
            crate::collection::render_item_value(lua, item).map_err(mlua::Error::external)
        })
        .map_err(error::lua)?;
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
        .map_err(error::lua)
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
        .map_err(error::lua)?;
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
        .map_err(error::lua)?;
    Ok((get, set))
}

/// Stashes the shim chunk's `store` under its registry key, for the store
/// install that reads it after the Pack preludes install, and in test
/// builds its model-issued `tool_call` form under
/// `MODEL_TOOL_CALL_REGISTRY`.
fn stash_shims(lua: &Lua, shims: &Table) -> Result<()> {
    #[cfg(any(test, feature = "test-support"))]
    {
        let model_tool_call: Function = shims.raw_get("model_tool_call").map_err(error::lua)?;
        lua.set_named_registry_value(MODEL_TOOL_CALL_REGISTRY, model_tool_call)
            .map_err(error::lua)?;
    }
    let store: Table = shims.raw_get("store").map_err(error::lua)?;
    lua.set_named_registry_value(STORE_REGISTRY, store)
        .map_err(error::lua)
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
        .map_err(error::lua)?;
    let left = Arc::clone(depth);
    let leave = lua
        .create_function(move |_, ()| {
            left.fetch_sub(1, Ordering::Relaxed);
            Ok(())
        })
        .map_err(error::lua)?;
    Ok((enter, leave))
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
    lua.named_registry_value(GUARD_REGISTRY).map_err(error::lua)
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
/// determinism violation that `pcall` cannot catch.
///
/// The executor's section setup and live H1 setup are the only callers.
///
/// # Errors
/// Returns a `LuaRuntime` failure if the shim install never ran on this VM, the
/// `store` table is absent, or the install fails.
pub fn install_store_shims(lua: &Lua) -> Result<()> {
    let shims: Table = lua
        .named_registry_value(STORE_REGISTRY)
        .map_err(error::lua)?;
    let store: Table = lua.globals().raw_get("store").map_err(error::lua)?;
    // `pairs` order is unspecified; every iteration only assigns one named
    // function into `store`, so the resulting table does not depend on it.
    for pair in shims.pairs::<String, Function>() {
        let (name, function) = pair.map_err(error::lua)?;
        store.raw_set(name, function).map_err(error::lua)?;
    }
    route_store_to_shims(lua, &shims)
}

#[cfg(test)]
#[path = "coro-tests.rs"]
mod tests;
