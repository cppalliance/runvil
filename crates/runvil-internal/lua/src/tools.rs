//! The `tools` namespace: offering, adhoc tools, invocation, and counts.
//!
//! One Lua table holds every tool operation, mirroring the `models.*`
//! namespacing of model operations. The shared [`ToolSet`] holds the run
//! catalog, every catalog tool's `ToolInfo` by id. The model sees only
//! wire names: a tool's `ToolInfo.wire_name`, or else its id with `/` and
//! `.` replaced by `_`, such as `web_fetch`. Lua names a tool by its id
//! (`web/fetch`) or by its tool object, and a Pack by its id (`web`) or
//! its Pack object, never by a global. `adhoc` defines a prompt-author
//! Lua function as a tool of the adhoc Pack, `get` reads one tool object
//! by id, `packs` reads the Pack objects, `offer` and `offer_dynamic`
//! remember tools and Packs for the section's Static Set and Dynamic Set,
//! `call` dispatches a tool by id or object (installed by the coroutine
//! shim prelude, since dispatch suspends), and `calls` is the read-only
//! dispatch counter surface, keyed by id. Offering a tool the run does
//! not hold is a hard error, and so is every offer once the section's
//! first model call froze its sets. The installation logic sits here, out
//! of the VM driver; the VM only calls the installers in setup order.

use std::sync::{Arc, Mutex};

use mlua::{Function, Lua, MultiValue, Table, Value};
use runvil_model_client::detail::tool_schema_new;
use runvil_types::tools::{ToolId, ToolInfo};
use serde_json::{Value as Json, json};

use crate::alias::Alias;
use crate::error::{Error, Result};
use crate::handles::{ToolBinding, ToolSet};
use crate::json::to_json;
use crate::messages::MessageList;
use crate::packs::{ADHOC_PACK, install_pack_list, install_pack_objects};
use crate::proxy::key_text;
use crate::scope::{OfferMode, ToolCallCounts, ToolRuntime};
use crate::vm::AdhocTools;

mod decode;
mod objects;
mod offer;
mod userdata;

pub(crate) use decode::{model_tool_name, tool_name};

use decode::adhoc_params_schema;
pub(crate) use objects::tool_object;
use objects::{insert_tool_object, install_tool_get, install_tool_objects, remove_tool_object};
use offer::{Offering, install_offer};

/// The registry key of the VM's adhoc-tool handler table: `tools.adhoc`
/// writes each handler under its tool's wire name, and the scheduler's
/// `Adhoc` answer reads it back to hand the function to the shim.
const ADHOC_HANDLERS_REGISTRY: &str = "runvil.tools.adhoc_handlers";

/// The refusal of an offer function once the section's sets are frozen,
/// after the calling function's name.
const TOOL_LIST_FIXED: &str = "the section's tool list is fixed at its first models.loop round; \
                               make every offer before the first loop, or offer in another \
                               section";

/// The prefix of a `tools.adhoc` JSON Schema `parameters` table that has no
/// JSON form.
const PARAMETERS_NOT_JSON: &str = "tools.adhoc: parameters schema cannot be encoded as JSON";

/// Refuses `call`, an offer function, once `list` holds the section's
/// frozen sets.
pub(crate) fn refuse_after_render(call: &str, list: &MessageList) -> mlua::Result<()> {
    if list.rendered() {
        return Err(mlua::Error::external(format!("{call}: {TOOL_LIST_FIXED}")));
    }
    Ok(())
}

/// Reads the handler `tools.adhoc` registered under the wire name `name`
/// from the VM's handler table.
///
/// # Errors
/// Returns an internal-invariant error when the table is absent or holds no
/// function under `name`: the scheduler answers `Adhoc` only for a tool
/// the VM reports as defined.
pub(crate) fn adhoc_handler(lua: &Lua, name: &str) -> mlua::Result<Function> {
    let handlers: Option<Table> = lua.named_registry_value(ADHOC_HANDLERS_REGISTRY)?;
    let handler = match handlers {
        Some(handlers) => handlers.raw_get::<Value>(name)?,
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
/// `catalog`, the ids of every catalog tool, and `adhoc`, the VM's adhoc
/// tools, feed the unknown-key diagnostic.
///
/// # Errors
/// Returns [`Error::LuaRuntime`] if the Lua table or callbacks cannot be created or
/// installed.
fn install_lua_tool_calls(
    lua: &Lua,
    counts: &ToolCallCounts,
    catalog: Vec<String>,
    adhoc: AdhocTools,
) -> Result<()> {
    let globals = lua.globals();
    let tools: Table = globals.raw_get("tools").map_err(Error::lua)?;

    let calls_inner = lua.create_table().map_err(Error::lua)?;
    let meta = lua.create_table().map_err(Error::lua)?;

    let counts_for_index = counts.clone();
    let index = lua
        .create_function(move |_, (_table, key): (Table, String)| {
            let value = counts_for_index.get(&key).map_err(mlua::Error::external)?;
            if let Some(count) = value {
                return Ok(count);
            }
            let seeded = counts_for_index.names().map_err(mlua::Error::external)?;
            let adhoc_unseeded = key.contains('/')
                && adhoc
                    .binding(&key)
                    .map_err(mlua::Error::external)?
                    .is_some();
            let hint = if catalog.iter().any(|id| id == &key) {
                " (a catalog tool that was neither offered in this section nor called with \
                 tools.call)"
            } else if adhoc_unseeded {
                " (an adhoc tool that was neither offered in this section nor called with \
                 tools.call)"
            } else if seeded.is_empty() {
                ""
            } else {
                " - check for typos or offer it with tools.offer"
            };
            Err(mlua::Error::external(format!(
                "tools.calls: {key:?} has no seeded count; seeded names: {seeded:?}{hint}"
            )))
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
/// [`ToolCallCounts`] seeded with `ids`. Each seeded key
/// reads its live count; indexing an unseeded key is a hard error that
/// names the bad key and lists the seeded set. When the key is the id of a
/// catalog tool or of an adhoc tool `adhoc` holds that was never seeded,
/// neither offered to the section nor dispatched by a script `tools.call`,
/// the diagnostic says so.
///
/// Returns the `ToolCallCounts` handle so the executor's tool loop can
/// increment it.
///
/// # Errors
/// Returns [`Error::LuaRuntime`] when installing the `tools.calls` index fails.
pub(crate) fn install_tool_call_counts(
    lua: &Lua,
    bound_tools: &ToolSet,
    adhoc: &AdhocTools,
    ids: &[ToolId],
) -> Result<ToolCallCounts> {
    let counts = ToolCallCounts::new(ids.iter().map(ToString::to_string));
    let catalog: Vec<String> = bound_tools
        .catalog()
        .tools()
        .iter()
        .map(|info| info.id.to_string())
        .collect();
    install_lua_tool_calls(lua, &counts, catalog, adhoc.clone())?;
    Ok(counts)
}

/// Installs the tool and Pack APIs into one section VM (H1 included: there
/// is one install path for every section): the tool objects, the Pack
/// objects, `tools.packs`, both offer verbs, `tools.adhoc`, and
/// `tools.get`.
///
/// The coroutine shim prelude installs the suspending `tools.call` on this
/// table as a Lua function, because yield cannot cross the Rust callback
/// boundary. Both offer verbs refuse once `list`, the section's
/// conversation, holds the section's frozen sets.
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
    let names = install_pack_objects(lua, set, adhoc_tools)?;
    install_pack_list(lua, &tools, names)?;
    let offering = Offering {
        set: Arc::clone(set),
        runtime: Arc::clone(runtime),
        adhoc: adhoc_tools.clone(),
        list: list.clone(),
    };
    install_offer(lua, &tools, "offer", OfferMode::Static, &offering)?;
    install_offer(lua, &tools, "offer_dynamic", OfferMode::Dynamic, &offering)?;
    install_adhoc(lua, &tools, adhoc_tools)?;
    install_tool_get(lua, &tools)?;

    globals.raw_set("tools", tools).map_err(Error::lua)
}

/// Installs `tools.adhoc(attrs, f)` over a fresh handler table: `f` is
/// written under the tool's wire name for the scheduler's `Adhoc` answer
/// to read back, the tool object is stored in the tool-object registry,
/// the tool registers on `adhoc_tools`, and the tool object is returned.
/// A definition is all or nothing: the tool registers last, and a failed
/// registration clears the handler and the object, so a definition that
/// fails at any point leaves no tool defined and the name free to define
/// again. Defining offers nothing, and works before and after the section
/// renders.
fn install_adhoc(lua: &Lua, tools: &Table, adhoc_tools: &AdhocTools) -> Result<()> {
    lua.set_named_registry_value(
        ADHOC_HANDLERS_REGISTRY,
        lua.create_table().map_err(Error::lua)?,
    )
    .map_err(Error::lua)?;
    let adhoc = adhoc_tools.clone();
    let define = lua
        .create_function(move |lua, args: MultiValue| {
            let mut args = args.into_iter();
            let attrs = match args.next().unwrap_or(Value::Nil) {
                Value::Table(attrs) => attrs,
                other => {
                    return Err(mlua::Error::external(format!(
                        "tools.adhoc takes an attrs table first, got {}",
                        other.type_name()
                    )));
                }
            };
            let handler = match args.next().unwrap_or(Value::Nil) {
                Value::Function(handler) => handler,
                other => {
                    return Err(mlua::Error::external(format!(
                        "tools.adhoc takes the tool's function second, got {}",
                        other.type_name()
                    )));
                }
            };
            if let Some(extra) = args.next() {
                return Err(mlua::Error::external(format!(
                    "tools.adhoc takes two arguments, got extra {}",
                    extra.type_name()
                )));
            }
            let (name, info) = adhoc_info(lua, &attrs, &adhoc)?;
            let binding = ToolBinding::from_info(&name, &info);
            let handlers: Table = lua.named_registry_value(ADHOC_HANDLERS_REGISTRY)?;
            handlers.raw_set(name.as_str(), handler)?;
            let object = insert_tool_object(lua, &binding)?;
            let id = binding.id().to_string();
            if let Err(error) = adhoc.register(binding) {
                handlers.raw_set(name, Value::Nil)?;
                remove_tool_object(lua, &id)?;
                return Err(mlua::Error::external(error));
            }
            Ok(object)
        })
        .map_err(Error::lua)?;
    tools.set("adhoc", define).map_err(Error::lua)
}

/// Reads `tools.adhoc`'s attrs as the tool's wire name and info, checking
/// the keys, the name, the description, the parameters, and the schema,
/// and that neither the name nor the id is taken.
fn adhoc_info(lua: &Lua, attrs: &Table, adhoc: &AdhocTools) -> mlua::Result<(String, ToolInfo)> {
    let mut unknown: Vec<String> = Vec::new();
    for pair in attrs.pairs::<Value, Value>() {
        let (key, _) = pair?;
        let known = matches!(&key, Value::String(text)
            if text == "name" || text == "description" || text == "parameters");
        if !known {
            unknown.push(key_text(&key));
        }
    }
    unknown.sort();
    if let Some(key) = unknown.first() {
        return Err(mlua::Error::external(format!(
            "tools.adhoc: unknown key {key:?}; accepted keys are name, description, parameters"
        )));
    }
    let name = attr_text(attrs, "name")?;
    let name = String::from(Alias::parse(name.clone()).map_err(|_| {
        mlua::Error::external(format!(
            "tools.adhoc: invalid name {name:?}: expected [A-Za-z][A-Za-z0-9_-]{{0,63}}"
        ))
    })?);
    let description = attr_text(attrs, "description")?;
    let id = format!("{ADHOC_PACK}/{}", name.to_ascii_lowercase());
    if adhoc
        .binding(&name)
        .map_err(mlua::Error::external)?
        .is_some()
    {
        return Err(mlua::Error::external(format!(
            "tools.adhoc name {name:?} is already registered"
        )));
    }
    if let Some(holder) = adhoc.binding(&id).map_err(mlua::Error::external)? {
        return Err(mlua::Error::external(format!(
            "tools.adhoc name {name:?} has the id {id}, which {:?} already holds",
            holder.alias()
        )));
    }
    let parameters = match attrs.raw_get::<Value>("parameters")? {
        Value::Nil => json!({ "type": "object", "properties": {}, "required": [] }),
        Value::Table(parameters) => parameters_schema(lua, &parameters)?,
        other => {
            return Err(mlua::Error::external(format!(
                "tools.adhoc: parameters must be a table, got {}",
                other.type_name()
            )));
        }
    };
    tool_schema_new(name.clone(), description.clone(), parameters.clone())
        .map_err(|error| mlua::Error::external(format!("tools.adhoc: {error}")))?;
    let id = ToolId::parse(&id).map_err(|error| {
        mlua::Error::external(Error::Internal(
            format!("an adhoc tool's id is a tool id: {error}").into(),
        ))
    })?;
    Ok((name, ToolInfo::new(id, description, parameters)))
}

/// Reads the required string attr `key`.
fn attr_text(attrs: &Table, key: &str) -> mlua::Result<String> {
    match attrs.raw_get::<Value>(key)? {
        Value::String(text) => Ok(text.to_string_lossy()),
        Value::Nil => Err(mlua::Error::external(format!(
            "tools.adhoc: {key:?} is required"
        ))),
        other => Err(mlua::Error::external(format!(
            "tools.adhoc: {key:?} must be a string, got {}",
            other.type_name()
        ))),
    }
}

/// The schema a `parameters` table declares: a JSON Schema object when
/// its `type` is `"object"`, as `json.encode` converts it, else the short
/// form, one type name or `{type, description}` per parameter.
fn parameters_schema(lua: &Lua, parameters: &Table) -> mlua::Result<Json> {
    match parameters.raw_get::<Value>("type")? {
        Value::String(kind) if kind == "object" => to_json(
            lua,
            PARAMETERS_NOT_JSON,
            false,
            &Value::Table(parameters.clone()),
        ),
        _ => adhoc_params_schema(parameters),
    }
}

#[cfg(test)]
mod tests;
