//! The Packs a Host builds: [`PackRegistry`], holding the built-ins
//! from the start and each Pack a Plugin adds with its config, the
//! [`PackOrigin`] that says which a Pack is, and the [`InstallError`] an
//! add refuses with.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use runvil_types::packs::PackId;
use runvil_types::tools::{ToolError, ToolInfo, ToolOutput};
use serde_json::Value;

use crate::{HostServices, Pack, PackFuture, PackInfo, ToolContext};

#[cfg(test)]
#[path = "registry-tests.rs"]
mod tests;

/// Every Pack a Host builds, by fully qualified name, each with its
/// Pack id and its config.
///
/// Every registry holds the built-ins, such as `runvil/subagents`, from
/// the start. A Host makes one with [`PackRegistry::new`] and calls each
/// Plugin it links, which adds its Packs with [`PackRegistry::add`]. The
/// Host then builds every Pack the registry holds, through
/// [`into_entries`](PackRegistry::into_entries).
pub struct PackRegistry {
    entries: BTreeMap<&'static str, Registered>,
}

/// One Pack the registry holds.
struct Registered {
    info: PackInfo,
    origin: PackOrigin,
    id: PackId,
    config: Value,
}

/// Where a Pack comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PackOrigin {
    /// Built into every registry; its behavior will live in the Engine, and
    /// today's one built-in, `runvil/subagents`, has none yet.
    Builtin,
    /// Added by a linked Plugin with [`PackRegistry::add`].
    Linked,
}

impl Default for PackRegistry {
    /// A registry holding only the built-ins.
    fn default() -> PackRegistry {
        let entries = BUILTIN
            .into_iter()
            .filter_map(|info| {
                let id = pack_id(info.fqn)?;
                let registered = Registered {
                    info,
                    origin: PackOrigin::Builtin,
                    id,
                    config: Value::Null,
                };
                Some((info.fqn, registered))
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

    /// Adds `info` as a linked Pack with `config`, which its `construct`
    /// receives when the Host builds it.
    ///
    /// The Pack id is the FQN's second segment, so `runvil/web` is `web`,
    /// and the registry may hold Packs of different vendors with the same
    /// Pack id, such as `runvil/web` and `acme/web`.
    ///
    /// # Errors
    /// Returns [`InstallError::InvalidFqn`] when the FQN is not a
    /// `vendor/name` pair of valid one-segment names,
    /// [`InstallError::DuplicatePack`] when the registry holds the FQN
    /// already, as it holds each built-in's from the start, and
    /// [`InstallError::ReservedName`] when the Pack id is a built-in's.
    pub fn add(&mut self, info: PackInfo, config: Value) -> Result<(), InstallError> {
        let id = pack_id(info.fqn).ok_or(InstallError::InvalidFqn { fqn: info.fqn })?;
        if self.entries.contains_key(info.fqn) {
            return Err(InstallError::DuplicatePack { fqn: info.fqn });
        }
        if self.reserves(&id) {
            return Err(InstallError::ReservedName {
                fqn: info.fqn,
                name: id,
            });
        }
        self.entries.insert(
            info.fqn,
            Registered {
                info,
                origin: PackOrigin::Linked,
                id,
                config,
            },
        );
        Ok(())
    }

    /// Every Pack the registry holds, built-ins included, with its origin,
    /// in FQN order.
    pub fn list(&self) -> impl Iterator<Item = (&PackInfo, PackOrigin)> {
        self.entries
            .values()
            .map(|registered| (&registered.info, registered.origin))
    }

    /// Every Pack the registry holds, built-ins included, with its Pack
    /// id and its config, in FQN order: what a Host builds its Packs from.
    pub fn into_entries(self) -> impl Iterator<Item = (PackInfo, PackId, Value)> {
        self.entries
            .into_values()
            .map(|registered| (registered.info, registered.id, registered.config))
    }

    /// Whether `id` is a built-in's Pack id, which no other Pack may
    /// take.
    fn reserves(&self, id: &PackId) -> bool {
        self.entries
            .values()
            .any(|registered| registered.origin == PackOrigin::Builtin && registered.id == *id)
    }
}

impl fmt::Debug for PackRegistry {
    /// Shows each Pack's FQN, origin, and Pack id, and never its config,
    /// which may hold a credential such as a bearer token.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let entries: Vec<(&str, PackOrigin, &PackId)> = self
            .entries
            .values()
            .map(|registered| (registered.info.fqn, registered.origin, &registered.id))
            .collect();
        f.debug_struct("PackRegistry")
            .field("entries", &entries)
            .finish_non_exhaustive()
    }
}

/// A Pack [`PackRegistry::add`] refuses outright, so the Host's author
/// sees it at startup.
///
/// A Pack whose `construct` fails is not one of these; the Host builds it
/// as unavailable instead.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InstallError {
    /// The added Pack's FQN is not `vendor/name`, or a segment of it is
    /// not a valid one-segment name.
    #[error("Pack fully qualified name `{fqn}` is not a vendor/name pair")]
    InvalidFqn {
        /// The FQN as its `PackInfo` spells it.
        fqn: &'static str,
    },
    /// The registry holds the added Pack's FQN already, as a built-in or
    /// as a Pack added before.
    #[error("Pack `{fqn}` is already in the registry")]
    DuplicatePack {
        /// The FQN as its `PackInfo` spells it.
        fqn: &'static str,
    },
    /// The added Pack's id is a built-in's, which only that built-in may
    /// have.
    #[error(
        "Pack `{fqn}` cannot take the Pack id {name}, which the built-in runvil/{name} reserves"
    )]
    ReservedName {
        /// The FQN as its `PackInfo` spells it.
        fqn: &'static str,
        /// The Pack id it would have taken.
        name: PackId,
    },
}

/// The built-ins every registry holds from the start.
///
/// A built-in Pack's behavior and tools live in the Engine. Its Pack
/// object exists only so building, readiness, and the run snapshot treat
/// it like any other Pack, so a built-in must never offer tools, carry a
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

/// The Pack id of the FQN `vendor/name`: its second segment, when both
/// segments are valid one-segment names.
fn pack_id(fqn: &str) -> Option<PackId> {
    let (vendor, name) = fqn.split_once('/')?;
    PackId::parse(vendor).ok()?;
    PackId::parse(name).ok()
}
