//! Per-run params and resource limits: [`RunParams`] and [`RunLimits`].

use std::collections::BTreeMap;
use std::fmt;
use std::num::{NonZero, NonZeroU32, NonZeroUsize};

use runvil_prompt::MAX_TOOL_ITERATIONS;
use runvil_types::cancel::CancelHandle;
use runvil_types::packs::{PackId, Prelude};
use runvil_types::seed::RunSeed;
use runvil_types::timestamp::Timestamp;
use runvil_types::tools::ToolCatalog;
use runvil_vfs::VfsRef;

use crate::lua::RosterEntry;
use crate::model::{ModelInfo, ModelRole};

/// The inputs for one run of a prompt.
///
/// The caller builds the params with `new` and the `with_*` builder
/// methods, among them the run's tool catalog
/// ([`with_tools`](RunParams::with_tools)), Pack roster
/// ([`with_packs`](RunParams::with_packs)), Pack preludes
/// ([`with_preludes`](RunParams::with_preludes)), and role map
/// ([`with_models`](RunParams::with_models)). Building the run fills the
/// five model roles from the role map, and the run's first step checks
/// the params against what the prompt requires. The Engine owns the
/// params for the run. Each `RunParams` belongs to exactly one run.
///
/// The params hold only the Engine's input. The observer, client, tool
/// implementations, broker, and capture belong to the caller. The Engine
/// reports events and issues effects as values, and returns each one to
/// the caller from `step`.
///
/// The caller supplies the run's clock and randomness through `new`.
pub struct RunParams {
    /// The run's seed, as the caller supplied it: the source of the
    /// untrusted-envelope nonce and every other in-run random choice.
    pub(super) seed: RunSeed,
    /// When the run began, as the caller stamped it: rendered as `sys.when`
    /// in every section, the H1 pass included.
    pub(super) started_at: Timestamp,
    /// Where the root task's provenance sequence starts; set by `with_provenance_start`.
    pub(super) provenance_start: u32,
    /// Whether the run reports raw model-round bodies as events; set by `with_report_debug`.
    pub(super) report_debug: bool,
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

impl RunParams {
    /// Params for one run with the given `seed` and `started_at`.
    ///
    /// The new params have a fresh cancel flag, a `ui` snapshot of `None`,
    /// debug reporting off, default [`RunLimits`], and the default
    /// filesystem, which is a fresh memory store at `/`.
    ///
    /// `seed` is the source of the untrusted-envelope nonce. For a live
    /// run, the caller must draw its 32 bytes from a CSPRNG,
    /// because a predictable seed makes a guessable nonce. `started_at` is
    /// the instant every section reads as `sys.when`. The caller records
    /// both values, and a replay passes them back verbatim.
    #[must_use]
    pub fn new(seed: RunSeed, started_at: Timestamp) -> RunParams {
        RunParams {
            seed,
            started_at,
            provenance_start: 0,
            report_debug: false,
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
    /// The default, `false`, turns both events off, so the rounds never
    /// clone a body for them. A caller that wants the pair in the event
    /// stream, such as for a debug capture, passes `true`.
    #[must_use]
    pub fn with_report_debug(mut self, report: bool) -> RunParams {
        self.report_debug = report;
        self
    }

    /// Sets the run's cancel flag, replacing the one that `new` created.
    ///
    /// The caller keeps its own clone of `handle` and cancels the run by
    /// setting it, from any thread. The flag `new` created is the run's
    /// alone, so a run built without this call cannot be canceled.
    ///
    /// The flag is a synchronous [`CancelHandle`] that the Engine polls
    /// between chain steps and from the Lua instruction hook, so running
    /// Lua code sees it too. The next [`step`](super::Run::step) tears down
    /// every chain and lists every effect still outstanding in its
    /// `withdrawn`. The run then reports itself as canceled once every
    /// outstanding effect is answered. Cancellation is a request that later
    /// steps complete: the caller answers each effect it abandons with
    /// [`EffectAnswer::Dropped`](super::EffectAnswer::Dropped).
    ///
    /// A caller that cancels through an awaitable token must bridge the
    /// token to this flag by setting the flag when the token fires.
    #[must_use]
    pub fn with_cancel(mut self, handle: CancelHandle) -> RunParams {
        self.cancel = handle;
        self
    }

    /// Sets the resource limits honored across the run.
    #[must_use]
    pub fn with_limits(mut self, limits: RunLimits) -> RunParams {
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
    pub fn with_ui(mut self, snapshot: serde_json::Value) -> RunParams {
        self.ui = Some(snapshot);
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
    pub fn with_provenance_start(mut self, start: u32) -> RunParams {
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
    /// before the run and extracts output after it through its own clone of
    /// `vfs`.
    #[must_use]
    pub fn with_vfs(mut self, vfs: VfsRef) -> RunParams {
        self.vfs = vfs;
        self
    }

    /// Sets the run's tool catalog: the `ToolInfo` values of every tool of
    /// every Pack the caller can serve this run, declared by the prompt or
    /// not. The catalog lists every tool the run can use: Lua calls one by id,
    /// and a section offers one to its model with `tools.offer`. The caller
    /// performs every call.
    #[must_use]
    pub fn with_tools(mut self, tools: ToolCatalog) -> RunParams {
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
    ) -> RunParams {
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
    /// A prelude fails the run with an `ErrorCode::LuaRuntime` error if it
    /// fails to load, or if it defines a global that another prelude, an
    /// Engine global, or a reserved name already holds.
    /// The failure happens when the first section VM is set up, before the
    /// run issues any effect.
    #[must_use]
    pub fn with_preludes(mut self, preludes: Vec<Prelude>) -> RunParams {
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
    pub fn with_models(mut self, models: BTreeMap<ModelRole, ModelInfo>) -> RunParams {
        self.models = models;
        self
    }
}

impl fmt::Debug for RunParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut state = f.debug_struct("RunParams");
        state
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

/// Resource limits for one run.
///
/// The defaults are built-in constants and are safe to use as they are. A
/// caller changes one by setting its field:
/// `let mut limits = RunLimits::default(); limits.max_concurrency = n;`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct RunLimits {
    /// The default maximum number of tool iterations per `models.loop` call.
    ///
    /// A tool iteration is one model round of such a call; `models.infer`
    /// rounds do not count. A prompt's frontmatter `max_tool_iterations`
    /// field overrides this default for that prompt, and
    /// [`tool_iterations_ceiling`](Self::tool_iterations_ceiling) caps
    /// both.
    pub max_tool_iterations: NonZeroU32,
    /// The most tool iterations any `models.loop`
    /// call may run, whatever the prompt's
    /// `max_tool_iterations` declares; it caps
    /// `max_tool_iterations` too. The default is
    /// 1000, the most a prompt may declare.
    pub tool_iterations_ceiling: NonZeroU32,
    /// The run's concurrency ceiling: the most tasks the scheduler admits
    /// at once across the whole run.
    ///
    /// A chain's `tasks.concurrency` call sets its own limit, clamped to its
    /// enclosing chain's limit, and this ceiling is the root's. Each spawned
    /// task counts against its owner's limit and every ancestor's, so limits
    /// nest: a fanout started inside one arm of another fanout runs within
    /// that arm's remaining share.
    pub max_concurrency: NonZeroUsize,
    /// The most tasks the run holds idle after a
    /// reply. One more reply closes the task idle
    /// longest, its latest reply its result.
    /// Defaults to 8.
    pub max_idle_tasks: NonZeroUsize,
    /// The memory ceiling for each Lua virtual machine, in bytes.
    pub lua_memory_bytes: NonZeroUsize,
    /// The maximum number of `log` events that a prompt's Lua code can
    /// record in each Lua virtual machine.
    ///
    /// The log byte ceiling is derived from this count, so raising it raises
    /// both budgets.
    pub lua_log_events: NonZeroU32,
}

impl Default for RunLimits {
    /// Returns limits with the default values: 24 tool iterations under a
    /// ceiling of 1000, 8 concurrent tasks, 8 idle tasks, 64 MiB of Lua
    /// memory, and 1024 Lua log events.
    fn default() -> RunLimits {
        RunLimits {
            max_tool_iterations: const { NonZero::new(24).unwrap() },
            tool_iterations_ceiling: const { NonZero::new(MAX_TOOL_ITERATIONS).unwrap() },
            max_concurrency: const { NonZero::new(8).unwrap() },
            max_idle_tasks: const { NonZero::new(8).unwrap() },
            lua_memory_bytes: const { NonZero::new(64 * 1024 * 1024).unwrap() },
            lua_log_events: const { NonZero::new(1024).unwrap() },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_limits_pins_all_six_defaults() {
        let defaults = RunLimits::default();
        assert_eq!(defaults.max_tool_iterations.get(), 24);
        assert_eq!(defaults.tool_iterations_ceiling.get(), 1000);
        assert_eq!(
            defaults.tool_iterations_ceiling.get(),
            MAX_TOOL_ITERATIONS,
            "the default ceiling is the most a prompt may declare"
        );
        assert_eq!(defaults.max_concurrency.get(), 8);
        assert_eq!(defaults.max_idle_tasks.get(), 8);
        assert_eq!(defaults.lua_memory_bytes.get(), 64 * 1024 * 1024);
        assert_eq!(defaults.lua_log_events.get(), 1024);
    }

    #[test]
    fn limits_compare_and_hash_by_value() {
        let tighter = RunLimits {
            max_tool_iterations: const { NonZero::new(8).unwrap() },
            ..RunLimits::default()
        };
        assert_ne!(RunLimits::default(), tighter);
        let distinct: std::collections::HashSet<RunLimits> =
            [RunLimits::default(), RunLimits::default(), tighter]
                .into_iter()
                .collect();
        assert_eq!(distinct.len(), 2, "equal limits hash alike");
    }
}
