//! Per-run context and resource limits: [`RunContext`] and [`RunLimits`].

use std::collections::BTreeMap;
use std::fmt;

#[path = "config-limits.rs"]
mod limits;

use runvil_types::cancel::CancelHandle;
use runvil_types::emitter::DebugMode;
use runvil_types::packs::{PackId, Prelude};
use runvil_types::replay::Flags;
use runvil_types::seed::{RunSeed, RunUid};
use runvil_types::timestamp::Timestamp;
use runvil_types::tools::ToolCatalog;
use runvil_vfs::VfsRef;

pub use limits::RunLimits;

use crate::lua::RosterEntry;
use crate::model::{ModelInfo, ModelRole};

/// The inputs for one run of a prompt.
///
/// The caller builds a context with `new` and the `with_*` builder
/// methods, among them the run's tool catalog
/// ([`with_tools`](RunContext::with_tools)), Pack roster
/// ([`with_packs`](RunContext::with_packs)), Pack preludes
/// ([`with_preludes`](RunContext::with_preludes)), and role map
/// ([`with_models`](RunContext::with_models)). Building the run fills the
/// five model roles from the role map, and the run's first step checks
/// the context against what the prompt requires. The Engine owns the
/// context for the run. Each context belongs to exactly one run.
///
/// The context holds only the Engine's input. The observer, client, tool
/// implementations, broker, and capture belong to the caller. The Engine
/// reports events and issues effects as values, and returns each one to
/// the caller from `step`.
///
/// The caller supplies the run's clock and randomness through `new`.
pub struct RunContext {
    /// The caller's run label, moved into `RunState::execution` at start and
    /// read nowhere else.
    pub(super) name: String,
    /// The run's seed, as the caller supplied it: the source of the run id,
    /// the untrusted-envelope nonce, and every other in-run random choice.
    pub(super) seed: RunSeed,
    /// The behavior flags the run records; empty by default.
    flags: Flags,
    /// When the run began, as the caller stamped it: rendered as `sys.when`
    /// in every section, the H1 pass included.
    pub(super) started_at: Timestamp,
    /// Where the root task's provenance sequence starts; set by `with_provenance_start`.
    pub(super) provenance_start: u32,
    /// Whether the run reports raw model-round bodies as events; set by `with_report_debug`.
    pub(super) report_debug: DebugMode,
    /// The run's cancel flag, minted by `new` and replaced by `with_cancel`.
    pub(super) cancel: CancelHandle,
    /// The run's resource limits; set by `with_limits`.
    pub(super) limits: RunLimits,
    /// The application-state snapshot the `ui()` global serves; set by `with_ui`.
    pub(super) ui: Option<serde_json::Value>,
    /// The run's whole filesystem; set by `with_vfs`.
    pub(super) vfs: VfsRef,
    /// The caller's role map, which fills the run's five roles; set by
    /// `with_models`.
    pub(super) models: BTreeMap<ModelRole, ModelInfo>,
    /// The run's tool catalog; set by `with_tools`.
    pub(super) tools: ToolCatalog,
    /// Each roster Pack by name, with its vendor when it can serve and the
    /// reason when it can't; set by `with_packs`, and `None` without a
    /// roster.
    pub(super) packs: Option<BTreeMap<PackId, RosterEntry>>,
    /// The run's Pack preludes, in declaration order; set by `with_preludes`.
    pub(super) preludes: Vec<Prelude>,
}

impl RunContext {
    /// Builds a context for a run named `name`, with the given `seed` and
    /// `started_at`.
    ///
    /// The new context has a fresh cancel flag, a `ui` snapshot of `None`,
    /// debug reporting set to `DebugMode::Off`, default [`RunLimits`], empty
    /// [`Flags`], and the default filesystem, which is a fresh memory store
    /// at `/`.
    ///
    /// `seed` is the source of the run id and the untrusted-envelope nonce.
    /// For a live run, the caller must draw its 32 bytes from a CSPRNG,
    /// because a predictable seed makes a guessable nonce. `started_at` is
    /// the instant every section reads as `sys.when`. The caller records
    /// both values, and a replay passes them back verbatim.
    #[must_use]
    pub fn new(name: impl Into<String>, seed: RunSeed, started_at: Timestamp) -> RunContext {
        RunContext {
            name: name.into(),
            seed,
            flags: Flags::EMPTY,
            started_at,
            provenance_start: 0,
            report_debug: DebugMode::Off,
            cancel: CancelHandle::new(),
            limits: RunLimits::default(),
            ui: None,
            vfs: VfsRef::default(),
            models: BTreeMap::new(),
            tools: ToolCatalog::default(),
            packs: None,
            preludes: Vec::new(),
        }
    }

    /// Sets whether the run reports each model round's raw request and
    /// response bodies as `Request` and `Response` events.
    ///
    /// The default, [`DebugMode::Off`], turns both events off. A caller that
    /// wants the pair in the event stream, such as for a debug capture,
    /// passes [`DebugMode::On`].
    #[must_use]
    pub fn with_report_debug(mut self, mode: DebugMode) -> RunContext {
        self.report_debug = mode;
        self
    }

    /// Sets the run's cancel flag, replacing the one that `new` created.
    ///
    /// The flag is a synchronous [`CancelHandle`] that the Engine polls
    /// between chain steps and from the Lua instruction hook. A caller that
    /// cancels through an awaitable token must bridge the token to this
    /// flag by setting the flag when the token fires.
    #[must_use]
    pub fn with_cancel(mut self, handle: CancelHandle) -> RunContext {
        self.cancel = handle;
        self
    }

    /// Sets the resource limits honored across the run.
    #[must_use]
    pub fn with_limits(mut self, limits: RunLimits) -> RunContext {
        self.limits = limits;
        self
    }

    /// Sets the snapshot of application state that prompts read through the
    /// `ui()` global.
    ///
    /// With a snapshot set, every section VM gets a `ui()` global that
    /// returns it. The caller takes the snapshot at run start, so a change
    /// to application state takes effect on the next run. A snapshot also
    /// makes `models.get` resolve a label outside the five roles as a raw
    /// gateway model id. So a prompt can run
    /// `models.loop(models.get(ui().selected_model))` and run on the model the
    /// user picked. The default is `None`, which leaves the `ui` global
    /// absent and limits `models.get` to the five roles.
    #[must_use]
    pub fn with_ui(mut self, snapshot: serde_json::Value) -> RunContext {
        self.ui = Some(snapshot);
        self
    }

    /// Sets the behavior flags the run records.
    ///
    /// A live run keeps the default, the empty set. A replay passes back the
    /// set the original run recorded.
    #[must_use]
    pub fn with_flags(mut self, flags: Flags) -> RunContext {
        self.flags = flags;
        self
    }

    /// Sets where the root task's provenance sequence starts.
    ///
    /// The default, 0, fits a run recorded on its own. When the caller
    /// records the prompt's parse events ahead of the run in one stream, it
    /// passes their count. `Prompt::parse` stamps those events under task
    /// `0`, counting from zero, so this start moves the run's root counter
    /// past them and every `(task, seq)` in the stream stays unique.
    /// Spawned tasks count from zero.
    #[must_use]
    pub fn with_provenance_start(mut self, start: u32) -> RunContext {
        self.provenance_start = start;
        self
    }

    /// Sets the run's VFS handle, which is the run's whole filesystem.
    ///
    /// The filesystem holds the real directories and the declared store
    /// that every section's `store` table operates on. The default is a
    /// fresh memory store at `/`.
    ///
    /// Every tool call effect carries an access to this same filesystem, so
    /// the tools and the run share one set of files. The caller seeds files
    /// before the run and extracts output after it through the context's
    /// handle, returned by [`vfs`](RunContext::vfs).
    #[must_use]
    pub fn with_vfs(mut self, vfs: VfsRef) -> RunContext {
        self.vfs = vfs;
        self
    }

    /// Sets the run's tool catalog: the `ToolInfo` values of every tool of
    /// every Pack the caller can serve this run, declared by the prompt or
    /// not. The catalog lists every tool the run can use: Lua calls one by id,
    /// and a section offers one to its model with `tools.offer`. The caller
    /// performs every call. Read it back with
    /// [`tools`](RunContext::tools).
    #[must_use]
    pub fn with_tools(mut self, tools: ToolCatalog) -> RunContext {
        self.tools = tools;
        self
    }

    /// Sets the run's roster: every installed Pack that can serve the run,
    /// declared by the prompt or not, with its vendor, and every one that
    /// can't, with the reason. A Pack listed twice keeps its last entry.
    ///
    /// Each usable Pack gets an object in the run's Lua, with or without
    /// tools. An unavailable one gets none, and offering it raises its
    /// reason. Without a roster, the run's Lua sees the declared Packs and
    /// the Packs with a tool in the catalog.
    ///
    /// The run's first step checks each Pack the prompt declares against
    /// the roster: one the roster lacks is missing, and one it lists as
    /// unavailable fails the run with its reason. A run given no roster
    /// checks no declared Pack.
    ///
    /// The caller reports each usable Pack's vendor, as it picked the
    /// Pack. Every tool-call and tool-result event for a catalog tool of
    /// that Pack records the vendor, and a run given no roster records
    /// none.
    #[must_use]
    pub fn with_packs(
        mut self,
        roster: impl IntoIterator<Item = (PackId, RosterEntry)>,
    ) -> RunContext {
        self.packs = Some(roster.into_iter().collect());
        self
    }

    /// Sets the Pack preludes that every section VM of the run installs.
    ///
    /// A prelude is Lua source that a Pack the prompt declares
    /// contributed. The caller passes them in the order the prompt
    /// declares the Packs. Every section VM of the run installs each
    /// prelude after the other Engine globals and before `require` and any
    /// block, so a prelude cannot call `require` while it installs, while
    /// every block, and every library a block loads with `require`, can
    /// call what the preludes define.
    ///
    /// A prelude fails the run as
    /// [`RunErrorKind::Lua`](super::RunErrorKind::Lua) if it fails to load,
    /// or if it defines a global that another prelude, an Engine global, or
    /// a reserved name already holds.
    /// The failure happens when the first section VM is set up, before the
    /// run issues any effect.
    #[must_use]
    pub fn with_preludes(mut self, preludes: Vec<Prelude>) -> RunContext {
        self.preludes = preludes;
        self
    }

    /// Sets the caller's role map, which fills the run's five model roles
    /// (`default`, `strong`, `small`, `fast` and `creative`) when the run
    /// is built.
    ///
    /// A role's model is its own entry in `models`, else the `default`
    /// entry, so one entry for `default` fills all five roles. Every prompt
    /// gets all five roles, whether or not it declares them. A role's
    /// declared `thinking` requirement turns thinking on or off for its
    /// rounds.
    ///
    /// The run's first step checks each declared role's `min_context` and
    /// `thinking` requirements against the info of the model that fills
    /// it, and fails the run naming each one that falls short, with the
    /// required and actual values.
    ///
    /// An empty `models`, the default, means the caller has no model: the
    /// roles stay empty, nothing is checked, and a model round at run time
    /// fails. A non-empty `models` with no `default` entry leaves the roles
    /// empty and fails the run's first step.
    #[must_use]
    pub fn with_models(mut self, models: BTreeMap<ModelRole, ModelInfo>) -> RunContext {
        self.models = models;
        self
    }

    /// Returns the run's VFS handle, set with [`with_vfs`](RunContext::with_vfs).
    #[must_use]
    pub fn vfs(&self) -> &VfsRef {
        &self.vfs
    }

    /// Returns the run's tool catalog, set with
    /// [`with_tools`](RunContext::with_tools). It is empty by default.
    #[must_use]
    pub fn tools(&self) -> &ToolCatalog {
        &self.tools
    }

    /// Returns the run's seed, as the caller supplied it.
    #[must_use]
    pub fn seed(&self) -> RunSeed {
        self.seed
    }

    /// Returns the run's id, derived from its seed.
    ///
    /// The id is one-way, so it does not reveal the seed or the nonce. It
    /// appears in a debug rendering only: events and reports carry the
    /// run's `name`. A replay under the recorded seed has the same id.
    #[must_use]
    pub(crate) fn uid(&self) -> RunUid {
        RunUid::from_seed(&self.seed)
    }

    /// Returns the behavior flags the run records. Set them with
    /// [`with_flags`](RunContext::with_flags).
    #[must_use]
    pub fn flags(&self) -> Flags {
        self.flags
    }

    /// Returns when the run began, as the caller stamped it.
    #[must_use]
    pub fn started_at(&self) -> Timestamp {
        self.started_at
    }
}

impl fmt::Debug for RunContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut state = f.debug_struct("RunContext");
        state
            .field("name", &self.name)
            .field("uid", &self.uid())
            .field("flags", &self.flags)
            .field("started_at", &self.started_at)
            .field("provenance_start", &self.provenance_start)
            .field("report_debug", &self.report_debug)
            .field("cancel", &self.cancel)
            .field("limits", &self.limits)
            .field("ui", &self.ui)
            .field("vfs", &self.vfs)
            .field("models", &self.models)
            .field("tools", &self.tools)
            .field("packs", &self.packs)
            .field(
                "preludes",
                &self.preludes.iter().map(Prelude::pack).collect::<Vec<_>>(),
            )
            .finish_non_exhaustive()
    }
}
