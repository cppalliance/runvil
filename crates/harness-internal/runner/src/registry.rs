//! The Packs a Host can install: [`PackRegistry`], holding the
//! built-ins from the start and each linked Pack the Host adds, and the
//! [`PackOrigin`] that says which a Pack is.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::sync::Arc;

use runvil_plugin::{
    HostServices, Pack, PackFuture, PackId, PackInfo, ToolContext, ToolError, ToolInfo, ToolOutput,
};
use serde_json::Value;

use crate::host::InstallError;

#[cfg(test)]
#[path = "registry-tests.rs"]
mod tests;

/// Every Pack a Host can install, by fully qualified name.
///
/// Every registry holds the built-ins, such as `runvil/subagents`, from
/// the start. A Host adds the `PLUGIN` constant of each Plugin crate it
/// links, lists what the registry holds, and builds its
/// [`HostContext`](crate::HostContext) over it, which installs Packs by
/// FQN.
#[derive(Debug)]
pub struct PackRegistry {
    entries: BTreeMap<&'static str, (PackInfo, PackOrigin, PackId)>,
}

/// Where a Pack comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PackOrigin {
    /// Built into every registry; its behavior will live in the Engine, and
    /// today's one built-in, `runvil/subagents`, has none yet.
    Builtin,
    /// Added by the Host with [`PackRegistry::add`].
    Linked,
}

impl Default for PackRegistry {
    /// A registry holding only the built-ins.
    fn default() -> PackRegistry {
        let entries = BUILTIN
            .into_iter()
            .filter_map(|info| {
                let default = default_id(info.fqn)?;
                Some((info.fqn, (info, PackOrigin::Builtin, default)))
            })
            .collect();
        PackRegistry { entries }
    }
}

impl PackRegistry {
    /// A registry holding only the built-ins.
    #[must_use]
    pub fn new() -> PackRegistry {
        PackRegistry::default()
    }

    /// Adds `info` as a linked Pack, so a Host can install it by its
    /// FQN.
    ///
    /// # Errors
    /// Returns [`InstallError::InvalidFqn`] when the FQN is not a
    /// `vendor/name` pair of valid one-segment names, and
    /// [`InstallError::DuplicatePack`] when the registry holds the FQN
    /// already, as it holds each built-in's from the start.
    pub fn add(&mut self, info: PackInfo) -> Result<(), InstallError> {
        let default = default_id(info.fqn).ok_or(InstallError::InvalidFqn { fqn: info.fqn })?;
        match self.entries.entry(info.fqn) {
            Entry::Occupied(_) => Err(InstallError::DuplicatePack { fqn: info.fqn }),
            Entry::Vacant(slot) => {
                slot.insert((info, PackOrigin::Linked, default));
                Ok(())
            }
        }
    }

    /// Every Pack the registry holds, built-ins included, with its kind,
    /// in FQN order.
    pub fn list(&self) -> impl Iterator<Item = (&PackInfo, PackOrigin)> {
        self.entries.values().map(|(info, kind, _)| (info, *kind))
    }

    /// The Pack with the FQN `fqn`, its kind, and its default Pack id.
    pub(crate) fn entry(&self, fqn: &str) -> Option<(PackInfo, PackOrigin, &PackId)> {
        let (info, kind, default) = self.entries.get(fqn)?;
        Some((*info, *kind, default))
    }

    /// Whether `name` is a built-in's default Pack id, which only that
    /// built-in may be installed under.
    pub(crate) fn reserves(&self, name: &PackId) -> bool {
        self.entries
            .values()
            .any(|(_, kind, default)| *kind == PackOrigin::Builtin && default == name)
    }
}

/// The built-ins every registry holds from the start.
///
/// A built-in Pack's behavior and tools live in the Engine. Its Pack
/// object exists only so install, readiness, and the run snapshot treat it
/// like any other Pack, so a built-in must never offer tools, carry a
/// prelude, or need services. Its `call` is unreachable, because the run
/// snapshot routes a call only to a tool in a Pack's own list.
const BUILTIN: [PackInfo; 1] = [PackInfo::new("runvil/subagents", builtin).description(
    "Reserves the subagents name for the Engine's subagent tools, which are not built yet",
)];

/// Builds a built-in's [`Builtin`] object; see [`BUILTIN`].
#[expect(
    clippy::unnecessary_wraps,
    reason = "the PackInfo construct signature fixes the return type"
)]
fn builtin(
    _name: &PackId,
    _config: Value,
    _services: &HostServices,
) -> Result<Arc<dyn Pack>, ToolError> {
    Ok(Arc::new(Builtin))
}

/// The object of a built-in Pack, which offers no tools; see [`BUILTIN`].
struct Builtin;

impl Pack for Builtin {
    fn tools(&self) -> Vec<ToolInfo> {
        Vec::new()
    }

    fn call<'a>(
        &'a self,
        cx: ToolContext<'a>,
        _args: Value,
    ) -> PackFuture<'a, Result<ToolOutput, ToolError>> {
        let message = format!(
            "{} belongs to a built-in Pack, whose calls the Engine answers",
            cx.tool()
        );
        Box::pin(async move { Err(ToolError::message(message)) })
    }
}

/// The default Pack id for the FQN `vendor/name`: its second segment,
/// when both segments are valid one-segment names.
pub(crate) fn default_id(fqn: &str) -> Option<PackId> {
    let (vendor, name) = fqn.split_once('/')?;
    PackId::parse(vendor).ok()?;
    PackId::parse(name).ok()
}
