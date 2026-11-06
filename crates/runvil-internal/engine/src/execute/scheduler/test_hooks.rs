//! Test-only hooks over the scheduler's private state: the seams the
//! `execute::tests` suites drive the scheduler's edge paths through
//! (overflow bounds, the effect table, task entries, answer delivery, and
//! a store dispatch with no store) and the turn counter and tool set they
//! read. Compiled only under test; nothing here
//! exists in a shipped Engine.

use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use runvil_types::ids::{ChainId, TaskId};
use runvil_vfs::{MemoryBackend, Origin, VfsRef};

use crate::Result;
use crate::execute::run::EffectId;
use crate::heading_address::{SectionIndex, SectionPos};
use crate::lua::{Answer, Request, ToolSet, VfsOp};

use super::chain::ChainStart;
use super::chain_record::{CallDepth, ChainKind};
use super::tasks::TaskBacking;
pub(crate) use super::tasks::TaskState;
use super::{ChainIndex, Counters, Scheduler};

impl Scheduler {
    /// Shrinks the chain-count bound so a test can drive the
    /// [`start_chain`](Self::start_chain) overflow path.
    pub(crate) fn set_max_chains_for_test(&mut self, limit: usize) {
        self.chains.set_max_for_test(limit);
    }

    /// Fills every admission slot of the chain at arena index `chain` with
    /// a task it owns whose chain holds its slot but never runs or ends, so
    /// a test can leave a queued task that no slot will ever admit.
    pub(crate) fn wedge_admission_for_test(&mut self, chain: u32) {
        let (owner, _) = self
            .chains
            .iter()
            .nth(chain as usize)
            .expect("the wedged chain is in the arena");
        for _ in 0..self.chains[owner].concurrency.get() {
            let lineage = self.chains[owner]
                .next_child_id()
                .expect("the wedged chain has a child id left");
            let task = TaskId::from(lineage.clone());
            let access = self
                .run_scope()
                .expect("the run holds its scope handle")
                .spawn(
                    self.chains[owner]
                        .access()
                        .expect("the wedged chain holds its access"),
                    Origin::at("wedge", self.ctx.prompt().title(), 1),
                )
                .expect("the wedged chain's access forks");
            let exec = access.id();
            let wedge = self
                .start_chain(ChainStart {
                    lineage,
                    counters: Counters::default(),
                    ctx: self.ctx.clone(),
                    model: self.ctx.default_binding(),
                    access: Arc::new(access),
                    start: SectionPos::top(SectionIndex::FIRST),
                    kind: ChainKind::Task,
                    var: serde_json::json!({}),
                    call_depth: CallDepth::ROOT,
                    concurrency: NonZeroUsize::MIN,
                })
                .expect("the arena has room for the wedge");
            self.tasks.insert(
                task,
                TaskBacking::Chain { chain: wedge, exec },
                owner,
                "wedge".to_owned(),
                None,
            );
            self.admission.hold_for_test(wedge);
        }
    }

    /// Orphans `effect` through the abort path a chain end, a cancel, or an
    /// abandonment takes for an effect its chain leaves in flight.
    pub(crate) fn abort_effect_for_test(&mut self, effect: EffectId) {
        self.abort_effect(effect);
    }

    /// Whether `effect` is recorded as orphaned: its chain stopped waiting,
    /// and the caller's answer, still owed, is discarded on arrival.
    pub(crate) fn orphaned_for_test(&self, effect: EffectId) -> bool {
        self.effects.is_orphaned(effect)
    }

    /// The state of one task's entry, or `None` when no task with that id
    /// was ever started, so a test can prove a chain's end moved its entry.
    pub(crate) fn task_state_for_test(&self, task: &TaskId) -> Option<TaskState> {
        self.tasks.state_of(task)
    }

    /// The run's tool set: the Packs the prompt declares, the roster, and
    /// the run catalog.
    pub(crate) fn tool_set_for_test(&self) -> &ToolSet {
        self.ctx.tool_set()
    }

    /// The model-turn counter of the run's ambient context, which the main
    /// walk and its `call` children advance.
    pub(crate) fn turns_for_test(&self) -> u32 {
        self.ctx.turns().load(Ordering::Relaxed)
    }

    /// Starts a chain over the top-level slice at `index`, as a jump out
    /// of the H1 pass starts the walk, and returns the section name the
    /// chain reports before its first entry.
    ///
    /// # Errors
    /// Returns a store failure when the backend refuses the chain's
    /// access, or an `Internal` failure when the run's chain count exceeds
    /// its bound.
    pub(crate) fn name_before_first_entry_for_test(&mut self, index: usize) -> Result<String> {
        let id = self.start_probe_chain(index)?;
        Ok(self.chains[id].section_name().to_owned())
    }

    /// Starts a chain that has entered no section, so it holds no
    /// suspended block, and answers it through the inline path and then
    /// a wait's wake, returning what each delivery reported.
    ///
    /// # Errors
    /// As [`name_before_first_entry_for_test`](Self::name_before_first_entry_for_test).
    pub(crate) fn answer_unsuspended_for_test(&mut self) -> Result<[Result<()>; 2]> {
        let id = self.start_probe_chain(0)?;
        let inline = self.answer_inline(id, Ok(Answer::Note));
        let woken = self.wake_from_wait(id, Ok(Answer::Note));
        Ok([inline, woken])
    }

    /// Delivers `Note` and then `Concurrency(1)` to the chain at arena
    /// index 0 with no resume between them, returning what each delivery
    /// reported and whether the suspended block still holds the `Note`.
    pub(crate) fn deliver_twice_for_test(&mut self) -> ([Result<()>; 2], bool) {
        let (id, _) = self
            .chains
            .iter()
            .next()
            .expect("the run has started a chain");
        let first = self.chains[id].deliver(Ok(Answer::Note));
        let second = self.chains[id].deliver(Ok(Answer::Concurrency(1)));
        let holds_first = matches!(
            self.chains[id]
                .entered
                .as_ref()
                .and_then(|entered| entered.running.as_ref())
                .and_then(|running| running.answer.as_ref()),
            Some(Ok(Answer::Note))
        );
        ([first, second], holds_first)
    }

    /// Dispatches `op` again for the chain at arena index 0, whose block
    /// is suspended on a store request, through an access from a handle
    /// that declares no store: the store-view failure the run's start
    /// probe otherwise rules out. The access's own scope handle replaces
    /// the run's, since a scope handle derives store views for its own
    /// scope alone.
    ///
    /// # Errors
    /// Returns the dispatch's store failure.
    pub(crate) fn dispatch_store_without_a_store_for_test(&mut self, op: VfsOp) -> Result<()> {
        let (id, _) = self
            .chains
            .iter()
            .next()
            .expect("the run has started a chain");
        let (access, scope) = VfsRef::new(MemoryBackend::new())
            .acquire_scope(Origin::new("no store"))
            .expect("a memory backend acquires");
        self.chains[id].access = Some(Arc::new(access));
        self.scope = Some(scope);
        self.dispatch(id, Request::Store { op })
    }

    /// Starts a root chain over the top-level slice at `index` under a
    /// fresh access, entering no section.
    fn start_probe_chain(&mut self, index: usize) -> Result<ChainIndex> {
        let access = self
            .ctx
            .vfs()
            .acquire(Origin::at("probe", self.ctx.prompt().title(), 1))
            .map_err(|cause| crate::error::store(&cause))?;
        self.start_chain(ChainStart {
            lineage: ChainId::root(),
            counters: Counters::default(),
            ctx: self.ctx.clone(),
            model: self.ctx.default_binding(),
            access: Arc::new(access),
            start: SectionPos::top(SectionIndex::new(index)),
            kind: ChainKind::Root,
            var: serde_json::json!({}),
            call_depth: CallDepth::ROOT,
            concurrency: NonZeroUsize::MIN,
        })
    }
}
