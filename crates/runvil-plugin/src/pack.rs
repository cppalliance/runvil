//! The [`Pack`] trait and the [`PackInfo`] a Plugin crate exports.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use runvil_types::packs::PackId;
use runvil_types::tools::{ToolError, ToolInfo, ToolOutput};

use crate::{HostServices, RenderContext, ServiceId, ToolContext};

/// A boxed future that may borrow for `'a`, the same type as
/// `futures::future::BoxFuture`.
///
/// [`Pack::call`], [`Pack::ready`], and [`Pack::render`] return one, so
/// the Harness can hold Packs as `dyn Pack`. An implementation wraps its
/// body in `Box::pin(async move { ... })`.
pub type PackFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// One Pack object: the tools it offers and the calls it performs.
///
/// The Host builds one object per registered Pack, through
/// [`PackInfo::construct`], and every run shares it, so an implementation
/// is `Send + Sync`. Cleanup, such as stopping a child process, goes in
/// the implementation's `Drop`, which runs when the Host drops the context
/// holding the Pack. There is no shutdown hook.
pub trait Pack: Send + Sync {
    /// Returns the tools the Pack offers now.
    ///
    /// The Harness reads the list once when each run starts, after it
    /// awaits [`ready`](Pack::ready), so a Pack whose tools change
    /// returns its current list, and a Pack still coming up returns what
    /// it has so far. Each tool's id sits under the Pack id `construct`
    /// received, as in `web/fetch`.
    fn tools(&self) -> Vec<ToolInfo>;

    /// Resolves once the Pack's tool list is complete, or with the
    /// reason it can't serve now.
    ///
    /// The Harness awaits every Pack's `ready` together before each run
    /// reads [`tools`](Pack::tools). A Pack whose `ready` fails is
    /// unavailable to that run, with the error's message as the reason.
    /// The default resolves `Ok(())` at once, for a Pack whose tools are
    /// known when `construct` returns.
    ///
    /// # Invariants
    ///
    /// - `ready` resolves `Ok` once [`tools`](Pack::tools) is complete,
    ///   or `Err` with a reason a model can read when the Pack can't serve
    ///   now. A Pack that recovers answers `Ok` to a later call.
    /// - `ready` resolves within a bound the Pack owns. The Pack's own
    ///   task enforces that bound, and `ready` uses no runtime timer,
    ///   because it may be polled outside any particular runtime.
    /// - `ready` is cancellation-safe: a cancel drops it unresolved. After
    ///   its first resolution it answers at once unless the Pack's state
    ///   has changed.
    ///
    /// # Errors
    /// Returns a [`ToolError`], whose message is safe to show the model,
    /// when the Pack can't offer its tools now, such as when the backend
    /// it connects to refuses it or is down.
    fn ready(&self) -> PackFuture<'_, Result<(), ToolError>> {
        Box::pin(async { Ok(()) })
    }

    /// Performs one tool call.
    ///
    /// `cx` names the called tool and lends the call's filesystem access,
    /// its origin, and the run's services. `args` is the JSON the model or
    /// the script passed.
    ///
    /// # Invariants
    ///
    /// - `call` must not block while polled. The Harness polls every
    ///   effect of a run on the same task, so blocking or CPU-heavy work
    ///   goes to the Host's runtime, without the access. Operations
    ///   through [`ToolContext::access`] run inside the call; on real
    ///   directories they block the poll briefly, as `store.*` does.
    /// - `call` must not panic. If it does, the Harness answers its effect
    ///   `Dropped` and logs the panic.
    /// - `call` marks the trust of every output correctly. Output that
    ///   embeds data an attacker can influence is
    ///   [`ToolOutput::untrusted`].
    /// - `call` is cancellation-aware. The Harness drops the returned
    ///   future on a stop, when the run ends (a cancel included), and when
    ///   the task that made the call ends early: it is cancelled, its owner
    ///   ends, or an ancestor is aborted. Only a stop spares a call whose
    ///   tool's info sets `survives_stop`. After a stop or a task's
    ///   early end, the run goes on.
    /// - `call` must not wait for another call of the same run to finish.
    ///   The Harness runs a bounded number of a run's tool calls at once
    ///   and holds the rest, so a call that waits on one held behind it can
    ///   stall the run until a stop or a cancel.
    ///
    /// # Errors
    /// Returns a [`ToolError`], whose message is safe to show the model,
    /// when the call rejects the arguments, its backend refuses, its
    /// transport fails, or the run is cancelled.
    fn call<'a>(
        &'a self,
        cx: ToolContext<'a>,
        args: serde_json::Value,
    ) -> PackFuture<'a, Result<ToolOutput, ToolError>>;

    /// Returns the tools the model sees, given the tools a section offered
    /// from this Pack in one set.
    ///
    /// A section offers tools in two sets: statically, into the tool list
    /// every round of the section sends, or dynamically. `cx` names the
    /// section entry and the model of its first round, and `offered` holds
    /// the tools the section offered from this Pack in one set. A render
    /// may reshape, rename, or drop offered tools, and may add model-only
    /// tools under its own Pack id. Every catalog tool it returns is one
    /// the section offered in that set. It receives the same inputs for
    /// either set, and a set the Pack was not offered into is not rendered.
    /// The default returns `offered` unchanged.
    ///
    /// # Invariants
    ///
    /// - `render` runs in the effect loop with [`call`](Pack::call)'s
    ///   rules: it must not block while polled, it must not panic, and it
    ///   is cancellation-aware.
    fn render<'a>(
        &'a self,
        _cx: RenderContext<'a>,
        offered: Vec<ToolInfo>,
    ) -> PackFuture<'a, Vec<ToolInfo>> {
        Box::pin(async move { offered })
    }
}

/// A Pack described as data, which a Plugin inserts into the Host's
/// [`PackRegistry`](crate::PackRegistry).
///
/// It is fixed data the Host reads before anything is built. A Plugin
/// crate keeps it in a private constant, builds it with
/// [`PackInfo::new`], adding a [`PackInfo::description`], and
/// [`PackInfo::prelude`] and [`PackInfo::needs`] only where it has them,
/// and inserts it from its `install` function:
///
/// ```text
/// const GREETER: PackInfo = PackInfo::new("acme/greeter", construct)
///     .description("Greet the person running the prompt")
///     .prelude(PRELUDE)
///     .needs(NEEDS);
///
/// pub fn install(packs: &mut PackRegistry, config: Value) -> Result<(), InstallError> {
///     packs.insert(GREETER, config["greeter"].clone())
/// }
/// ```
#[derive(Clone, Copy)]
#[non_exhaustive]
pub struct PackInfo {
    /// The Pack's fully qualified name (FQN): `vendor/name` with two
    /// segments, such as `runvil/web`. The second segment is the Pack id,
    /// which prefixes every tool the Pack offers.
    pub fqn: &'static str,
    /// Lua run once for each prompt that declares the Pack, as a chunk
    /// whose `...` is the Pack id; `None` when the Pack has no prelude.
    pub prelude: Option<&'static str>,
    /// The per-run services the Pack's calls read. A run without every
    /// one of them cannot use the Pack.
    pub needs: &'static [ServiceId],
    /// Builds the one Pack object every run shares. The Host calls it
    /// once, when it builds its Packs, with the Pack id, the config the
    /// Plugin inserted the Pack with, and the Host-wide services.
    ///
    /// A failure, such as a missing Host-wide service, leaves the Pack
    /// unavailable, and its message appears in the refusal notice of each
    /// run whose prompt declares the Pack.
    #[expect(
        clippy::type_complexity,
        reason = "spelled out so a Plugin author reads the signature construct implements in one place"
    )]
    pub construct: fn(
        name: &PackId,
        config: serde_json::Value,
        services: &HostServices,
    ) -> Result<Arc<dyn Pack>, ToolError>,
    /// One line saying what the Pack offers, for a Host listing the
    /// Packs its registry holds; empty unless the Plugin crate sets it.
    pub description: &'static str,
}

impl PackInfo {
    /// Builds the `PackInfo` for the FQN `fqn`, with an empty
    /// description, no prelude, and no needs.
    #[expect(
        clippy::type_complexity,
        reason = "spelled out so a Plugin author reads the signature construct implements in one place"
    )]
    #[must_use]
    pub const fn new(
        fqn: &'static str,
        construct: fn(
            name: &PackId,
            config: serde_json::Value,
            services: &HostServices,
        ) -> Result<Arc<dyn Pack>, ToolError>,
    ) -> PackInfo {
        PackInfo {
            fqn,
            prelude: None,
            needs: &[],
            construct,
            description: "",
        }
    }

    /// Sets the one line saying what the Pack offers.
    #[must_use]
    pub const fn description(self, description: &'static str) -> PackInfo {
        PackInfo {
            description,
            ..self
        }
    }

    /// Sets the Lua run once for each prompt that declares the Pack.
    #[must_use]
    pub const fn prelude(self, prelude: &'static str) -> PackInfo {
        PackInfo {
            prelude: Some(prelude),
            ..self
        }
    }

    /// Sets the per-run services the Pack's calls read.
    #[must_use]
    pub const fn needs(self, needs: &'static [ServiceId]) -> PackInfo {
        PackInfo { needs, ..self }
    }
}

impl fmt::Debug for PackInfo {
    /// Shows the FQN, the description, whether a prelude exists, and the
    /// needs.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PackInfo")
            .field("fqn", &self.fqn)
            .field("description", &self.description)
            .field("prelude", &self.prelude.is_some())
            .field("needs", &self.needs)
            .finish_non_exhaustive()
    }
}
