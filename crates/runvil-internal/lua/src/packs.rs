//! The `packs` table: one read-only object per Pack the run knows.
//!
//! A Pack the frontmatter declares and a Pack the roster lists as able
//! to serve have an object even when they offer no tool, and every other
//! Pack has one when it offers a tool. A Pack the roster lists as
//! unable to serve has none, and offering it raises its reason. The objects
//! are built once per VM, after the tool objects, so `packs.required()`,
//! `packs.extras()`, `packs.get`, and a tool object's `pack` getter
//! hand out the same userdata for a Pack, while each list call builds a
//! fresh array. A Pack object's `tools` reads the shared tool objects,
//! so it passes straight to `tools.offer`. `packs.offer` puts every tool
//! of a Pack in the section's scope, recording the Pack's name rather
//! than its tools' wire names.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use mlua::{
    AnyUserData, Lua, MetaMethod, Table, UserData, UserDataFields, UserDataMethods, Value, Variadic,
};
use runvil_types::packs::PackId;

use crate::error::{Error, Result};
use crate::handles::ToolSet;
use crate::scope::ToolRuntime;
use crate::tools::tool_object;

/// The registry key of the VM's Pack-object table, keyed by name.
const PACK_OBJECTS_REGISTRY: &str = "runvil.packs.objects";

/// One Pack as Lua sees it: `name`, and `tools`, which reads a fresh
/// array of the Pack's tool objects in id order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LuaPackHandle {
    name: PackId,
    /// The ids of the Pack's offered tools, in id order.
    tools: Vec<String>,
}

impl UserData for LuaPackHandle {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("name", |_, this| Ok(this.name.to_string()));
        fields.add_field_method_get("tools", |lua, this| {
            let objects = lua.create_table()?;
            for id in &this.tools {
                if let Some(object) = tool_object(lua, id)? {
                    objects.raw_push(object)?;
                }
            }
            Ok(objects)
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(
            MetaMethod::NewIndex,
            |_, _, (key, _): (String, Value)| -> mlua::Result<()> {
                Err(mlua::Error::external(format!(
                    "Pack objects are frozen: cannot assign field {key:?}"
                )))
            },
        );
    }
}

/// The VM's Pack object for `name`, read by `packs.get` and by a tool
/// object's `pack` getter, or `None` for a Pack the run does not know.
///
/// # Errors
/// Returns an `mlua` error when the registry cannot be read.
pub(crate) fn pack_object(lua: &Lua, name: &str) -> mlua::Result<Option<AnyUserData>> {
    let objects: Option<Table> = lua.named_registry_value(PACK_OBJECTS_REGISTRY)?;
    match objects
        .map(|objects| objects.raw_get::<Value>(name))
        .transpose()?
    {
        Some(Value::UserData(object)) => Ok(Some(object)),
        _ => Ok(None),
    }
}

/// Builds one Pack object per declared Pack, per usable roster Pack,
/// and per other Pack with offered tools, then installs
/// `packs.required()`, the declared Packs, `packs.extras()`, the
/// others, both in name order, `packs.get(name)`, and `packs.offer`,
/// which records into the section's `runtime`.
///
/// # Errors
/// Returns [`Error::LuaRuntime`] if an object or function cannot be created or installed.
pub(crate) fn install_packs(
    lua: &Lua,
    globals: &Table,
    set: &Arc<ToolSet>,
    runtime: &Arc<Mutex<ToolRuntime>>,
) -> Result<()> {
    let objects = lua.create_table().map_err(Error::lua)?;
    let (mut required, mut extras) = (Vec::new(), Vec::new());
    for (name, tools) in pack_tools(set) {
        let key = name.to_string();
        if set.declared().contains(&name) {
            required.push(key.clone());
        } else {
            extras.push(key.clone());
        }
        let object = lua
            .create_userdata(LuaPackHandle { name, tools })
            .map_err(Error::lua)?;
        objects.raw_set(key, object).map_err(Error::lua)?;
    }
    lua.set_named_registry_value(PACK_OBJECTS_REGISTRY, objects)
        .map_err(Error::lua)?;

    let table = lua.create_table().map_err(Error::lua)?;
    for (function, names) in [("required", required), ("extras", extras)] {
        let list = lua
            .create_function(move |lua, ()| {
                let objects = lua.create_table()?;
                for name in &names {
                    if let Some(object) = pack_object(lua, name)? {
                        objects.raw_push(object)?;
                    }
                }
                Ok(objects)
            })
            .map_err(Error::lua)?;
        table.set(function, list).map_err(Error::lua)?;
    }
    let get = lua
        .create_function(|lua, name: Value| match name {
            Value::String(name) => pack_object(lua, &name.to_str()?),
            other => Err(mlua::Error::external(format!(
                "packs.get takes a Pack id, got {}",
                other.type_name()
            ))),
        })
        .map_err(Error::lua)?;
    table.set("get", get).map_err(Error::lua)?;
    install_offer(lua, &table, set, runtime)?;
    globals.raw_set("packs", table).map_err(Error::lua)
}

/// Installs `packs.offer(pack)` and its array form: each Pack enters
/// the section's scope once, by name, in first-offer order.
fn install_offer(
    lua: &Lua,
    table: &Table,
    set: &Arc<ToolSet>,
    runtime: &Arc<Mutex<ToolRuntime>>,
) -> Result<()> {
    let set = Arc::clone(set);
    let state = Arc::clone(runtime);
    let offer = lua
        .create_function(move |lua, args: Variadic<Value>| {
            let names = pack_names(lua, &set, args)?;
            let mut state = state
                .lock()
                .map_err(|_| mlua::Error::external("tool declaration runtime was poisoned"))?;
            for name in names {
                if !state.packs.contains(&name) {
                    state.packs.push(name);
                }
            }
            Ok(())
        })
        .map_err(Error::lua)?;
    table.set("offer", offer).map_err(Error::lua)
}

/// Reads `packs.offer`'s one argument, a Pack id, a Pack object,
/// or an array of either, as the names of Packs the run knows, checking
/// every entry against `set`'s roster before returning any.
fn pack_names(lua: &Lua, set: &ToolSet, args: Variadic<Value>) -> mlua::Result<Vec<PackId>> {
    let mut args = args.into_iter();
    let target = args.next().unwrap_or(Value::Nil);
    if let Some(extra) = args.next() {
        return Err(mlua::Error::external(format!(
            "packs.offer takes one Pack or a list of them, got extra {}",
            extra.type_name()
        )));
    }
    match target {
        Value::Table(list) => list
            .sequence_values::<Value>()
            .map(|entry| pack_name(lua, set, &entry?))
            .collect(),
        single => Ok(vec![pack_name(lua, set, &single)?]),
    }
}

/// Reads one name or Pack object as the name of a Pack the run knows,
/// raising the roster's reason for a Pack that can't serve the run.
fn pack_name(lua: &Lua, set: &ToolSet, value: &Value) -> mlua::Result<PackId> {
    let rejected = |found: String| {
        mlua::Error::external(format!(
            "packs.offer takes a Pack id, a Pack object, or a list of them, got {found}"
        ))
    };
    match value {
        Value::String(text) => {
            let text = text.to_string_lossy();
            let name = match PackId::parse(&text) {
                Ok(name) => name,
                Err(error) => return Err(rejected(format!("{text:?}: {error}"))),
            };
            if let Some(Some(reason)) = set.roster().get(&name) {
                return Err(mlua::Error::external(format!(
                    "packs.offer: the {name} Pack is unavailable in this run: {reason}"
                )));
            }
            if pack_object(lua, &text)?.is_none() {
                return Err(mlua::Error::external(format!(
                    "packs.offer: no Pack named {text:?} in this run; declare it under \
                     uses: to require it, or offer it only when packs.get({text:?}) \
                     returns it"
                )));
            }
            Ok(name)
        }
        Value::UserData(object) => object
            .borrow::<LuaPackHandle>()
            .map(|handle| handle.name.clone())
            .map_err(|_| rejected(value.type_name().to_owned())),
        other => Err(rejected(other.type_name().to_owned())),
    }
}

/// Each Pack the run knows, in name order, with the ids of its offered
/// tools in the set's id order: every declared Pack and every usable
/// roster Pack, with or without tools, and every other Pack that
/// offers one.
fn pack_tools(set: &ToolSet) -> BTreeMap<PackId, Vec<String>> {
    let usable = set
        .roster()
        .iter()
        .filter(|(_, reason)| reason.is_none())
        .map(|(name, _)| name);
    let mut packs: BTreeMap<PackId, Vec<String>> = set
        .declared()
        .iter()
        .chain(usable)
        .map(|name| (name.clone(), Vec::new()))
        .collect();
    for binding in set.offered() {
        packs
            .entry(binding.id().pack())
            .or_default()
            .push(binding.id().to_string());
    }
    packs
}

#[cfg(test)]
#[path = "packs-tests.rs"]
mod tests;
