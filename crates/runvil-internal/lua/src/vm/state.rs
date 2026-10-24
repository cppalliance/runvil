//! Section VM state reads: `var`, bare globals, `argv`, the tool call
//! counts, the rendered list's entries, and the section's effective tool
//! and model bindings.

use std::borrow::Cow;
use std::sync::Mutex;

use mlua::{LuaSerdeExt, Value};
use runvil_model_client::model::{ModelBinding, ModelView};
use runvil_types::tools::ToolId;
use serde_json::Value as Json;

use super::{AdhocTools, SectionVm};
use crate::error::{Error, Result};
use crate::handles::{ToolBinding, ToolSet};
use crate::models::ModelRuntime;
use crate::packs::is_adhoc;
use crate::scope::{Advertised, OfferMode, OfferTarget, ToolCallCounts, ToolRuntime};
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
    /// of a tool the run offers but was never seeded - neither offered to
    /// the section nor dispatched by a script `tools.call` - the diagnostic
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

    /// Returns the tool the section's rendered list advertises under
    /// `wire_name`, or `None` when no list is rendered or it has no such
    /// name.
    #[must_use]
    pub fn rendered_target(&self, wire_name: &str) -> Option<Advertised> {
        self.messages.rendered_target(wire_name)
    }

    /// Returns the ids of the tools the section's rendered list
    /// advertises, never its dynamic ones, or `None` when no list is
    /// rendered.
    #[must_use]
    pub fn rendered_ids(&self) -> Option<Vec<ToolId>> {
        let rendered = self.messages.rendered_tools()?;
        Some(
            rendered
                .dispatch
                .into_values()
                .map(|advertised| advertised.id)
                .collect(),
        )
    }

    /// How many of this VM's instruction-hook firings have counted.
    #[cfg(test)]
    pub(crate) fn hook_firings(&self) -> u64 {
        self.instruction_budget.fired()
    }
}

/// Reads the section's static bindings without mutating the tool
/// runtime: each static offer in call order, a tool adding itself and a
/// Pack adding its tools in id order, each tool once, at its first place.
/// Catalog ids resolve against the offering and adhoc ids against
/// `adhoc`. Last, an offered catalog tool whose wire name an offered adhoc
/// tool shares is dropped.
///
/// The section's first loop round renders its tool list from these
/// bindings. An offered adhoc Pack expands to the adhoc tools defined
/// when this function runs, so after the render the Engine reads the
/// rendered list instead.
///
/// # Errors
/// Returns [`Error::Internal`] if the tool runtime's mutex or the adhoc
/// tools are poisoned, or an offered id has no binding.
pub fn current_tool_bindings(
    set: &ToolSet,
    runtime: &Mutex<ToolRuntime>,
    adhoc: &AdhocTools,
) -> Result<Vec<ToolBinding>> {
    let mut bindings: Vec<ToolBinding> = Vec::new();
    for binding in expanded(set, runtime, adhoc, OfferMode::Static)? {
        if !bindings.iter().any(|held| held.id() == binding.id()) {
            bindings.push(binding);
        }
    }
    let adhoc_names: Vec<String> = bindings
        .iter()
        .filter(|binding| is_adhoc(&binding.id().pack()))
        .map(|binding| binding.alias().to_owned())
        .collect();
    bindings.retain(|binding| {
        is_adhoc(&binding.id().pack()) || !adhoc_names.iter().any(|name| name == binding.alias())
    });
    Ok(bindings)
}

/// Reads the section's dynamic list without mutating the tool runtime:
/// each tool the dynamic offers name, in offer order and once, unless
/// `static_bindings` holds its id or its wire name.
///
/// # Errors
/// Returns [`Error::Internal`] if the tool runtime's mutex or the adhoc
/// tools are poisoned, or an offered id has no binding.
pub fn current_dynamic_tools(
    set: &ToolSet,
    runtime: &Mutex<ToolRuntime>,
    adhoc: &AdhocTools,
    static_bindings: &[ToolBinding],
) -> Result<Vec<ToolId>> {
    let mut dynamic: Vec<ToolId> = Vec::new();
    for binding in expanded(set, runtime, adhoc, OfferMode::Dynamic)? {
        let in_static = static_bindings
            .iter()
            .any(|held| held.id() == binding.id() || held.alias() == binding.alias());
        if !in_static && !dynamic.contains(binding.id()) {
            dynamic.push(binding.id().clone());
        }
    }
    Ok(dynamic)
}

/// Every binding the offers of `mode` name, in call order, with repeats:
/// the one expansion both lists read, so they never disagree about what
/// an offer names.
fn expanded(
    set: &ToolSet,
    runtime: &Mutex<ToolRuntime>,
    adhoc: &AdhocTools,
    mode: OfferMode,
) -> Result<Vec<ToolBinding>> {
    let runtime = runtime
        .lock()
        .map_err(|_| Error::Internal("tool declaration runtime was poisoned".into()))?;
    let mut bindings = Vec::new();
    for offer in runtime.offers.iter().filter(|offer| offer.mode == mode) {
        match &offer.target {
            OfferTarget::Tool(id) => {
                let name = id.to_string();
                let binding = if is_adhoc(&id.pack()) {
                    adhoc.binding(&name)?
                } else {
                    set.offered_binding(&name).cloned()
                };
                bindings.push(binding.ok_or_else(|| {
                    Error::Internal(Cow::Owned(format!("offered tool {name:?} has no binding")))
                })?);
            }
            OfferTarget::Pack(pack) if is_adhoc(pack) => bindings.extend(adhoc.bindings()?),
            OfferTarget::Pack(pack) => bindings.extend(
                set.offered()
                    .iter()
                    .filter(|binding| binding.id().pack() == *pack)
                    .cloned(),
            ),
        }
    }
    Ok(bindings)
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
