//! Section VM state reads: `var`, bare globals, `argv`, the tool call
//! counts, the frozen sets' entries, the section's offered tools, and its
//! effective model binding.

use std::borrow::Cow;
use std::sync::{Arc, Mutex};

use mlua::{LuaSerdeExt, Value};
use runvil_model_client::model::{ModelBinding, ModelView};
use runvil_types::tools::{ToolId, ToolInfo};
use serde_json::Value as Json;

use super::{AdhocTools, SectionVm};
use crate::error::{Error, Result};
use crate::handles::{ToolBinding, ToolSet};
use crate::models::ModelRuntime;
use crate::packs::is_adhoc;
use crate::scope::{
    OfferMode, OfferTarget, OfferedTool, RenderedTools, ToolCallCounts, ToolRuntime,
};
use crate::sys::var_to_json;
use crate::tools::install_tool_call_counts as install_tool_call_counts_impl;

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
    /// or thread, or when its value cannot be represented as JSON.
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

    /// Installs `tools.calls` as a read-only Lua table backed by a fresh
    /// [`ToolCallCounts`] seeded with `ids`. Each seeded key
    /// reads its live count; indexing an unseeded key is a hard error that
    /// names the bad key and lists the seeded set. When the key is the id
    /// of a catalog tool that was never seeded - neither offered to the
    /// section nor dispatched by a script `tools.call` - the diagnostic
    /// says so.
    ///
    /// The installation itself sits in the `tools` module; this method only
    /// supplies the VM's own state.
    ///
    /// Returns the `ToolCallCounts` handle so the executor's tool loop can
    /// increment it.
    ///
    /// # Errors
    /// Returns [`Error::LuaRuntime`] when installing the `tools.calls` index fails.
    pub fn install_tool_call_counts(&self, ids: &[ToolId]) -> Result<ToolCallCounts> {
        install_tool_call_counts_impl(&self.lua, &self.bound_tools, &self.adhoc_tools, ids)
    }

    /// Returns the binding the section's Static Set holds under
    /// `wire_name`, or `None` when the section has not frozen its sets or
    /// its Static Set does not hold that name.
    #[must_use]
    pub fn rendered_target(&self, wire_name: &str) -> Option<ToolBinding> {
        self.messages.rendered_target(wire_name)
    }

    /// Returns the ids of the Static Set's tools, or `None` when the
    /// section has not frozen its sets.
    #[must_use]
    pub fn rendered_ids(&self) -> Option<Vec<ToolId>> {
        let rendered = self.messages.rendered_tools()?;
        Some(
            rendered
                .static_set
                .iter()
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

    /// How many of this VM's instruction-hook firings have counted.
    #[cfg(test)]
    pub(crate) fn hook_firings(&self) -> u64 {
        self.instruction_budget.fired()
    }
}

/// Reads the tools the section's static offers name without mutating the
/// tool runtime: each static offer in call order, a tool adding itself
/// and a Pack adding its tools in id order, each tool once, at its first
/// place. Catalog ids resolve against the run catalog and adhoc ids
/// against `adhoc`.
///
/// The section's first model call freezes its Static Set from these
/// tools. An offered adhoc Pack expands to the adhoc tools defined when
/// this function runs, so after the freeze the Engine reads the frozen
/// sets instead.
///
/// # Errors
/// Returns [`Error::Internal`] if the tool runtime's mutex or the adhoc
/// tools are poisoned, or an offered id names no tool.
pub fn current_tool_bindings(
    set: &ToolSet,
    runtime: &Mutex<ToolRuntime>,
    adhoc: &AdhocTools,
) -> Result<Vec<OfferedTool>> {
    let mut tools: Vec<OfferedTool> = Vec::new();
    for tool in expanded(set, runtime, adhoc, OfferMode::Static)? {
        if !tools.iter().any(|held| held.id() == tool.id()) {
            tools.push(tool);
        }
    }
    Ok(tools)
}

/// Reads the tools the section's dynamic offers name without mutating
/// the tool runtime: each tool once, in offer order, unless `static_tools`
/// holds its id.
///
/// # Errors
/// Returns [`Error::Internal`] if the tool runtime's mutex or the adhoc
/// tools are poisoned, or an offered id names no tool.
pub fn current_dynamic_set(
    set: &ToolSet,
    runtime: &Mutex<ToolRuntime>,
    adhoc: &AdhocTools,
    static_tools: &[OfferedTool],
) -> Result<Vec<OfferedTool>> {
    let mut tools: Vec<OfferedTool> = Vec::new();
    for tool in expanded(set, runtime, adhoc, OfferMode::Dynamic)? {
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

/// Every tool the offers of `mode` name, in call order, with repeats:
/// the one expansion both sets read, so they never disagree about what
/// an offer names.
fn expanded(
    set: &ToolSet,
    runtime: &Mutex<ToolRuntime>,
    adhoc: &AdhocTools,
    mode: OfferMode,
) -> Result<Vec<OfferedTool>> {
    let runtime = runtime
        .lock()
        .map_err(|_| Error::Internal("tool declaration runtime was poisoned".into()))?;
    let mut tools = Vec::new();
    for offer in runtime.offers.iter().filter(|offer| offer.mode == mode) {
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
/// without mutating the model runtime: the H2 `models.use` selection with
/// its options applied, else the prompt-wide `models.default` baseline.
///
/// # Errors
/// Returns [`Error::Internal`] if the model runtime's mutex or the shared
/// model set is poisoned, or the selected alias has no frozen binding.
pub fn resolve_model_binding(
    bindings: &dyn ModelView,
    runtime: &Mutex<ModelRuntime>,
) -> Result<Option<ModelBinding>> {
    let selection = {
        let runtime = runtime
            .lock()
            .map_err(|_| Error::Internal("model declaration runtime was poisoned".into()))?;
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
