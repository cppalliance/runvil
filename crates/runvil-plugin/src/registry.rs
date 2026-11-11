//! The Packs a Host builds: [`PackRegistry`], holding the built-ins
//! from the start and each Pack a Plugin inserts with its factory, and the
//! [`PackOrigin`] that says which a Pack is.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use runvil_types::error::{Error, ErrorCode, ErrorFact};
use runvil_types::packs::PackId;
use runvil_types::tools::{ToolInfo, ToolOutput, make_tool_error};
use serde_json::Value;

use crate::{BoxFuture, Pack, PackFactory, PackInfo, ToolContext};

#[cfg(test)]
#[path = "registry-tests.rs"]
mod tests;

/// Every Pack a Host's Plugins insert, by fully qualified name, each
/// with its Pack id and its factory, or the reason it was inserted
/// unavailable.
///
/// The Host owns the registry; a Plugin only inserts its Packs into it,
/// by calling [`insert`](PackRegistry::insert) or
/// [`insert_unavailable`](PackRegistry::insert_unavailable) from its
/// `install` function. The Host makes the registry with
/// [`new`](PackRegistry::new), which holds the built-ins
/// `runvil/subagents` and `runvil/user` from the start, and calls each
/// Plugin it links. Then it hands the registry to `Packs::new`, which
/// reads it through [`into_entries`](PackRegistry::into_entries) and keeps
/// each Pack's factory. Each run calls the factory of each Pack it holds
/// to make that run's instance.
pub struct PackRegistry {
    entries: BTreeMap<&'static str, Registered>,
}

/// One Pack the registry holds.
struct Registered {
    info: PackInfo,
    origin: PackOrigin,
    id: PackId,
    /// The Pack's factory, or the reason it was inserted unavailable.
    factory: Result<Arc<dyn PackFactory>, Error>,
}

/// Where a Pack comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PackOrigin {
    /// Built into every registry, with no tools of its own:
    /// `runvil/subagents`, whose behavior will live in the Engine and which
    /// has none yet, and `runvil/user`, which marks a prompt that needs a
    /// human.
    Builtin,
    /// Inserted by a Plugin with [`PackRegistry::insert`] or
    /// [`PackRegistry::insert_unavailable`]: by a linked Plugin's
    /// `install`, or by the Harness for a Plugin's C entry point, linked
    /// or from a dynamic library.
    Plugin,
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
                    factory: Ok(Arc::new(builtin)),
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

    /// Adds the Pack `info` names as a Plugin's Pack, made by `factory`.
    ///
    /// The Pack id is the FQN's second segment, so `runvil/web` is `web`,
    /// and the registry may hold Packs of different vendors with the same
    /// Pack id, such as `runvil/web` and `acme/web`.
    ///
    /// # Errors
    /// Returns an [`InvalidValue`](ErrorCode::InvalidValue) error that names
    /// the FQN, so the Host's author sees the refusal at startup, with the
    /// reason:
    ///
    /// - `invalid_fqn` when the FQN is not a `vendor/name` pair of valid
    ///   one-segment names;
    /// - `duplicate_pack` when the registry holds the FQN already, as it
    ///   holds each built-in's from the start;
    /// - `reserved_pack_id` when the Pack id is
    ///   [reserved](PackId::is_reserved), such as `adhoc`;
    /// - `reserved_name` when the Pack id is a built-in's.
    ///
    /// A Pack whose factory refuses a run is not refused here; that run
    /// holds the Pack as unavailable instead.
    pub fn insert(
        &mut self,
        info: PackInfo,
        factory: impl PackFactory + 'static,
    ) -> Result<(), Error> {
        self.add(info, Ok(Arc::new(factory)))
    }

    /// Adds the Pack `info` names as a Plugin's Pack that is unavailable for
    /// `reason`, such as bad config or a missing Host-wide service.
    /// `Host::parse` refuses a prompt that declares it, naming `reason`.
    ///
    /// # Errors
    /// Refuses what [`insert`](PackRegistry::insert) refuses, with the
    /// same errors.
    pub fn insert_unavailable(&mut self, info: PackInfo, reason: Error) -> Result<(), Error> {
        self.add(info, Err(reason))
    }

    /// Adds `info` as a Plugin's Pack with `factory`, or refuses it as
    /// [`insert`](PackRegistry::insert) documents.
    fn add(
        &mut self,
        info: PackInfo,
        factory: Result<Arc<dyn PackFactory>, Error>,
    ) -> Result<(), Error> {
        let fqn = info.fqn;
        let id = pack_id(fqn).ok_or_else(|| {
            make_refusal_error(
                "invalid_fqn",
                format!("Pack fully qualified name `{fqn}` is not a vendor/name pair"),
                fqn,
            )
        })?;
        if self.entries.contains_key(fqn) {
            return Err(make_refusal_error(
                "duplicate_pack",
                format!("Pack `{fqn}` is already in the registry"),
                fqn,
            ));
        }
        if id.is_reserved() {
            return Err(make_refusal_error(
                "reserved_pack_id",
                format!("Pack `{fqn}` cannot take the Pack id {id}, which the Engine reserves"),
                fqn,
            ));
        }
        if self.reserves(&id) {
            return Err(make_refusal_error(
                "reserved_name",
                format!(
                    "Pack `{fqn}` cannot take the Pack id {id}, which the built-in runvil/{id} \
                     reserves"
                ),
                fqn,
            ));
        }
        self.entries.insert(
            info.fqn,
            Registered {
                info,
                origin: PackOrigin::Plugin,
                id,
                factory,
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
    /// id and its factory, or the reason it was inserted unavailable, in
    /// FQN order: what a Host builds its Packs from.
    pub fn into_entries(
        self,
    ) -> impl Iterator<Item = (PackInfo, PackId, Result<Arc<dyn PackFactory>, Error>)> {
        self.entries
            .into_values()
            .map(|registered| (registered.info, registered.id, registered.factory))
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
    /// Shows each Pack's FQN, origin, and Pack id.
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

/// The refusal of an insert: an `InvalidValue` that names the FQN, with the
/// broken rule as its reason. Its prose is its message, which names the Pack
/// and the rule.
fn make_refusal_error(reason: &'static str, message: String, fqn: &str) -> Error {
    let mut error = Error::new(ErrorCode::InvalidValue, message.clone(), message);
    error.facts.insert(ErrorFact::Name, Value::from(fqn));
    error.facts.insert(ErrorFact::Reason, Value::from(reason));
    error
}

/// The Packs every registry holds from the start: `runvil/subagents`, and
/// `runvil/user`, which marks a prompt that needs a human. Neither offers
/// tools or has a prelude. A run holds `runvil/user` only when its setup
/// has a user.
///
/// A built-in Pack's behavior and tools live in the Engine. Its instance
/// exists only so making, readiness, and the run snapshot treat it like
/// any other Pack, so a built-in must never offer tools or carry a
/// prelude, and its factory never refuses a run. A built-in can still be
/// unavailable to a run: `runvil/user` is unavailable in a run without a
/// user, which the Harness decides from the run's setup instead of making
/// the Pack. Its `call` is unreachable, because the Engine never issues a
/// call under a built-in's id: a built-in lists no tools, so no section
/// offers one of its tools and no render of it adds one. A built-in never
/// takes a [reserved](PackId::is_reserved) Pack id: the Harness puts every
/// `runvil` Pack no `uses:` entry replaces into every run's roster, so a
/// reserved one would refuse every run.
const BUILTIN: [PackInfo; 2] = [
    PackInfo::new("runvil/subagents").description(
        "Reserves the subagents name for the Engine's subagent tools, which are not built yet",
    ),
    PackInfo::new("runvil/user")
        .description("Marks a prompt that needs a human; only a run with a user holds it"),
];

/// The factory of every built-in: makes each run's [`Builtin`] instance;
/// see [`BUILTIN`].
#[expect(
    clippy::unnecessary_wraps,
    reason = "the PackFactory signature fixes the return type"
)]
fn builtin(_id: &PackId) -> Result<Arc<dyn Pack>, Error> {
    Ok(Arc::new(Builtin))
}

/// The instance of a built-in Pack, which offers no tools; see
/// [`BUILTIN`].
struct Builtin;

impl Pack for Builtin {
    fn tools(&self) -> Vec<ToolInfo> {
        Vec::new()
    }

    fn call<'a>(
        &'a self,
        cx: ToolContext<'a>,
        _args: Value,
    ) -> BoxFuture<'a, Result<ToolOutput, Error>> {
        let message = format!(
            "{} belongs to a built-in Pack, whose calls the Engine answers",
            cx.tool()
        );
        Box::pin(async move { Err(make_tool_error(ErrorCode::ToolOther, message)) })
    }
}

/// The Pack id of the FQN `vendor/name`: its second segment, when both
/// segments are valid one-segment names.
fn pack_id(fqn: &str) -> Option<PackId> {
    let (vendor, name) = fqn.split_once('/')?;
    PackId::parse(vendor).ok()?;
    PackId::parse(name).ok()
}
