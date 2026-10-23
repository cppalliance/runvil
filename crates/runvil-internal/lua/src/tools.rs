//! The `tools` namespace: scoping, invocation, and counts.
//!
//! One Lua table holds every tool operation, mirroring the `models.*`
//! namespacing of model operations. The shared [`ToolSet`] holds every
//! catalog tool the run can offer, each bound under its wire name: its id
//! with `/` and `.` replaced by `_`, such as `web_fetch`. The model sees
//! only wire names; Lua names a tool by its canonical id (`web/fetch`) or
//! by its tool object, and never by a global. `required` and `extras` list
//! the tool objects of the declared Packs and of every other Pack,
//! `get` reads one by id, `offer` scopes tools into the section,
//! `offer_local` registers a prompt-author Lua function as a tool, `call`
//! dispatches a catalog or adhoc tool (installed by the coroutine shim
//! prelude, since dispatch suspends), and `calls` is the read-only
//! dispatch counter surface, keyed by id for a catalog tool and by alias
//! for an adhoc one. Offering an id the run cannot offer is a
//! hard error, and so is every offer once the section's first loop round
//! rendered its tool list. The installation logic sits here, out of the
//! VM driver; the VM only calls the installers in setup order.

use std::sync::{Arc, Mutex};

use mlua::{FromLuaMulti, Function, Lua, MultiValue, Table, Value, Variadic};
use runvil_model_client::detail::tool_schema_new;
use runvil_types::tools::ToolId;

use crate::alias::Alias;
use crate::error::{Error, Result};
use crate::handles::{ToolBinding, ToolSet};
use crate::messages::MessageList;
use crate::scope::{ToolCallCounts, ToolRuntime};
use crate::vm::AdhocTools;

mod decode;
mod objects;
mod userdata;

pub(crate) use decode::tool_name;

use decode::{collect_offer_entries, local_params_schema};
pub(crate) use objects::tool_object;
use objects::{install_tool_lists, install_tool_objects};

/// The registry key of the VM's adhoc-tool handler table:
/// `tools.offer_local` writes each handler under its alias, and the
/// scheduler's `Adhoc` answer reads it back to hand the function to the
/// shim.
const ADHOC_HANDLERS_REGISTRY: &str = "runvil.tools.adhoc_handlers";

/// The refusal of an offer function once the section's tool list is
/// rendered, after the calling function's name.
const TOOL_LIST_FIXED: &str = "the section's tool list is fixed at its first models.loop round; \
                               make every offer before the first loop, or offer in another \
                               section";

/// Refuses `call`, an offer function, once `list` holds the section's
/// rendered tool list.
pub(crate) fn refuse_after_render(call: &str, list: &MessageList) -> mlua::Result<()> {
    if list.rendered() {
        return Err(mlua::Error::external(format!("{call}: {TOOL_LIST_FIXED}")));
    }
    Ok(())
}

/// Reads the handler `tools.offer_local` registered under `alias` from the
/// VM's handler table.
///
/// # Errors
/// Returns an internal-invariant error when the table is absent or holds no
/// function under `alias`: the scheduler answers `Adhoc` only for an alias
/// the VM reports as registered.
pub(crate) fn adhoc_handler(lua: &Lua, alias: &str) -> mlua::Result<Function> {
    let handlers: Option<Table> = lua.named_registry_value(ADHOC_HANDLERS_REGISTRY)?;
    let handler = match handlers {
        Some(handlers) => handlers.raw_get::<Value>(alias)?,
        None => Value::Nil,
    };
    match handler {
        Value::Function(handler) => Ok(handler),
        _ => Err(mlua::Error::external(Error::Internal(
            "an adhoc tool answer names a registered handler".into(),
        ))),
    }
}

/// Installs the read-only `tools.calls` counter table over `counts`;
/// `offered`, the ids of every tool the run can offer, feeds the
/// unknown-key diagnostic.
///
/// # Errors
/// Returns [`Error::LuaRuntime`] if the Lua table or callbacks cannot be created or
/// installed.
fn install_lua_tool_calls(lua: &Lua, counts: &ToolCallCounts, offered: Vec<String>) -> Result<()> {
    let globals = lua.globals();
    let tools: Table = globals.raw_get("tools").map_err(Error::lua)?;

    let calls_inner = lua.create_table().map_err(Error::lua)?;
    let meta = lua.create_table().map_err(Error::lua)?;

    let counts_for_index = counts.clone();
    let index = lua
        .create_function(move |_, (_table, key): (Table, String)| {
            let value = counts_for_index.get(&key).map_err(mlua::Error::external)?;
            if let Some(count) = value {
                Ok(count)
            } else {
                let seeded = counts_for_index.names().map_err(mlua::Error::external)?;
                let offered_unseeded = offered.iter().any(|id| id == &key);
                Err(mlua::Error::external(format!(
                    "tools.calls: {key:?} has no seeded count; \
                     seeded names: {seeded:?}{}",
                    if offered_unseeded {
                        " (a catalog tool that was neither offered in this section \
                         nor called with tools.call)"
                    } else if seeded.is_empty() {
                        ""
                    } else {
                        " - check for typos or offer it with tools.offer"
                    }
                )))
            }
        })
        .map_err(Error::lua)?;
    meta.set("__index", index).map_err(Error::lua)?;

    let newindex_err = lua
        .create_function(|_, _: MultiValue| -> mlua::Result<()> {
            Err(mlua::Error::external("tools.calls is read-only"))
        })
        .map_err(Error::lua)?;
    meta.set("__newindex", newindex_err).map_err(Error::lua)?;

    calls_inner.set_metatable(Some(meta)).map_err(Error::lua)?;

    tools.set("calls", calls_inner).map_err(Error::lua)?;
    Ok(())
}

/// Installs `tools.calls` as a read-only Lua table backed by a fresh
/// [`ToolCallCounts`] seeded with the ids of `bindings`. Each seeded key
/// reads its live count; indexing an unseeded key is a hard error that
/// names the bad key and lists the seeded set. When the key is the id of a
/// tool the run offers but was never seeded - neither offered to the
/// section nor dispatched by a script `tools.call` - the diagnostic says
/// so.
///
/// Returns the `ToolCallCounts` handle so the executor's tool loop can
/// increment it.
///
/// # Errors
/// Returns [`Error::LuaRuntime`] when installing the `tools.calls` index fails.
pub(crate) fn install_tool_call_counts(
    lua: &Lua,
    bound_tools: &ToolSet,
    bindings: &[ToolBinding],
) -> Result<ToolCallCounts> {
    let counts = ToolCallCounts::new(bindings.iter().map(|b| b.id().to_string()));
    let offered: Vec<String> = bound_tools
        .offered()
        .iter()
        .map(|binding| binding.id().to_string())
        .collect();
    install_lua_tool_calls(lua, &counts, offered)?;
    Ok(counts)
}

/// Resolves each of `call`'s entries to its offered binding's wire name,
/// checking every entry before the caller records any: an entry must parse
/// as a tool id, so a wire name, a malformed id, or an adhoc alias is
/// refused as an id the run does not offer.
fn resolve_entries(call: &str, set: &ToolSet, entries: Vec<String>) -> mlua::Result<Vec<String>> {
    entries
        .into_iter()
        .map(|name| {
            let binding = match ToolId::parse(&name) {
                Ok(_) => set.offered_binding(&name),
                Err(_) => None,
            };
            match binding {
                Some(binding) => Ok(binding.alias().to_owned()),
                None => Err(mlua::Error::external(format!(
                    "{call}: {name:?} is not a catalog tool in this run"
                ))),
            }
        })
        .collect()
}

/// Installs the tool scoping and adhoc-tool APIs into one section VM (H1
/// included: there is one install path for every section).
///
/// The coroutine shim prelude installs the suspending `tools.call` on this
/// table as a Lua function, because yield cannot cross the Rust callback
/// boundary. Both offer functions refuse once `list`, the section's
/// conversation, holds a rendered tool list.
///
/// # Errors
/// Returns [`Error::LuaRuntime`] if a Lua table or callback cannot be created or
/// installed.
pub(crate) fn install_tools(
    lua: &Lua,
    globals: &Table,
    set: &Arc<ToolSet>,
    runtime: &Arc<Mutex<ToolRuntime>>,
    adhoc_tools: &AdhocTools,
    list: &MessageList,
) -> Result<()> {
    let tools = lua.create_table().map_err(Error::lua)?;
    install_tool_objects(lua, set)?;
    install_offer(lua, &tools, set, runtime, list)?;
    install_tool_lists(lua, &tools, set)?;
    install_offer_local(lua, &tools, adhoc_tools, list)?;

    globals.raw_set("tools", tools).map_err(Error::lua)
}

/// Installs `tools.offer(tool)` and its array form: each tool enters the
/// section's scope once, under its wire name, in first-offer order.
fn install_offer(
    lua: &Lua,
    tools: &Table,
    set: &Arc<ToolSet>,
    runtime: &Arc<Mutex<ToolRuntime>>,
    list: &MessageList,
) -> Result<()> {
    let frozen = Arc::clone(set);
    let state = Arc::clone(runtime);
    let list = list.clone();
    let offer = lua
        .create_function(move |_, args: Variadic<Value>| {
            refuse_after_render("tools.offer", &list)?;
            let entries = collect_offer_entries("tools.offer", args)?;
            let resolved = resolve_entries("tools.offer", &frozen, entries)?;
            let mut state = state
                .lock()
                .map_err(|_| mlua::Error::external("tool declaration runtime was poisoned"))?;
            for name in resolved {
                if !state.added.iter().any(|existing| existing == &name) {
                    state.added.push(name);
                }
            }
            Ok(())
        })
        .map_err(Error::lua)?;
    tools.set("offer", offer).map_err(Error::lua)
}

/// Installs `tools.offer_local(alias, description, params, handler)` over
/// a fresh handler table: the alias and schema register on `adhoc_tools`
/// for membership and advertising, and the handler is written under the
/// alias for the scheduler's `Adhoc` answer to read back. An alias may
/// equal an offered tool's wire name; the adhoc tool wins in scope. The
/// arguments convert after the rendered-list check, so that refusal comes
/// first.
fn install_offer_local(
    lua: &Lua,
    tools: &Table,
    adhoc_tools: &AdhocTools,
    list: &MessageList,
) -> Result<()> {
    lua.set_named_registry_value(
        ADHOC_HANDLERS_REGISTRY,
        lua.create_table().map_err(Error::lua)?,
    )
    .map_err(Error::lua)?;
    let adhoc = adhoc_tools.clone();
    let list = list.clone();
    let offer_local = lua
        .create_function(move |lua, args: MultiValue| {
            refuse_after_render("tools.offer_local", &list)?;
            let (alias, description, params, handler) =
                <(String, String, Table, Function)>::from_lua_args(args, 1, None, lua)?;
            let alias = Alias::parse(alias).map_err(mlua::Error::external)?;
            if adhoc
                .contains(alias.as_str())
                .map_err(mlua::Error::external)?
            {
                return Err(mlua::Error::external(format!(
                    "tools.offer_local alias {:?} is already registered",
                    alias.as_str()
                )));
            }
            let parameters = local_params_schema(&params)?;
            let schema = tool_schema_new(alias.as_str(), description, parameters)
                .map_err(mlua::Error::external)?;
            let handlers: Table = lua.named_registry_value(ADHOC_HANDLERS_REGISTRY)?;
            handlers.raw_set(alias.as_str(), handler)?;
            adhoc
                .register(alias, schema)
                .map_err(mlua::Error::external)?;
            Ok(())
        })
        .map_err(Error::lua)?;
    tools.set("offer_local", offer_local).map_err(Error::lua)
}

#[cfg(test)]
mod tests;
