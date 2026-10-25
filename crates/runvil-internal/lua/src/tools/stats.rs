//! `tools.stats`: the read-only view of a section's tool-call counts,
//! computed at each read from the one id-keyed [`ToolCallCounts`] map.
//!
//! `total` sums every count. `packs` sums each Pack's ids, so the
//! discovery tools' calls count under `dynamic`. `tools` keeps only the
//! ids Lua can see, a run catalog tool's or an adhoc tool's, so a
//! discovery tool's id and a tool only the model sees stay out of it, out
//! of its walk, and out of the keys its unknown-key error lists. Each of
//! the three is an empty proxy with a protected metatable, as
//! `tools.packs` is: `pairs` walks its keys in order, a key it does not
//! hold raises, naming the key and the keys it holds, and every write
//! raises.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound;
use std::sync::Arc;

use mlua::{Function, Lua, MultiValue, Table, Value};
use runvil_types::tools::ToolId;

use crate::error::{Error, Result};
use crate::handles::ToolSet;
use crate::proxy::key_text;
use crate::scope::ToolCallCounts;
use crate::vm::AdhocTools;

/// The path of the table itself, which its refusals open with.
const STATS: &str = "tools.stats";

/// The keys `tools.stats` holds, in key order.
const STATS_KEYS: [&str; 3] = ["packs", "tools", "total"];

/// What every read computes from: the counts, and what tells the ids Lua
/// can see from the rest.
#[derive(Debug, Clone)]
struct Stats {
    /// The section's one id-keyed count map.
    counts: ToolCallCounts,
    /// Every run catalog tool's id.
    catalog: Arc<BTreeSet<String>>,
    /// The VM's adhoc tools, read at each read, so a tool defined after
    /// the install is one Lua can see.
    adhoc: AdhocTools,
}

/// One of the two count tables under `tools.stats`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    /// `packs`: each Pack id with the sum of its ids' counts.
    Packs,
    /// `tools`: each id Lua can see with its count.
    Tools,
}

impl View {
    /// The table's path, which its refusals open with.
    fn path(self) -> &'static str {
        match self {
            View::Packs => "tools.stats.packs",
            View::Tools => "tools.stats.tools",
        }
    }
}

impl Stats {
    /// Every count, summed.
    fn total(&self) -> Result<u64> {
        Ok(self.counts.snapshot()?.values().sum())
    }

    /// The keys `view` holds, with their counts, from one snapshot.
    fn view(&self, view: View) -> Result<BTreeMap<String, u64>> {
        let snapshot = self.counts.snapshot()?;
        let mut held = BTreeMap::new();
        for (id, count) in snapshot {
            match view {
                View::Packs => {
                    let pack = id.split_once('/').map_or(id.as_str(), |(pack, _)| pack);
                    *held.entry(pack.to_owned()).or_default() += count;
                }
                View::Tools => {
                    if self.catalog.contains(&id) || self.adhoc.binding(&id)?.is_some() {
                        held.insert(id, count);
                    }
                }
            }
        }
        Ok(held)
    }

    /// What a read of `key` from `view`, which holds `held`, raises: the
    /// key and the keys held, then a remedy. In `tools`, a catalog tool's
    /// or an adhoc tool's id takes its own remedy, and with nothing held
    /// there is none.
    fn unseeded(&self, view: View, key: &str, held: &BTreeMap<String, u64>) -> Result<String> {
        let tools = view == View::Tools;
        let hint = if tools && self.catalog.contains(key) {
            " (a catalog tool that was neither offered in this section nor called with tools.call)"
        } else if tools && key.contains('/') && self.adhoc.binding(key)?.is_some() {
            " (an adhoc tool that was neither offered in this section nor called with tools.call)"
        } else if held.is_empty() {
            ""
        } else {
            " - check for typos or offer it with tools.offer"
        };
        let names: Vec<&String> = held.keys().collect();
        Ok(format!(
            "{}: {key:?} has no seeded count; seeded names: {names:?}{hint}",
            view.path()
        ))
    }
}

/// `tools.stats`'s members: the counts `total` sums, and the two count
/// tables.
#[derive(Clone)]
struct Members {
    stats: Stats,
    packs: Table,
    tools: Table,
}

impl Members {
    /// The member under `key`, or `None` for a key `tools.stats` does not
    /// hold.
    fn get(&self, key: &str) -> mlua::Result<Option<Value>> {
        Ok(Some(match key {
            "packs" => Value::Table(self.packs.clone()),
            "tools" => Value::Table(self.tools.clone()),
            "total" => {
                let total = self.stats.total().map_err(mlua::Error::external)?;
                Value::Integer(i64::try_from(total).unwrap_or(i64::MAX))
            }
            _ => return Ok(None),
        }))
    }
}

/// Installs `tools.stats` over a fresh [`ToolCallCounts`] seeded with
/// `ids`, telling the ids Lua can see by `bound_tools`' run catalog and
/// `adhoc`, and returns the counts so the scheduler can increment them.
///
/// # Errors
/// Returns [`Error::LuaRuntime`] when `tools.stats` cannot be built or
/// installed.
pub(crate) fn install_tool_call_counts(
    lua: &Lua,
    bound_tools: &ToolSet,
    adhoc: &AdhocTools,
    ids: &[ToolId],
) -> Result<ToolCallCounts> {
    let counts = ToolCallCounts::new(ids.iter().map(ToString::to_string));
    let stats = Stats {
        counts: counts.clone(),
        catalog: Arc::new(
            bound_tools
                .catalog()
                .tools()
                .iter()
                .map(|info| info.id.to_string())
                .collect(),
        ),
        adhoc: adhoc.clone(),
    };
    let tools: Table = lua.globals().raw_get("tools").map_err(Error::lua)?;
    let proxy = stats_proxy(lua, &stats).map_err(Error::lua)?;
    tools.raw_set("stats", proxy).map_err(Error::lua)?;
    Ok(counts)
}

/// Builds `tools.stats` over `stats`: `packs`, `tools`, and `total`, and
/// a refusal for any other key.
fn stats_proxy(lua: &Lua, stats: &Stats) -> mlua::Result<Table> {
    let members = Members {
        stats: stats.clone(),
        packs: view_proxy(lua, stats, View::Packs)?,
        tools: view_proxy(lua, stats, View::Tools)?,
    };
    let reader = members.clone();
    let index = lua.create_function(move |_, (_, key): (Value, Value)| {
        let member = match &key {
            Value::String(name) => reader.get(&name.to_string_lossy())?,
            _ => None,
        };
        member.ok_or_else(|| {
            mlua::Error::external(format!(
                "{STATS}: unknown key {:?}; accepted keys are total, packs, tools",
                key_text(&key)
            ))
        })
    })?;
    let step = lua.create_function(move |lua, (_, key): (Value, Value)| {
        let next = match &key {
            Value::Nil => 0,
            Value::String(name) => {
                let name = name.to_string_lossy();
                STATS_KEYS
                    .iter()
                    .position(|held| *held == name)
                    .map_or(STATS_KEYS.len(), |at| at + 1)
            }
            _ => STATS_KEYS.len(),
        };
        match STATS_KEYS.get(next) {
            Some(name) => Ok((
                Value::String(lua.create_string(name)?),
                members.get(name)?.unwrap_or(Value::Nil),
            )),
            None => Ok((Value::Nil, Value::Nil)),
        }
    })?;
    proxy(lua, index, step, STATS)
}

/// Builds the table of `view` over `stats`: a key it holds reads its
/// count, any other key raises, and `pairs` walks the held keys in order.
fn view_proxy(lua: &Lua, stats: &Stats, view: View) -> mlua::Result<Table> {
    let reader = stats.clone();
    let index = lua.create_function(move |_, (_, key): (Value, Value)| {
        let held = reader.view(view).map_err(mlua::Error::external)?;
        let count = match &key {
            Value::String(name) => held.get(name.to_string_lossy().as_str()).copied(),
            _ => None,
        };
        match count {
            Some(count) => Ok(count),
            None => Err(mlua::Error::external(
                reader
                    .unseeded(view, &key_text(&key), &held)
                    .map_err(mlua::Error::external)?,
            )),
        }
    })?;
    let walker = stats.clone();
    let step = lua.create_function(move |_, (_, key): (Value, Value)| {
        let held = walker.view(view).map_err(mlua::Error::external)?;
        let next = match &key {
            Value::Nil => held.into_iter().next(),
            Value::String(name) => {
                let after = name.to_string_lossy();
                held.range::<str, _>((Bound::Excluded(after.as_str()), Bound::Unbounded))
                    .next()
                    .map(|(id, count)| (id.clone(), *count))
            }
            _ => None,
        };
        Ok(next.unzip())
    })?;
    proxy(lua, index, step, view.path())
}

/// An empty proxy whose metatable holds `index`, a `__pairs` that walks
/// with `step`, a `__newindex` that raises `<path> is read-only`, and that
/// text as its `__metatable`, which protects it.
fn proxy(lua: &Lua, index: Function, step: Function, path: &str) -> mlua::Result<Table> {
    let label = format!("{path} is read-only");
    let pairs =
        lua.create_function(move |_, proxy: Table| Ok((step.clone(), proxy, Value::Nil)))?;
    let refusal = label.clone();
    let newindex = lua.create_function(move |_, _: MultiValue| -> mlua::Result<()> {
        Err(mlua::Error::external(refusal.clone()))
    })?;
    let meta = lua.create_table()?;
    meta.raw_set("__index", index)?;
    meta.raw_set("__pairs", pairs)?;
    meta.raw_set("__newindex", newindex)?;
    meta.raw_set("__metatable", label)?;
    let proxy = lua.create_table()?;
    proxy.set_metatable(Some(meta))?;
    Ok(proxy)
}
