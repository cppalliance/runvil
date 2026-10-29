//! Section VM state reads: `var`, bare globals, `argv`, the tool call
//! counts, the frozen sets' entries, the section's offered tools and the
//! catalog Packs they come from, and its effective model binding.

use std::borrow::Cow;
use std::sync::{Arc, Mutex};

use mlua::{LuaSerdeExt, Value};
use runvil_model_client::model::{ModelBinding, ModelView};
use runvil_types::packs::PackId;
use runvil_types::tools::{ToolId, ToolInfo};
use serde_json::Value as Json;

use super::{AdhocTools, SectionVm};
use crate::bindings::{ToolBinding, ToolSet};
use crate::error::{Error, Result};
use crate::models::ModelSelection;
use crate::packs::is_adhoc;
use crate::scope::{OfferMode, OfferTarget, OfferedTool, Offers, RenderedTools, ToolCallCounts};
use crate::tools::install_tool_call_counts as install_tool_call_counts_impl;
use crate::var::var_to_json;

impl SectionVm {
    /// Returns the current `var` table as JSON, read from the hidden data
    /// table behind the guarded proxy (not the proxy, which stays empty).
    ///
    /// # Errors
    /// Returns [`Error::Lua`] if the author reassigned the `var` global, or
    /// [`Error::LuaRuntime`] if `var` cannot be represented as JSON.
    pub fn var(&self) -> Result<Json> {
        var_to_json(&self.lua)
    }

    /// Reads the `argv` global back as JSON at the H1 freeze: `None` when
    /// nil, its JSON form otherwise. Call this on the H1 VM only - a frozen
    /// section's `argv` sits behind the guard proxy, which is not the
    /// read-back path.
    ///
    /// # Errors
    /// Returns [`Error::Lua`] when H1 left `argv` as a function, userdata,
    /// or thread, and [`Error::LuaRuntime`] when its value cannot be
    /// represented as JSON.
    pub fn argv_json(&self) -> Result<Option<Json>> {
        let value: Value = self.lua.globals().get("argv").map_err(Error::lua)?;
        match value {
            Value::Nil => Ok(None),
            Value::Function(_) | Value::UserData(_) | Value::Thread(_) => Err(Error::Lua(format!(
                "argv must be JSON data, got {}",
                value.type_name()
            ))),
            other => Ok(Some(self.lua.from_value(other).map_err(Error::lua)?)),
        }
    }

    /// Sets a global in the VM to the Lua form of a JSON value, overwriting
    /// any existing value.
    ///
    /// Used by fanout to inject `item` after Engine injection; the conversion
    /// is the same `LuaSerdeExt` bridge that seeds `var`.
    ///
    /// # Errors
    /// Returns [`Error::LuaRuntime`] if the value cannot convert or the global
    /// cannot be set.
    pub fn set_global_json(&self, name: &str, value: &Json) -> Result<()> {
        let value = self.lua.to_value(value).map_err(Error::lua)?;
        self.lua.globals().raw_set(name, value).map_err(Error::lua)
    }

    /// Installs `tools.stats`, the read-only Lua view of a fresh
    /// [`ToolCallCounts`] seeded with `ids`: its `total`, and its `packs`
    /// and `tools` tables, each computed from the counts at each read.
    /// Reading a key a table does not hold is a hard error that names the
    /// key and lists the keys held, and `tools` holds only run catalog and
    /// adhoc tool ids.
    ///
    /// The installation itself sits in the `tools` module; this method only
    /// supplies the VM's own state.
    ///
    /// Returns the `ToolCallCounts` handle so the scheduler can increment
    /// it.
    ///
    /// # Errors
    /// Returns [`Error::LuaRuntime`] when installing `tools.stats` fails.
    pub fn install_tool_call_counts(&self, ids: &[ToolId]) -> Result<ToolCallCounts> {
        install_tool_call_counts_impl(&self.lua, &self.tool_set, &self.adhoc_tools, ids)
    }

    /// Returns the binding the section's frozen sets hold under
    /// `wire_name`, with the set that holds it, or `None` when the section
    /// has not frozen its sets or neither set holds that name.
    #[must_use]
    pub fn rendered_target(&self, wire_name: &str) -> Option<(ToolBinding, OfferMode)> {
        self.messages.rendered_target(wire_name)
    }

    /// Returns the ids of both frozen sets' tools, the Static Set's first,
    /// or `None` when the section has not frozen its sets.
    #[must_use]
    pub fn rendered_ids(&self) -> Option<Vec<ToolId>> {
        let rendered = self.messages.rendered_tools()?;
        Some(
            rendered
                .static_set
                .iter()
                .chain(&rendered.dynamic_set)
                .map(|binding| binding.id().clone())
                .collect(),
        )
    }

    /// Returns the section's frozen sets, or `None` when it has not frozen
    /// them.
    #[must_use]
    pub fn rendered_tools(&self) -> Option<Arc<RenderedTools>> {
        self.messages.rendered_tools()
    }
}

/// Reads the tools the section's static offers name without mutating the
/// offers: each static offer in call order, a tool adding itself
/// and a Pack adding its tools in id order, each tool once, at its first
/// place. Catalog ids resolve against the run catalog and adhoc ids
/// against `adhoc`.
///
/// The section's first loop round freezes its Static Set from these
/// tools. An offered adhoc Pack expands to the adhoc tools defined when
/// this function runs, so after the freeze the Engine reads the frozen
/// sets instead.
///
/// # Errors
/// Returns [`Error::Internal`] if the offers' mutex or the adhoc
/// tools are poisoned, or an offered id names no tool.
pub fn current_static_set(
    set: &ToolSet,
    runtime: &Mutex<Offers>,
    adhoc: &AdhocTools,
) -> Result<Vec<OfferedTool>> {
    let mut tools: Vec<OfferedTool> = Vec::new();
    for tool in expanded(set, runtime, adhoc, Some(OfferMode::Static))? {
        if !tools.iter().any(|held| held.id() == tool.id()) {
            tools.push(tool);
        }
    }
    Ok(tools)
}

/// Reads the tools the section's dynamic offers name without mutating
/// the offers: each tool once, in offer order, unless `static_tools`
/// holds its id.
///
/// # Errors
/// Returns [`Error::Internal`] if the offers' mutex or the adhoc
/// tools are poisoned, or an offered id names no tool.
pub fn current_dynamic_set(
    set: &ToolSet,
    runtime: &Mutex<Offers>,
    adhoc: &AdhocTools,
    static_tools: &[OfferedTool],
) -> Result<Vec<OfferedTool>> {
    let mut tools: Vec<OfferedTool> = Vec::new();
    for tool in expanded(set, runtime, adhoc, Some(OfferMode::Dynamic))? {
        let held = static_tools
            .iter()
            .chain(&tools)
            .any(|held| held.id() == tool.id());
        if !held {
            tools.push(tool);
        }
    }
    Ok(tools)
}

/// Reads the catalog Packs the section's offers name without mutating
/// the offers: each Pack once, in the order the section first
/// offered one of its tools, into either set, which is the order a
/// section entry's render asks its Packs in.
///
/// # Errors
/// Returns [`Error::Internal`] if the offers' mutex or the adhoc
/// tools are poisoned, or an offered id names no tool.
pub fn current_catalog_packs(
    set: &ToolSet,
    runtime: &Mutex<Offers>,
    adhoc: &AdhocTools,
) -> Result<Vec<PackId>> {
    let mut packs: Vec<PackId> = Vec::new();
    for tool in expanded(set, runtime, adhoc, None)? {
        if let OfferedTool::Catalog(info) = tool {
            let pack = info.id.pack();
            if !packs.contains(&pack) {
                packs.push(pack);
            }
        }
    }
    Ok(packs)
}

/// Every tool the offers of `mode` name, or every offer's tools when
/// `mode` is `None`, in call order, with repeats: the one expansion both
/// sets and the catalog Packs read, so they never disagree about what an
/// offer names.
fn expanded(
    set: &ToolSet,
    runtime: &Mutex<Offers>,
    adhoc: &AdhocTools,
    mode: Option<OfferMode>,
) -> Result<Vec<OfferedTool>> {
    let runtime = runtime
        .lock()
        .map_err(|_| Error::Internal("section offers lock was poisoned".into()))?;
    let mut tools = Vec::new();
    let offers = runtime
        .offers
        .iter()
        .filter(|offer| mode.is_none_or(|mode| offer.mode == mode));
    for offer in offers {
        match &offer.target {
            OfferTarget::Tool(id) => {
                let tool = if is_adhoc(&id.pack()) {
                    adhoc.binding(&id.to_string())?.map(OfferedTool::Adhoc)
                } else {
                    set.catalog().get(id).cloned().map(OfferedTool::Catalog)
                };
                tools.push(tool.ok_or_else(|| {
                    Error::Internal(Cow::Owned(format!("offered tool {id} names no tool")))
                })?);
            }
            OfferTarget::Pack(pack) if is_adhoc(pack) => {
                tools.extend(adhoc.bindings()?.into_iter().map(OfferedTool::Adhoc));
            }
            OfferTarget::Pack(pack) => {
                let mut infos: Vec<&ToolInfo> = set
                    .catalog()
                    .tools()
                    .iter()
                    .filter(|info| info.id.pack() == *pack)
                    .collect();
                infos.sort_by(|left, right| left.id.cmp(&right.id));
                tools.extend(infos.into_iter().cloned().map(OfferedTool::Catalog));
            }
        }
    }
    Ok(tools)
}

/// Reads the section's effective model binding through the run's model view
/// without mutating the model selection: the H2 `models.use` selection with
/// its options applied, else the prompt-wide `models.default` baseline.
///
/// # Errors
/// Returns [`Error::Internal`] if the model selection's mutex or the shared
/// model set is poisoned, or the selected alias has no frozen binding.
pub fn resolve_model_binding(
    bindings: &dyn ModelView,
    runtime: &Mutex<ModelSelection>,
) -> Result<Option<ModelBinding>> {
    let selection = {
        let runtime = runtime
            .lock()
            .map_err(|_| Error::Internal("model selection lock was poisoned".into()))?;
        runtime
            .selection()
            .map(|(alias, options)| (alias.as_str().to_owned(), options))
    };
    let frozen = |alias: &str| -> Result<ModelBinding> {
        bindings.binding(alias)?.ok_or_else(|| {
            Error::Internal(Cow::Owned(format!(
                "model alias {alias:?} has no frozen binding"
            )))
        })
    };
    match selection {
        Some((alias, options)) => Ok(Some(options.apply(frozen(&alias)?))),
        None => match bindings.default()? {
            Some(alias) => Ok(Some(frozen(&alias)?)),
            None => Ok(None),
        },
    }
}
