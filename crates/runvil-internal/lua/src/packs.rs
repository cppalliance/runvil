//! Pack objects and `tools.packs`: one read-only object per Pack the run
//! knows, and the adhoc Pack every section VM holds.
//!
//! A Pack the frontmatter declares and a Pack the roster lists as able
//! to serve have an object even when they have no tool, and every other
//! Pack has one when the run catalog holds a tool of it. A Pack the
//! roster lists as unable to serve has none, and offering it raises its
//! reason. The adhoc Pack, `adhoc`, has an object in every VM; its `tools`
//! lists the adhoc tools defined so far. The objects are built once per
//! VM, after the tool objects, so `tools.packs.get`, the integer keys of
//! `tools.packs`, `tools.packs.extras()`, and a tool object's `pack`
//! getter hand out the same userdata for a Pack, while each list read
//! builds a fresh array. A Pack object's `tools` reads the shared tool
//! objects, so it passes straight to `tools.offer`.
//!
//! `tools.packs` is an empty proxy with a protected metatable: integer
//! keys, `#`, and `pairs` walk the Pack objects in name order, `get` and
//! `extras` are its only other members, and every write raises.

use std::collections::BTreeMap;

use mlua::{
    AnyUserData, Lua, MetaMethod, MultiValue, Table, UserData, UserDataFields, UserDataMethods,
    Value,
};
use runvil_types::packs::PackId;
use runvil_types::tools::ToolId;

use crate::bindings::{RosterEntry, ToolSet};
use crate::error::{Error, Result};
use crate::proxy::read_only_walk;
use crate::tools::tool_object;
use crate::vm::AdhocTools;

/// The id of the adhoc Pack, which holds every tool `tools.adhoc` defines.
pub(crate) const ADHOC_PACK: &str = "adhoc";

/// The registry key of the VM's Pack-object table, keyed by name.
const PACK_OBJECTS_REGISTRY: &str = "runvil.packs.objects";

/// Whether `pack` is the adhoc Pack.
pub(crate) fn is_adhoc(pack: &PackId) -> bool {
    pack.to_string() == ADHOC_PACK
}

/// Where a Pack object's `tools` come from.
#[derive(Debug, Clone)]
enum PackTools {
    /// A catalog Pack's tools, by id, in id order.
    Catalog(Vec<String>),
    /// The VM's adhoc tools, read on each `tools` read.
    Adhoc(AdhocTools),
}

/// One Pack as Lua sees it: `name`; `tools`, which reads a fresh array of
/// the Pack's tool objects in id order; and `required`, whether the
/// frontmatter declares it under `uses:`.
#[derive(Debug, Clone)]
pub(crate) struct LuaPackHandle {
    name: PackId,
    required: bool,
    tools: PackTools,
}

impl LuaPackHandle {
    /// The Pack's id, which the object stands for.
    #[must_use]
    pub(crate) fn name(&self) -> &PackId {
        &self.name
    }
}

impl UserData for LuaPackHandle {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("name", |_, this| Ok(this.name.to_string()));
        fields.add_field_method_get("required", |_, this| Ok(this.required));
        fields.add_field_method_get("tools", |lua, this| {
            let ids = match &this.tools {
                PackTools::Catalog(ids) => ids.clone(),
                PackTools::Adhoc(adhoc) => adhoc
                    .bindings()
                    .map_err(mlua::Error::external)?
                    .iter()
                    .map(|binding| binding.id().to_string())
                    .collect(),
            };
            let objects = lua.create_table()?;
            for id in &ids {
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

/// The VM's Pack object for `name`, read by `tools.packs` and by a tool
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

/// The names of the VM's Pack objects, in name order: every one, and the
/// extras, those the frontmatter does not declare, never the adhoc Pack.
pub(crate) struct PackNames {
    all: Vec<String>,
    extras: Vec<String>,
}

/// Builds one Pack object per Pack `pack_tools` lists and returns their names.
///
/// # Errors
/// Returns [`Error::LuaRuntime`] if an object cannot be created or stored.
pub(crate) fn install_pack_objects(
    lua: &Lua,
    set: &ToolSet,
    adhoc: &AdhocTools,
) -> Result<PackNames> {
    let objects = lua.create_table().map_err(Error::lua)?;
    let mut names = PackNames {
        all: Vec::new(),
        extras: Vec::new(),
    };
    for (name, tools) in pack_tools(set, adhoc) {
        let key = name.to_string();
        let required = set.declared().contains(&name);
        if !required && !matches!(tools, PackTools::Adhoc(_)) {
            names.extras.push(key.clone());
        }
        names.all.push(key.clone());
        let object = lua
            .create_userdata(LuaPackHandle {
                name,
                required,
                tools,
            })
            .map_err(Error::lua)?;
        objects.raw_set(key, object).map_err(Error::lua)?;
    }
    lua.set_named_registry_value(PACK_OBJECTS_REGISTRY, objects)
        .map_err(Error::lua)?;
    Ok(names)
}

/// Installs `tools.packs` on `tools` over the Pack objects `names` lists.
///
/// # Errors
/// Returns [`Error::LuaRuntime`] if a function or table cannot be created
/// or installed, and [`Error::Internal`] if the `tools.packs` proxy comes
/// back without the metatable `__len` is set on.
pub(crate) fn install_pack_list(lua: &Lua, tools: &Table, names: PackNames) -> Result<()> {
    let PackNames { all, extras } = names;
    let get = lua
        .create_function(|lua, name: Value| match name {
            Value::String(name) => pack_object(lua, &name.to_str()?),
            other => Err(mlua::Error::external(format!(
                "tools.packs.get takes one Pack id, got {}",
                other.type_name()
            ))),
        })
        .map_err(Error::lua)?;
    let extras = lua
        .create_function(move |lua, ()| pack_array(lua, &extras))
        .map_err(Error::lua)?;

    let indexed = all.clone();
    let index = lua
        .create_function(move |lua, (_, key): (Table, Value)| match key {
            Value::String(name) if name == "get" => Ok(Value::Function(get.clone())),
            Value::String(name) if name == "extras" => Ok(Value::Function(extras.clone())),
            key => match position(&key).and_then(|index| indexed.get(index)) {
                Some(name) => Ok(pack_object(lua, name)?.map_or(Value::Nil, Value::UserData)),
                None => Ok(Value::Nil),
            },
        })
        .map_err(Error::lua)?;
    let count = all.len();
    let len = lua
        .create_function(move |_, _: MultiValue| Ok(count))
        .map_err(Error::lua)?;
    let step = lua
        .create_function(move |lua, (_, key): (Value, Value)| {
            let next = match key {
                Value::Nil => 0,
                key => position(&key).map_or(all.len(), |index| index + 1),
            };
            match all.get(next) {
                Some(name) => Ok((
                    Value::Integer(i64::try_from(next + 1).unwrap_or(i64::MAX)),
                    pack_object(lua, name)?.map_or(Value::Nil, Value::UserData),
                )),
                None => Ok((Value::Nil, Value::Nil)),
            }
        })
        .map_err(Error::lua)?;
    let proxy = read_only_walk(lua, index, step, "tools.packs").map_err(Error::lua)?;
    let meta = proxy
        .metatable()
        .ok_or_else(|| Error::Internal("tools.packs was built without a metatable".into()))?;
    meta.raw_set("__len", len).map_err(Error::lua)?;
    tools.raw_set("packs", proxy).map_err(Error::lua)
}

/// The 0-based position an integer key of `tools.packs` names, or `None`
/// for any other key.
fn position(key: &Value) -> Option<usize> {
    match key {
        Value::Integer(index) => usize::try_from(*index).ok()?.checked_sub(1),
        _ => None,
    }
}

/// A fresh array of the Pack objects `names` lists, in order.
fn pack_array(lua: &Lua, names: &[String]) -> mlua::Result<Table> {
    let objects = lua.create_table()?;
    for name in names {
        if let Some(object) = pack_object(lua, name)? {
            objects.raw_push(object)?;
        }
    }
    Ok(objects)
}

/// Each Pack the run knows, in id order, with its tools: every declared
/// Pack, each roster Pack that can serve, with or without tools, and every
/// other Pack the run catalog holds a tool of, each with its catalog
/// tools' ids in id order, and the adhoc Pack over `adhoc`.
fn pack_tools(set: &ToolSet, adhoc: &AdhocTools) -> BTreeMap<PackId, PackTools> {
    let usable = set
        .roster()
        .iter()
        .filter(|(_, entry)| matches!(entry, RosterEntry::Usable { .. }))
        .map(|(name, _)| name);
    let mut packs: BTreeMap<PackId, Vec<&ToolId>> = set
        .declared()
        .iter()
        .chain(usable)
        .map(|name| (name.clone(), Vec::new()))
        .collect();
    for info in set.catalog().tools() {
        packs.entry(info.id.pack()).or_default().push(&info.id);
    }
    let mut packs: BTreeMap<PackId, PackTools> = packs
        .into_iter()
        .map(|(name, mut ids)| {
            ids.sort();
            let ids = ids.into_iter().map(ToString::to_string).collect();
            (name, PackTools::Catalog(ids))
        })
        .collect();
    if let Ok(name) = PackId::parse(ADHOC_PACK) {
        packs.insert(name, PackTools::Adhoc(adhoc.clone()));
    }
    packs
}

#[cfg(test)]
#[path = "packs-tests.rs"]
mod tests;
