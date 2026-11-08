//! The [`Pack`] trait, the [`PackFactory`] that makes a Pack's instances,
//! and the [`PackInfo`] a Plugin crate exports.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use runvil_types::error::Error;
use runvil_types::packs::PackId;
use runvil_types::tools::{ToolInfo, ToolOutput};

use crate::{HostServices, RenderContext, ToolContext};

/// A boxed future that may borrow for `'a`, the same type as
/// `futures::future::BoxFuture`.
///
/// [`Pack::call`], [`Pack::ready`], and [`Pack::render`] return one, so
/// the Harness can hold Packs as `dyn Pack`. An implementation wraps its
/// body in `Box::pin(async move { ... })`.
pub type PackFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// One run's instance of a Pack: the tools it offers and the calls it
/// performs.
///
/// Each run makes its own instance of each Pack it holds, through the
/// Pack's [`PackFactory`]. The run's effects share it behind an `Arc`, so
/// an implementation is `Send + Sync`. Cleanup, such as closing what the
/// instance opened for its run, goes in the implementation's `Drop`, which
/// runs when the run ends. There is no shutdown hook.
pub trait Pack: Send + Sync {
    /// Returns the tools the Pack offers now.
    ///
    /// The Harness reads the list once, when the run takes its snapshot
    /// after it awaits [`ready`](Pack::ready), so a Pack whose tools
    /// change returns its current list, and a Pack still coming up
    /// returns what it has so far. Each tool's id sits under the Pack id
    /// [`new_pack`](PackFactory::new_pack) received, as in `web/fetch`.
    fn tools(&self) -> Vec<ToolInfo>;

    /// Resolves once the Pack's tool list is complete, or with the
    /// reason it can't serve now.
    ///
    /// The Harness awaits the `ready` of each of a run's instances
    /// together before the run reads [`tools`](Pack::tools). A Pack whose
    /// `ready` fails is unavailable to that run, with the error's message
    /// as the reason. The default resolves `Ok(())` at once, for a Pack
    /// whose tools are known when [`new_pack`](PackFactory::new_pack)
    /// returns.
    ///
    /// # Invariants
    ///
    /// - `ready` resolves `Ok` once [`tools`](Pack::tools) is complete,
    ///   or `Err` with a reason a model can read when the Pack can't serve
    ///   now.
    /// - `ready` resolves within a bound the Pack owns. The Pack's own
    ///   task enforces that bound, and `ready` uses no runtime timer,
    ///   because it may be polled outside any particular runtime.
    /// - `ready` is cancellation-safe: a cancel drops it unresolved. After
    ///   its first resolution it answers at once unless the Pack's state
    ///   has changed.
    ///
    /// # Errors
    /// Returns an [`Error`], whose prose is safe to show the model, when the
    /// Pack can't offer its tools now, such as when the backend it connects
    /// to refuses it or is down.
    fn ready(&self) -> PackFuture<'_, Result<(), Error>> {
        Box::pin(async { Ok(()) })
    }

    /// Performs one tool call.
    ///
    /// `cx` names the called tool and lends the call's filesystem access
    /// and its origin. What the call needs from the run's own services,
    /// the instance took at [`new_pack`](PackFactory::new_pack). `args` is
    /// the JSON the model or the script passed.
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
    ///   the task that made the call ends early: it is canceled, its owner
    ///   ends, or an ancestor is aborted. Only a stop spares a call whose
    ///   tool's info sets `survives_stop`. After a stop or a task's
    ///   early end, the run goes on.
    /// - `call` must not wait for another call of the same run to finish.
    ///   The Harness runs a bounded number of a run's tool calls at once
    ///   and holds the rest, so a call that waits on one held behind it can
    ///   stall the run until a stop or a cancel.
    ///
    /// # Errors
    /// Returns an [`Error`] with a tool code, built with
    /// [`make_tool_error`](crate::make_tool_error) so its prose is safe to show the
    /// model, when the call rejects the arguments, its backend refuses, its
    /// transport fails, or the run is canceled.
    fn call<'a>(
        &'a self,
        cx: ToolContext<'a>,
        args: serde_json::Value,
    ) -> PackFuture<'a, Result<ToolOutput, Error>>;

    /// Returns the tools the model sees, given the tools a section offered
    /// from this Pack in one set.
    ///
    /// A section offers tools in two sets: statically, into the tool list
    /// every round of the section sends, or dynamically. `cx` names the
    /// section entry and the model of its first round, and `offered` holds
    /// the tools the section offered from this Pack in one set. A render
    /// may reshape, rename, or drop offered tools, and may add model-only
    /// tools under its own Pack id. Every catalog tool it returns is one
    /// the section offered in that set, and keeps its catalog flags, such
    /// as `survives_stop`, whatever the render sets; a model-only tool
    /// takes the flags the render gives it. It receives the same inputs
    /// for either set, and a set the Pack was not offered into is not
    /// rendered. The default returns `offered` unchanged.
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

/// Makes each run's instance of one installed Pack.
///
/// A Plugin builds it at `install`, from its config and the Host-wide
/// services, and keeps in it what every run's instance shares, such as a
/// connection pool.
pub trait PackFactory: Send + Sync {
    /// Makes this run's instance named `id`, taking what it needs from
    /// the run's own services in `run`.
    ///
    /// It runs inside the run's future, so it must not block, do I/O, or
    /// panic. The Harness calls it once for each run that holds the Pack,
    /// and never for a run that does not.
    ///
    /// # Errors
    /// Returns an [`Error`], whose prose is safe to show the model, when
    /// the factory cannot make the instance, such as when `run` lacks a
    /// service it needs. The Pack is then unavailable to this run, with
    /// the error's message as the reason.
    fn new_pack(&self, id: &PackId, run: &HostServices) -> Result<Arc<dyn Pack>, Error>;
}

/// A plain function or closure with `new_pack`'s parameters and return
/// type is a factory, so a Pack that shares nothing needs no type of its
/// own.
impl<Func> PackFactory for Func
where
    Func: Fn(&PackId, &HostServices) -> Result<Arc<dyn Pack>, Error> + Send + Sync,
{
    fn new_pack(&self, id: &PackId, run: &HostServices) -> Result<Arc<dyn Pack>, Error> {
        self(id, run)
    }
}

/// A Pack described as data, which a Plugin inserts into the Host's
/// [`PackRegistry`](crate::PackRegistry) beside its [`PackFactory`].
///
/// It is fixed data the Host reads before anything is made: the Pack's
/// FQN, an optional prelude, and a one-line description. A Plugin crate
/// keeps it in a private constant, builds it with [`PackInfo::new`],
/// adding a [`PackInfo::description`], and a [`PackInfo::prelude`] only
/// where it has one, and inserts it from its `install` function, or
/// inserts it unavailable when its setup fails:
///
/// ```text
/// const GREETER: PackInfo = PackInfo::new("acme/greeter")
///     .description("Greet the person running the prompt")
///     .prelude(PRELUDE);
///
/// pub fn install(
///     packs: &mut PackRegistry,
///     config: Value,
///     services: &HostServices,
/// ) -> Result<(), Error> {
///     match greeting(&config["greeter"], services) {
///         Ok(greeting) => packs.insert(GREETER, Greetings { greeting }),
///         Err(reason) => packs.insert_unavailable(GREETER, reason),
///     }
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
    /// One line saying what the Pack offers, for a Host listing the
    /// Packs its registry holds; empty unless the Plugin crate sets it.
    pub description: &'static str,
}

impl PackInfo {
    /// Names the Pack `fqn`, with an empty description and no prelude;
    /// the builders add the rest.
    #[must_use]
    pub const fn new(fqn: &'static str) -> PackInfo {
        PackInfo {
            fqn,
            prelude: None,
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
}

impl fmt::Debug for PackInfo {
    /// Shows the FQN, the description, and whether a prelude exists.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PackInfo")
            .field("fqn", &self.fqn)
            .field("description", &self.description)
            .field("prelude", &self.prelude.is_some())
            .finish_non_exhaustive()
    }
}
