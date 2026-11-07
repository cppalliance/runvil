//! The execute subtree's ambient run state.
//!
//! [`ChainState`] is built once in [`run`](super::run), as the root
//! chain's state, and travels through the execute subtree as parameter one
//! (`ctx: &ChainState`). It holds the run's one [`SharedState`] and the
//! fields a chain's fork replaces. The invariant: a new run-scoped concern
//! becomes a field here, never a new parameter: a field of
//! [`SharedState`] when it is the same for every chain, and a field of
//! [`ChainState`] when a fork replaces it. Per-call data (a section, a
//! `var` snapshot) stays in parameters or on the per-section frame.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::AtomicU32;
use std::sync::{Arc, Mutex};

use runvil_types::cancel::CancelHandle;
use runvil_types::emitter::{Emitter, EventSink};
use runvil_types::event::Event;
use runvil_types::ids::{ChainId, TaskId};
use runvil_types::packs::{PackId, Prelude};
use runvil_types::tools::ToolCatalog;
use runvil_types::untrusted::GuardNonce;
use runvil_vfs::{Access, ScopeHandle, VfsRef};

use crate::Error;
use crate::lua::{Argv, RosterEntry, ToolSet};
use crate::model::{ModelBinding, ModelRole, ModelSet};
use crate::prompt::Prompt;

use super::fill;
use super::params::{RunLimits, RunParams};
use super::requirements::Requirements;
use super::section_context::SectionSys;
use super::section_vm::{SectionVmSetup, VmSeed};

/// A task's reporting handles: its task-scoped event emitter and its
/// model-turn counter.
#[derive(Clone, Debug)]
pub(super) struct ReportingHandles {
    /// The task-scoped event emitter, which also knows whether the run
    /// captures raw model-turn bodies.
    pub(super) emitter: Arc<Emitter>,
    /// The model-turn counter the task advances.
    pub(super) turns: Arc<AtomicU32>,
}

/// The run-wide state every chain of one run shares: one `Arc` per run,
/// built with the root chain's state and never forked.
///
/// Every field is the same for every chain. Its handles are fixed for the
/// run, though the cancel flag and the event buffer change behind them.
struct SharedState {
    /// The prompt this run executes.
    prompt: Arc<Prompt>,
    /// The untrusted-envelope nonce, derived once here from the run's seed
    /// so every wrap in the run shares it.
    nonce: GuardNonce,
    /// The run's VFS handle: the run's whole filesystem, the declared
    /// store backing every section's Lua `store` table included. Chain
    /// steps acquire or spawn their access capabilities from it.
    vfs: VfsRef,
    /// The run's resource limits.
    limits: RunLimits,
    /// The run's event buffer, shared by every chain's emitter, spawned
    /// task chains' included, and drained once per `step` into the batch
    /// returned to the caller.
    events: EventSink,
    /// The run's cancel flag: polled between chain steps and installed on
    /// every section VM's instruction hook. It is the one handle the params
    /// hold, the flag the caller supplies with `RunParams::with_cancel` and
    /// keeps its own clone of.
    cancel: CancelHandle,
    /// The run's tool set: built from the params' catalog at construction
    /// and changed by nothing during the run, so every reader, every
    /// section VM (H1 included) among them, shares it with no lock and no
    /// copy.
    tool_set: Arc<ToolSet>,
    /// The run's model set: filled from the params' role map at
    /// construction, shared with every section VM (H1 included) and changed
    /// by nothing during the run, so every reader shares it with no lock and
    /// no copy.
    model_set: Arc<ModelSet>,
    /// The refusal the run's first step fails with: what the prompt
    /// requires that the params lack, or `None` when they lack nothing.
    refusal: Option<Error>,
    /// The run's `started_at` rendered as RFC 3339, stamped into every
    /// section's `sys.when`, the H1 pass included.
    when: Arc<str>,
    /// The run's application-state snapshot, when the caller supplied one.
    ui: Option<Arc<serde_json::Value>>,
    /// The run's Pack preludes, in install order: every section VM
    /// installs each one before `require`.
    preludes: Arc<[Prelude]>,
    /// Test-only: installs the raw `tools.call_as_model` shim in every
    /// section VM, so a fixture section can yield one model-issued
    /// `tool_call` at the scheduler's dispatch arm without going through a
    /// loop shim.
    #[cfg(test)]
    raw_shims: bool,
}

/// One chain's state: the run's [`SharedState`] and the fields a chain's
/// fork replaces.
///
/// Cheap to clone: a clone shares the run's state and this chain's
/// reporting handles, so it advances the same turn counter.
/// The three sanctioned forks: [`with_walk_state`](Self::with_walk_state)
/// at the H1-to-walk handoff, [`with_task`](Self::with_task) giving a
/// spawned chain its own task emitter and turn counter, and
/// [`with_args`](Self::with_args) passing a `call` or task-spawning arm's
/// input override into its contained chain.
#[derive(Clone)]
pub(crate) struct ChainState {
    /// The run-wide state every chain of the run shares.
    shared: Arc<SharedState>,
    /// This context's reporting handles: the root task's at construction,
    /// a spawned chain's own after [`with_task`](Self::with_task). The Lua
    /// layer's seams (`require`'s library loads, `log`, teardown, the
    /// shared tool-dispatch body) take the emitter too, so their reports
    /// land in the buffer in order with the scheduler's own.
    reporting: ReportingHandles,
    /// The run's argument string for `{{ args }}` substitution.
    args: Arc<str>,
    /// The run's `argv`, as `derive_argv` derives it from `args`. At
    /// construction this is the value the H1 pass starts from; the walk's
    /// fork holds the value H1 left behind at the freeze, so an H1 repair
    /// reaches every downstream section.
    argv: Option<Arc<serde_json::Value>>,
}

impl ChainState {
    /// Builds the run's shared state and the root chain's state for one
    /// run of `prompt` over `params`, which it consumes. The turn counter
    /// is minted here (starting at zero), as are the run's shared tool and
    /// model sets - the tool set built from the catalog and the roster,
    /// the model set filled from the role map (it stays empty for an empty
    /// map) - and the refusal for what the prompt requires that
    /// `params` lacks; the nonce derives from `params`'s seed and `when`
    /// renders `params`'s `started_at`, so two states over the same inputs
    /// agree on both.
    #[must_use]
    pub(super) fn root(prompt: Arc<Prompt>, args: &str, params: RunParams) -> Self {
        let RunParams {
            seed,
            started_at,
            provenance_start,
            report_debug,
            cancel,
            limits,
            ui,
            vfs,
            models,
            tools,
            packs,
            preludes,
        } = params;
        let mut requirements = Requirements::default();
        let model_set = Arc::new(fill::fill_model_set(&prompt, models, &mut requirements));
        requirements.check_packs(&prompt, &tools, packs.as_ref());
        let refusal = requirements.refusal(&prompt);
        let tool_set = Arc::new(run_tool_set(&prompt, tools, packs.unwrap_or_default()));
        // The root task's event sequence starts at the params'
        // `provenance_start`: past any parse events the caller put in the
        // stream ahead of the run, or at zero.
        let events = EventSink::seeded(provenance_start);
        // The root chain - the main walk - is task `0`.
        let emitter = Arc::new(Emitter::new(
            events.clone(),
            TaskId::from(ChainId::root()),
            report_debug,
        ));
        let derived_argv = derive_argv(&prompt, args).map(Arc::from);
        Self {
            shared: Arc::new(SharedState {
                prompt,
                nonce: GuardNonce::from_seed(&seed),
                vfs,
                limits,
                events,
                cancel,
                tool_set,
                model_set,
                refusal,
                when: Arc::from(started_at.to_rfc3339()),
                ui: ui.map(Arc::new),
                preludes: preludes.into(),
                #[cfg(test)]
                raw_shims: false,
            }),
            reporting: ReportingHandles {
                emitter,
                turns: Arc::new(AtomicU32::new(0)),
            },
            args: Arc::from(args),
            argv: derived_argv,
        }
    }

    /// The run-wide state, for a test hook that writes it.
    ///
    /// # Panics
    /// Panics once the state has been cloned, because every clone shares
    /// the one [`SharedState`]. The hooks run before the run starts, when
    /// nothing has cloned it yet.
    #[cfg(test)]
    fn shared_mut_for_test(&mut self) -> &mut SharedState {
        Arc::get_mut(&mut self.shared)
            .expect("a test hook writes the run-wide state before anything clones it")
    }

    /// Exposes the raw `tools.call_as_model` shim in every section VM this
    /// run starts, so a test fixture can drive the scheduler's `tool_call`
    /// arm with one model-issued call.
    #[cfg(test)]
    pub(crate) fn expose_raw_shims_for_test(&mut self) {
        self.shared_mut_for_test().raw_shims = true;
    }

    /// Sets the run's model set to `set`, so a test can run a prompt
    /// over bindings it builds by hand. Call it before the run starts: every
    /// section VM shares the set it finds here.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn with_models_for_test(mut self, set: ModelSet) -> Self {
        self.shared_mut_for_test().model_set = Arc::new(set);
        self
    }

    /// The prompt this run executes.
    pub(super) fn prompt(&self) -> &Arc<Prompt> {
        &self.shared.prompt
    }

    /// The run's cancel flag.
    pub(super) fn cancel(&self) -> &CancelHandle {
        &self.shared.cancel
    }

    /// The run's untrusted-envelope nonce.
    pub(super) fn nonce(&self) -> &GuardNonce {
        &self.shared.nonce
    }

    /// The run's VFS handle.
    pub(super) fn vfs(&self) -> &VfsRef {
        &self.shared.vfs
    }

    /// The run's argument string.
    pub(super) fn args(&self) -> &Arc<str> {
        &self.args
    }

    /// The run's `argv`, as `derive_argv` derives it; on the walk,
    /// the value H1 left behind at the freeze.
    pub(super) fn argv(&self) -> Option<&Arc<serde_json::Value>> {
        self.argv.as_ref()
    }

    /// The run's resource limits.
    pub(super) fn limits(&self) -> RunLimits {
        self.shared.limits
    }

    /// This context's reporting handles: its task-scoped emitter and its
    /// model-turn counter.
    pub(super) fn reporting(&self) -> &ReportingHandles {
        &self.reporting
    }

    /// This context's task-scoped emitter: where every report the
    /// scheduler makes on this chain goes.
    pub(super) fn emitter(&self) -> &Arc<Emitter> {
        &self.reporting.emitter
    }

    /// Drains the run's event buffer: every event pushed since the last
    /// drain, in push order. The run's `step` calls this once per step and
    /// returns the batch to the caller.
    pub(super) fn take_events(&self) -> Vec<Event> {
        self.shared.events.take()
    }

    /// The model-turn counter this context advances.
    pub(super) fn turns(&self) -> &Arc<AtomicU32> {
        &self.reporting.turns
    }

    /// The run's shared tool set, the one every section VM the run
    /// constructs holds.
    pub(super) fn tool_set(&self) -> &Arc<ToolSet> {
        &self.shared.tool_set
    }

    /// The binding of the `default` role: the model a section starts on.
    /// It is `None` when the caller supplied no model.
    pub(super) fn default_binding(&self) -> Option<ModelBinding> {
        self.shared
            .model_set
            .binding(ModelRole::Default.as_str())
            .cloned()
    }

    /// The run's model set, shared with every section VM the run
    /// constructs.
    pub(super) fn model_set(&self) -> Arc<ModelSet> {
        Arc::clone(&self.shared.model_set)
    }

    /// The refusal the run's first step fails with, or `None` when the
    /// params lack nothing the prompt requires.
    pub(super) fn refusal(&self) -> Option<&Error> {
        self.shared.refusal.as_ref()
    }

    /// The resolved tool-loop cap per `models.loop` call: the frontmatter's
    /// `max_tool_iterations` over the limits default, held to the limits
    /// ceiling.
    fn max_tool_iterations(&self) -> usize {
        let limits = &self.shared.limits;
        let cap = self
            .shared
            .prompt
            .frontmatter()
            .max_tool_iterations()
            .unwrap_or(limits.max_tool_iterations);
        cap.min(limits.tool_iterations_ceiling).get() as usize
    }

    /// The run's top-level section count, reported as `sys.section_count`.
    fn section_count(&self) -> usize {
        crate::prompt::detail::sections(&self.shared.prompt).len()
    }

    /// The H1-to-walk handoff: the `argv` H1 left behind at the freeze,
    /// set on a cheap clone so the state H1 saw stays untouched. The
    /// run-wide state needs no delta: the tool and model sets were built at
    /// construction and nothing writes either during the run, and `when`
    /// is the run's `started_at`, the same for the pass and the walk.
    #[must_use]
    pub(super) fn with_walk_state(&self, argv: Option<serde_json::Value>) -> Self {
        let mut ctx = self.clone();
        ctx.argv = argv.map(Arc::from);
        ctx
    }

    /// The context a contained chain runs under: `args` in place of the
    /// run's own, because a `call`, a `tasks.spawn`, or a `tasks.fanout`
    /// arm that was given an input overrides the run's args for the chain -
    /// and `argv` re-derives from the chain's args, so the chain sees the
    /// `argv` of what it was passed.
    #[must_use]
    pub(super) fn with_args(&self, args: &str) -> Self {
        let mut ctx = self.clone();
        ctx.argv = derive_argv(&self.shared.prompt, args).map(Arc::from);
        ctx.args = Arc::from(args);
        ctx
    }

    /// The context a spawned task chain runs under: an emitter stamping
    /// the chain's own `task` on every report, and `turns` in place of the
    /// run's counter, so its turn numbers, and `tasks.status`'s `turns`,
    /// count only its own rounds.
    #[must_use]
    pub(super) fn with_task(&self, task: TaskId, turns: Arc<AtomicU32>) -> Self {
        let mut ctx = self.clone();
        ctx.reporting = ReportingHandles {
            emitter: Arc::new(self.reporting.emitter.for_task(task)),
            turns,
        };
        ctx
    }

    /// The borrowed VM-setup inputs both frame constructors share, sourcing
    /// the run-wide inputs from this context; the constructor supplies only
    /// its own deltas: the `sys` JSON, the seed, the chain step's access
    /// capability (the walk's own, a call chain's borrowed parent
    /// capability, a task chain's spawned one) and the handle on the run's
    /// scope, the section's current-model selection, the section name, and
    /// whether `argv` installs writable.
    ///
    /// `argv_writable` picks the [`Argv`] mode that
    /// [`SectionVmSetup::argv`] describes; the H1 pass is the only
    /// production caller that passes `true`.
    #[expect(
        clippy::too_many_arguments,
        reason = "each input is one of the constructor's own deltas, borrowed separately for the setup's life"
    )]
    pub(super) fn vm_setup<'a>(
        &'a self,
        sys: &'a serde_json::Value,
        seed: VmSeed<'a>,
        access: &'a Arc<Access>,
        scope: &'a ScopeHandle,
        selection: &'a Arc<Mutex<Option<ModelBinding>>>,
        section_name: &'a str,
        argv_writable: bool,
    ) -> SectionVmSetup<'a> {
        let argv = if argv_writable {
            Argv::Writable(self.argv.as_deref())
        } else {
            Argv::Frozen(self.argv.as_deref())
        };
        SectionVmSetup {
            args: &self.args,
            argv,
            sys,
            access,
            scope,
            selection,
            seed,
            emitter: &self.reporting.emitter,
            section_name,
            file: self.shared.prompt.file(),
            max_tool_iterations: self.max_tool_iterations(),
            ui: self.shared.ui.as_ref(),
            preludes: &self.shared.preludes,
            #[cfg(test)]
            raw_shims: self.shared.raw_shims,
        }
    }

    /// The `sys` for one section entry of this run: the five shared fields
    /// in one construction, with the constructor supplying only the
    /// section entry's hierarchical id, the entering chain's task id, and
    /// the section name. A task's first entry sets `index` at its own call
    /// site.
    pub(super) fn section_sys(&self, id: &str, task_id: &TaskId, section_name: &str) -> SectionSys {
        SectionSys {
            when: Arc::clone(&self.shared.when),
            id: id.to_owned(),
            taskid: task_id.clone(),
            section_name: section_name.to_owned(),
            section_count: self.section_count(),
            index: None,
            model: None,
        }
    }
}

impl fmt::Debug for ChainState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChainState")
            .field("shared", &self.shared)
            .field("reporting", &self.reporting)
            .field("args", &self.args)
            .field("argv", &self.argv)
            .finish()
    }
}

impl fmt::Debug for SharedState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut state = f.debug_struct("SharedState");
        #[cfg(test)]
        state.field("raw_shims", &self.raw_shims);
        state
            .field("prompt", &self.prompt)
            .field("nonce", &self.nonce)
            .field("vfs", &"<VfsRef>")
            .field("limits", &self.limits)
            .field("events", &self.events)
            .field("cancel", &self.cancel)
            .field("tool_set", &self.tool_set)
            .field("model_set", &self.model_set)
            .field("refusal", &self.refusal)
            .field("when", &self.when)
            .field("ui", &self.ui)
            .field(
                "preludes",
                &self.preludes.iter().map(Prelude::pack).collect::<Vec<_>>(),
            )
            .finish()
    }
}

/// Builds the run's shared tool set: the frontmatter's declared Packs,
/// and the run catalog and roster, all moved out of the params. Nothing
/// is bound here: each section entry binds the tools it offers when it
/// freezes its sets.
fn run_tool_set(
    prompt: &Prompt,
    tools: ToolCatalog,
    packs: BTreeMap<PackId, RosterEntry>,
) -> ToolSet {
    ToolSet::new(
        prompt
            .frontmatter()
            .uses()
            .iter()
            .map(|entry| entry.id().clone())
            .collect(),
        packs,
        tools,
    )
}

/// Derives a section's `argv` from the args string under the prompt's
/// declaration: a default-declared prompt wraps the interface prose into
/// the default shape (`argv.prose`, with the empty string present, not
/// absent); a structured declaration parses the string as JSON, and a parse
/// failure or a JSON `null` reads as nil (`if argv then` is the malformed
/// check). The executor never hard-errors on shape.
fn derive_argv(prompt: &Prompt, args: &str) -> Option<serde_json::Value> {
    if prompt.frontmatter().args().is_default() {
        return Some(serde_json::json!({ "prose": args }));
    }
    match serde_json::from_str(args) {
        Ok(serde_json::Value::Null) | Err(_) => None,
        Ok(value) => Some(value),
    }
}

#[cfg(test)]
mod tests {
    //! Tests for `ChainState` construction, forking, task sequence seeding,
    //! and the model set a run binds from its filled roles.

    use std::collections::BTreeMap;
    use std::num::NonZeroU32;

    use runvil_types::packs::PackId;
    use runvil_types::tools::ToolId;

    use serde_json::json;

    use super::*;
    use crate::execute::section_vm::VmSeed;
    use crate::lua::{Argv, RosterEntry};
    use crate::model::{ModelRole, ThinkingMode};
    use crate::test_support::fresh_access;
    use crate::test_support::scenario::{TEST_SEED, test_model_with};

    fn test_prompt() -> Prompt {
        let source = concat!(
            "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n",
            "# Title\n\n## Only\n\ndone\n",
        );
        Prompt::parse(&|_: &str| Ok(source.to_owned()), "prompt")
            .0
            .expect("the test prompt parses")
    }

    fn test_state(prompt: &Prompt) -> ChainState {
        state_over(
            prompt,
            RunParams::new(TEST_SEED, runvil_types::timestamp::Timestamp::UNIX_EPOCH),
        )
    }

    /// The state a run of `prompt` assembles from `params`, as `Run::new`
    /// assembles it.
    fn state_over(prompt: &Prompt, params: RunParams) -> ChainState {
        ChainState::root(Arc::new(prompt.clone()), "", params)
    }

    #[test]
    fn root_builds_a_context_over_the_prompt() {
        let prompt = test_prompt();
        let ctx = test_state(&prompt);
        assert_eq!(ctx.prompt().title(), prompt.title());
    }

    #[test]
    fn accessor_returns_the_run_prompt() {
        let prompt = test_prompt();
        let ctx = test_state(&prompt);
        assert_eq!(**ctx.prompt(), prompt);
    }

    #[test]
    fn clones_share_the_prompt_allocation() {
        let ctx = test_state(&test_prompt());
        let clone = ctx.clone();
        assert!(Arc::ptr_eq(ctx.prompt(), clone.prompt()));
    }

    #[test]
    fn derived_values_come_from_the_prompt_and_limits() {
        let prompt = test_prompt();
        let ctx = test_state(&prompt);
        assert_eq!(
            ctx.section_count(),
            runvil_prompt::detail::sections(&prompt).len()
        );
        assert_eq!(ctx.max_tool_iterations(), 24);
    }

    #[test]
    fn the_ceiling_caps_the_limits_default_too() {
        let limits = RunLimits {
            max_tool_iterations: NonZeroU32::new(50).expect("50 is non-zero"),
            tool_iterations_ceiling: NonZeroU32::new(10).expect("10 is non-zero"),
            ..RunLimits::default()
        };
        let ctx = state_over(
            &test_prompt(),
            RunParams::new(TEST_SEED, runvil_types::timestamp::Timestamp::UNIX_EPOCH)
                .with_limits(limits),
        );
        assert_eq!(ctx.max_tool_iterations(), 10);
    }

    #[test]
    fn vm_setup_hands_the_vm_the_runs_argv_in_the_mode_the_caller_asks_for() {
        let prompt = test_prompt();
        let ctx = test_state(&prompt).with_args("{\"query\":\"papers\"}");
        let sys = json!({});
        let (access, scope) = fresh_access();
        let requested = ctx.argv().cloned();
        let expected = requested.as_deref();
        assert!(
            expected.is_some(),
            "the fixture's args must derive an argv, or neither mode is observable"
        );
        let selection = Arc::new(Mutex::new(None));
        let frozen = ctx.vm_setup(
            &sys,
            VmSeed::default(),
            &access,
            &scope,
            &selection,
            "Only",
            false,
        );
        assert!(
            matches!(frozen.argv, Argv::Frozen(value) if value == expected),
            "an unprivileged constructor must get the run's argv frozen: {:?}",
            frozen.argv
        );
        let writable = ctx.vm_setup(
            &sys,
            VmSeed::default(),
            &access,
            &scope,
            &selection,
            "Only",
            true,
        );
        assert!(
            matches!(writable.argv, Argv::Writable(value) if value == expected),
            "the H1 pass's flag must get the run's argv writable: {:?}",
            writable.argv
        );
    }

    #[test]
    fn forks_swap_only_their_own_fields() {
        let ctx = test_state(&test_prompt());
        let chain = ctx.with_args("chain-args");
        assert_eq!(&**chain.args(), "chain-args");
        assert!(Arc::ptr_eq(&ctx.shared, &chain.shared));
        assert_eq!(&**ctx.args(), "");

        let turns = Arc::new(AtomicU32::new(7));
        let task: TaskId = "0.4".parse().expect("a task id parses");
        let arm = ctx.with_task(task.clone(), Arc::clone(&turns));
        assert!(Arc::ptr_eq(arm.turns(), &turns));
        assert!(Arc::ptr_eq(&ctx.shared, &arm.shared));
        assert_eq!(arm.emitter().task(), &task);
        assert_eq!(ctx.emitter().task(), &TaskId::from(ChainId::root()));
    }

    #[test]
    fn the_root_task_sequence_starts_where_the_params_say() {
        let ctx = state_over(
            &test_prompt(),
            RunParams::new(TEST_SEED, runvil_types::timestamp::Timestamp::UNIX_EPOCH)
                .with_provenance_start(5),
        );
        ctx.emitter()
            .report("Only", runvil_types::event_lifecycle::SECTION_STARTED);
        let events = ctx.take_events();
        assert_eq!(events[0].provenance().task, TaskId::from(ChainId::root()));
        assert_eq!(
            events[0].provenance().seq,
            5,
            "the root task's first stamp continues past the caller's parse events"
        );
    }

    #[test]
    fn a_task_fork_reports_into_the_shared_buffer_under_its_own_task() {
        let ctx = test_state(&test_prompt());
        let task: TaskId = "0.1".parse().expect("a task id parses");
        let arm = ctx.with_task(task.clone(), Arc::new(AtomicU32::new(0)));
        ctx.emitter()
            .report("Only", runvil_types::event_lifecycle::SECTION_STARTED);
        arm.emitter()
            .report("Only", runvil_types::event_lifecycle::SECTION_STARTED);
        // Both emitters share one buffer, drained through either context.
        let events = arm.take_events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].provenance().task, TaskId::from(ChainId::root()));
        assert_eq!(events[1].provenance().task, task);
        assert_eq!(events[1].provenance().seq, 0);
    }

    #[test]
    fn a_filled_role_binds_with_the_catalog_info_its_render_receives() {
        let prompt = test_prompt();
        let info = test_model_with("catalog-model", 65_536, ThinkingMode::Switchable)
            .with_provider("acme");
        let ctx = RunParams::new(TEST_SEED, runvil_types::timestamp::Timestamp::UNIX_EPOCH)
            .with_models(BTreeMap::from([(ModelRole::Default, info.clone())]));
        let state = state_over(&prompt, ctx);
        let set = state.model_set();
        let binding = set.binding("default").expect("the filled role binds");
        assert_eq!(binding.render_info(), info);
        assert_eq!(state.refusal(), None, "the prompt requires nothing");
    }

    #[test]
    fn an_unavailable_roster_pack_records_no_vendor_and_a_pack_listed_twice_keeps_its_last_entry() {
        let web = PackId::parse("web").expect("a static valid Pack id");
        let usable = RosterEntry::Usable {
            vendor: "acme".to_owned(),
        };
        let down = RosterEntry::Unavailable {
            reason: "the server is down".to_owned(),
        };
        let state = state_over(
            &test_prompt(),
            RunParams::new(TEST_SEED, runvil_types::timestamp::Timestamp::UNIX_EPOCH)
                .with_packs([(web.clone(), usable), (web.clone(), down.clone())]),
        );
        let tool = ToolId::parse("web/fetch").expect("a static valid tool id");
        assert_eq!(
            state.tool_set().vendor_for(&tool),
            None,
            "an unavailable Pack has no vendor"
        );
        assert_eq!(
            state.tool_set().roster_for_test().get(&web),
            Some(&down),
            "one roster entry per Pack, the last one given, carrying its reason"
        );
    }
}
