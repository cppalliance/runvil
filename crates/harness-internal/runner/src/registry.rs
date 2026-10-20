//! The Plugins a Host can install: [`PluginRegistry`], holding the
//! built-ins from the start and each linked Plugin the Host adds, and the
//! [`PluginKind`] that says which a Plugin is.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::sync::Arc;

use runvil_plugin::{
    HostServices, PackId, Plugin, PluginFuture, PluginInfo, ToolContext, ToolError, ToolInfo,
    ToolOutput,
};
use serde_json::Value;

use crate::host::InstallError;

#[cfg(test)]
#[path = "registry-tests.rs"]
mod tests;

/// Every Plugin a Host can install, by fully qualified name.
///
/// Every registry holds the built-ins, such as `runvil/subagents`, from
/// the start. A Host adds the `PLUGIN` constant of each Plugin crate it
/// links, lists what the registry holds, and builds its
/// [`HostContext`](crate::HostContext) over it, which installs Plugins by
/// FQN.
#[derive(Debug)]
pub struct PluginRegistry {
    entries: BTreeMap<&'static str, (PluginInfo, PluginKind, PackId)>,
}

/// Where a Plugin comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PluginKind {
    /// Built into every registry; its behavior will live in the Engine, and
    /// today's one built-in, `runvil/subagents`, has none yet.
    Builtin,
    /// Added by the Host with [`PluginRegistry::add`].
    Linked,
}

impl Default for PluginRegistry {
    /// A registry holding only the built-ins.
    fn default() -> PluginRegistry {
        let entries = BUILTIN
            .into_iter()
            .filter_map(|info| {
                let default = default_id(info.fqn)?;
                Some((info.fqn, (info, PluginKind::Builtin, default)))
            })
            .collect();
        PluginRegistry { entries }
    }
}

impl PluginRegistry {
    /// A registry holding only the built-ins.
    #[must_use]
    pub fn new() -> PluginRegistry {
        PluginRegistry::default()
    }

    /// Adds `info` as a linked Plugin, so a Host can install it by its
    /// FQN.
    ///
    /// # Errors
    /// Returns [`InstallError::InvalidFqn`] when the FQN is not a
    /// `vendor/name` pair of valid one-segment names, and
    /// [`InstallError::DuplicatePlugin`] when the registry holds the FQN
    /// already, as it holds each built-in's from the start.
    pub fn add(&mut self, info: PluginInfo) -> Result<(), InstallError> {
        let default = default_id(info.fqn).ok_or(InstallError::InvalidFqn { fqn: info.fqn })?;
        match self.entries.entry(info.fqn) {
            Entry::Occupied(_) => Err(InstallError::DuplicatePlugin { fqn: info.fqn }),
            Entry::Vacant(slot) => {
                slot.insert((info, PluginKind::Linked, default));
                Ok(())
            }
        }
    }

    /// Every Plugin the registry holds, built-ins included, with its kind,
    /// in FQN order.
    pub fn list(&self) -> impl Iterator<Item = (&PluginInfo, PluginKind)> {
        self.entries.values().map(|(info, kind, _)| (info, *kind))
    }

    /// The Plugin with the FQN `fqn`, its kind, and its default Plugin id.
    pub(crate) fn entry(&self, fqn: &str) -> Option<(PluginInfo, PluginKind, &PackId)> {
        let (info, kind, default) = self.entries.get(fqn)?;
        Some((*info, *kind, default))
    }

    /// Whether `name` is a built-in's default Plugin id, which only that
    /// built-in may be installed under.
    pub(crate) fn reserves(&self, name: &PackId) -> bool {
        self.entries
            .values()
            .any(|(_, kind, default)| *kind == PluginKind::Builtin && default == name)
    }
}

/// The built-ins every registry holds from the start.
///
/// A built-in Plugin's behavior and tools live in the Engine. Its Plugin
/// object exists only so install, readiness, and the run snapshot treat it
/// like any other Plugin, so a built-in must never offer tools, carry a
/// prelude, or need services. Its `call` is unreachable, because the run
/// snapshot routes a call only to a tool in a Plugin's own list.
const BUILTIN: [PluginInfo; 1] = [PluginInfo::new("runvil/subagents", builtin).description(
    "Reserves the subagents name for the Engine's subagent tools, which are not built yet",
)];

/// Builds a built-in's [`Builtin`] object; see [`BUILTIN`].
#[expect(
    clippy::unnecessary_wraps,
    reason = "the PluginInfo construct signature fixes the return type"
)]
fn builtin(
    _name: &PackId,
    _config: Value,
    _services: &HostServices,
) -> Result<Arc<dyn Plugin>, ToolError> {
    Ok(Arc::new(Builtin))
}

/// The object of a built-in Plugin, which offers no tools; see [`BUILTIN`].
struct Builtin;

impl Plugin for Builtin {
    fn tools(&self) -> Vec<ToolInfo> {
        Vec::new()
    }

    fn call<'a>(
        &'a self,
        cx: ToolContext<'a>,
        _args: Value,
    ) -> PluginFuture<'a, Result<ToolOutput, ToolError>> {
        let message = format!(
            "{} belongs to a built-in Plugin, whose calls the Engine answers",
            cx.tool()
        );
        Box::pin(async move { Err(ToolError::message(message)) })
    }
}

/// The default Plugin id for the FQN `vendor/name`: its second segment,
/// when both segments are valid one-segment names.
pub(crate) fn default_id(fqn: &str) -> Option<PackId> {
    let (vendor, name) = fqn.split_once('/')?;
    PackId::parse(vendor).ok()?;
    PackId::parse(name).ok()
}
