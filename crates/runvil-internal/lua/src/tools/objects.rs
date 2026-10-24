//! Tool objects: one frozen [`LuaToolHandle`] per offered catalog tool,
//! built once per VM, one per adhoc tool, added as `tools.adhoc` defines
//! it, and `tools.get` over them.
//!
//! Every read hands out the same userdata for a tool, so a tool read
//! through `tools.get` compares `==` to the same tool read through a
//! Pack's `tools`, while each `tools` read builds a fresh array.

use mlua::{AnyUserData, Lua, Table, Value};

use super::userdata::LuaToolHandle;
use crate::error::{Error, Result};
use crate::handles::{ToolBinding, ToolSet};

/// The registry key of the VM's tool-object table: one object per tool,
/// keyed by id.
const TOOL_OBJECTS_REGISTRY: &str = "runvil.tools.objects";

/// The VM's tool object for the id `id`, or `None` when the run cannot
/// offer that tool and no adhoc tool holds that id.
///
/// # Errors
/// Returns an `mlua` error when the registry cannot be read.
pub(crate) fn tool_object(lua: &Lua, id: &str) -> mlua::Result<Option<AnyUserData>> {
    let objects: Option<Table> = lua.named_registry_value(TOOL_OBJECTS_REGISTRY)?;
    match objects
        .map(|objects| objects.raw_get::<Value>(id))
        .transpose()?
    {
        Some(Value::UserData(object)) => Ok(Some(object)),
        _ => Ok(None),
    }
}

/// Builds the VM's tool-object table over the set's offering.
///
/// # Errors
/// Returns [`Error::Lua`] if an object or the table cannot be created or
/// stored.
pub(super) fn install_tool_objects(lua: &Lua, set: &ToolSet) -> Result<()> {
    let objects = lua.create_table().map_err(Error::lua)?;
    for binding in set.offered() {
        let object = lua
            .create_userdata(LuaToolHandle::from_binding(binding))
            .map_err(Error::lua)?;
        objects
            .raw_set(binding.id().to_string(), object)
            .map_err(Error::lua)?;
    }
    lua.set_named_registry_value(TOOL_OBJECTS_REGISTRY, objects)
        .map_err(Error::lua)
}

/// Adds the tool object of the adhoc tool `binding` binds and returns it.
///
/// # Errors
/// Returns an `mlua` error when the object cannot be created or stored.
pub(super) fn insert_tool_object(lua: &Lua, binding: &ToolBinding) -> mlua::Result<AnyUserData> {
    let objects: Table = lua.named_registry_value(TOOL_OBJECTS_REGISTRY)?;
    let object = lua.create_userdata(LuaToolHandle::from_binding(binding))?;
    objects.raw_set(binding.id().to_string(), object.clone())?;
    Ok(object)
}

/// Installs `tools.get(id)`.
///
/// # Errors
/// Returns [`Error::Lua`] if the function cannot be created or installed.
pub(super) fn install_tool_get(lua: &Lua, tools: &Table) -> Result<()> {
    let get = lua
        .create_function(|lua, id: Value| match id {
            Value::String(id) => tool_object(lua, &id.to_str()?),
            other => Err(mlua::Error::external(format!(
                "tools.get takes a tool id, got {}",
                other.type_name()
            ))),
        })
        .map_err(Error::lua)?;
    tools.set("get", get).map_err(Error::lua)
}
