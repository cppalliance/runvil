//! The offer verbs, `tools.offer` and `tools.offer_dynamic`: one
//! installer, parameterized by name and mode, and its decode.
//!
//! A verb takes one argument: a string with a `/`, a tool id; a string
//! without one, a Pack id, since Pack ids are one segment and tool ids
//! two or more; a tool object; a Pack object; or an array mixing them.
//! An array is its `#t` entries, read through `__index`, with no gap, no
//! nested table, and no raw key outside `1..#t`. The rendered-list
//! refusal comes first, and every entry is checked before any is
//! recorded.

use std::sync::{Arc, Mutex};

use mlua::{Lua, Table, Value, Variadic};
use runvil_types::packs::PackId;
use runvil_types::tools::ToolId;

use super::refuse_after_render;
use super::userdata::LuaToolHandle;
use crate::error::{Error, Result};
use crate::handles::{ToolSet, wire_name_of};
use crate::messages::MessageList;
use crate::packs::{LuaPackHandle, is_adhoc, pack_object};
use crate::proxy::key_text;
use crate::scope::{OfferMode, OfferTarget, ToolRuntime};
use crate::vm::AdhocTools;
use crate::walk_budget::HELD_KEY_LIMIT;

/// What an offer verb checks its entries against and records into: the
/// run's tool set, the section's offers, the VM's adhoc tools, and the
/// section's conversation, whose frozen sets fix the offers.
#[derive(Clone)]
pub(super) struct Offering {
    pub(super) set: Arc<ToolSet>,
    pub(super) runtime: Arc<Mutex<ToolRuntime>>,
    pub(super) adhoc: AdhocTools,
    pub(super) list: MessageList,
}

/// Installs the offer verb `tools.<key>` on `tools`, recording under
/// `mode`.
///
/// # Errors
/// Returns [`Error::LuaRuntime`] if the function cannot be created or
/// installed.
pub(super) fn install_offer(
    lua: &Lua,
    tools: &Table,
    key: &'static str,
    mode: OfferMode,
    offering: &Offering,
) -> Result<()> {
    let verb = format!("tools.{key}");
    let offering = offering.clone();
    let offer = lua
        .create_function(move |lua, args: Variadic<Value>| {
            refuse_after_render(&verb, &offering.list)?;
            let targets = offering.targets(lua, &verb, args)?;
            let mut runtime = offering
                .runtime
                .lock()
                .map_err(|_| mlua::Error::external("tool declaration runtime was poisoned"))?;
            for target in targets {
                match target {
                    OfferTarget::Tool(id) => runtime.add_tool(id, mode),
                    OfferTarget::Pack(pack) => runtime.add_pack(pack, mode),
                }
            }
            Ok(())
        })
        .map_err(Error::lua)?;
    tools.set(key, offer).map_err(Error::lua)
}

impl Offering {
    /// Reads `verb`'s arguments as the targets they name, checking every
    /// entry before returning any. No argument names nothing.
    fn targets(
        &self,
        lua: &Lua,
        verb: &str,
        args: Variadic<Value>,
    ) -> mlua::Result<Vec<OfferTarget>> {
        let mut args = args.into_iter();
        let Some(target) = args.next() else {
            return Ok(Vec::new());
        };
        if let Some(extra) = args.next() {
            return Err(mlua::Error::external(format!(
                "{verb} takes one tool or Pack, or an array of them, got extra {}",
                extra.type_name()
            )));
        }
        match target {
            Value::Table(table) => array(verb, &table)?
                .iter()
                .enumerate()
                .map(|(index, entry)| self.target(lua, verb, entry, Some(index + 1)))
                .collect(),
            single => Ok(vec![self.target(lua, verb, &single, None)?]),
        }
    }

    /// Reads one entry, at `index` of an array or alone, as the tool or
    /// Pack it names. A tool object under the `adhoc` Pack must be a
    /// defined adhoc tool, since only those have a binding to render.
    fn target(
        &self,
        lua: &Lua,
        verb: &str,
        entry: &Value,
        index: Option<usize>,
    ) -> mlua::Result<OfferTarget> {
        match entry {
            Value::String(text) => self.named(lua, verb, &text.to_string_lossy()),
            Value::UserData(object) => {
                if let Ok(tool) = object.borrow::<LuaToolHandle>() {
                    let id = tool.id();
                    if is_adhoc(&id.pack()) {
                        return self.named(lua, verb, &id.to_string());
                    }
                    return Ok(OfferTarget::Tool(id.clone()));
                }
                if let Ok(pack) = object.borrow::<LuaPackHandle>() {
                    return Ok(OfferTarget::Pack(pack.name().clone()));
                }
                Err(wrong_type(verb, entry, index))
            }
            other => Err(wrong_type(verb, other, index)),
        }
    }

    /// Reads a string entry: with a `/`, the id of a tool in the run
    /// catalog or a defined adhoc tool; without one, the id of a Pack with
    /// an object.
    fn named(&self, lua: &Lua, verb: &str, name: &str) -> mlua::Result<OfferTarget> {
        if name.contains('/') {
            let known = match ToolId::parse(name) {
                Ok(id) if is_adhoc(&id.pack()) => self
                    .adhoc
                    .binding(name)
                    .map_err(mlua::Error::external)?
                    .is_some()
                    .then_some(id),
                Ok(id) => self.set.catalog().get(&id).is_some().then_some(id),
                Err(_) => None,
            };
            return known.map(OfferTarget::Tool).ok_or_else(|| {
                mlua::Error::external(format!("{verb}: {name:?} is not a tool in this run"))
            });
        }
        if let Ok(pack) = PackId::parse(name) {
            if let Some(Some(reason)) = self.set.roster().get(&pack) {
                return Err(mlua::Error::external(format!(
                    "{verb}: the {pack} Pack is unavailable in this run: {reason}"
                )));
            }
            if pack_object(lua, name)?.is_some() {
                return Ok(OfferTarget::Pack(pack));
            }
        }
        let hint = match self.wire_named(name)? {
            Some(id) => format!(" (did you mean '{id}'?)"),
            None => String::new(),
        };
        Err(mlua::Error::external(format!(
            "{verb}: no Pack named {name:?} in this run; declare it under uses: to require \
             it, or offer it only when tools.packs.get({name:?}) returns it{hint}"
        )))
    }

    /// The id of the catalog tool, or else the defined adhoc tool, whose
    /// wire name is `name`.
    fn wire_named(&self, name: &str) -> mlua::Result<Option<ToolId>> {
        if let Some(info) = self
            .set
            .catalog()
            .tools()
            .iter()
            .find(|info| wire_name_of(info) == name)
        {
            return Ok(Some(info.id.clone()));
        }
        Ok(self
            .adhoc
            .binding(name)
            .map_err(mlua::Error::external)?
            .map(|binding| binding.id().clone()))
    }
}

/// The entries of the array `table`: `#t` of them, honoring `__len`, each
/// read through `__index`. A `#t` above [`HELD_KEY_LIMIT`] is refused
/// before any entry is read, since each entry holds a Lua reference until
/// the offer is recorded. A nil or a table in that range, or a raw key
/// outside it, is refused.
fn array(verb: &str, table: &Table) -> mlua::Result<Vec<Value>> {
    let len = table.len()?;
    let count = usize::try_from(len).unwrap_or(if len > 0 { usize::MAX } else { 0 });
    if count > HELD_KEY_LIMIT {
        return Err(mlua::Error::external(format!(
            "{verb} takes an array of at most {HELD_KEY_LIMIT} entries, got {len}"
        )));
    }
    let mut entries = Vec::with_capacity(count);
    for index in 1..=count {
        let entry: Value = table.get(index)?;
        if matches!(entry, Value::Nil | Value::Table(_)) {
            return Err(wrong_type(verb, &entry, Some(index)));
        }
        entries.push(entry);
    }
    for pair in table.pairs::<Value, Value>() {
        let (key, _) = pair?;
        let inside = match key {
            Value::Integer(index) => {
                usize::try_from(index).is_ok_and(|index| (1..=count).contains(&index))
            }
            _ => false,
        };
        if !inside {
            return Err(mlua::Error::external(format!(
                "{verb} takes an array, got a table with the key {}",
                key_text(&key)
            )));
        }
    }
    Ok(entries)
}

/// The refusal of `entry`, at `index` of an array or alone, as a value
/// that names no tool or Pack.
fn wrong_type(verb: &str, entry: &Value, index: Option<usize>) -> mlua::Error {
    let kind = entry.type_name();
    mlua::Error::external(match index {
        Some(index) => format!(
            "{verb} takes tool ids, Pack ids, tool objects, or Pack objects in an array, \
             got {kind} at index {index}"
        ),
        None => format!(
            "{verb} takes tool ids, Pack ids, tool objects, Pack objects, or an array of \
             them, got {kind}"
        ),
    })
}
