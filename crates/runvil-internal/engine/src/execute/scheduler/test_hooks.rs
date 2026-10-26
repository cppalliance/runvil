//! Test-only hooks over the scheduler's private state: the seams the
//! `execute::tests` suites drive the scheduler's edge paths through
//! (overflow bounds, the effect table, task slots, and answer delivery)
//! and the turn counter and tool set they read. Compiled only under test; nothing here
//! exists in a shipped Engine.

use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use runvil_types::ids::{ChainId, TaskId};
use runvil_vfs::{Origin, detail};

use crate::execute::run::EffectId;
use crate::execute::walk_target::SlicePath;
use crate::lua::{Answer, ToolSet};
use crate::{Error, Result};

use super::chain::ChainStart;
use super::chain_record::{Admission, CallDepth, ChainKind, SectionIndex};
pub(crate) use super::tasks::TaskState;
use super::tasks::{SlotState, TaskBacking, TaskSlot};
use super::{ChainIndex, Counters, Outstanding, Scheduler};

impl Scheduler {
    /// Shrinks the chain-count bound so a test can drive the
    /// [`start_chain`](Self::start_chain) overflow path.
    pub(crate) fn set_max_chains_for_test(&mut self, limit: usize) {
        self.max_chains = limit;
    }

    /// Fills every admission slot of the chain at arena index `chain` with
    /// a task it owns whose chain holds its slot but never runs or ends, so
    /// a test can leave a queued task that no slot will ever admit.
    pub(crate) fn wedge_admission_for_test(&mut self, chain: u32) {
        let owner = ChainIndex(chain);
        for _ in 0..self.chains[owner.index()].concurrency.get() {
            let lineage = self
                .allocate_child_id(owner)
                .expect("the wedged chain has a child id left");
            let task = TaskId::from(lineage.clone());
            let access = detail::access_spawn(
                self.chains[owner.index()]
                    .access()
                    .expect("the wedged chain holds its access"),
                Origin::at("wedge", self.ctx.prompt().title(), 1),
            )
            .expect("the wedged chain's access forks");
            let exec = detail::access_id(&access);
            let wedge = self
                .start_chain(ChainStart {
                    lineage,
                    counters: Counters::default(),
                    ctx: self.ctx.clone(),
                    access: Arc::new(access),
                    slice: SlicePath::root(),
                    index: SectionIndex::FIRST,
                    kind: ChainKind::Task {
                        admission: Admission::Admitted { holding: true },
                    },
                    var: serde_json::json!({}),
                    call_depth: CallDepth::ROOT,
                    concurrency: NonZeroUsize::MIN,
                })
                .expect("the arena has room for the wedge");
            self.tasks.insert(
                task,
                TaskSlot {
                    backing: TaskBacking::Chain { chain: wedge, exec },
                    owner,
                    target: "wedge".to_owned(),
                    state: SlotState::Running,
                    note: None,
                },
            );
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
        matches!(self.outstanding.get(&effect), Some(Outstanding::Orphaned))
    }

    /// The state of one task's slot, or `None` when no task with that id
    /// was ever started, so a test can prove a chain's end moved its slot.
    pub(crate) fn task_state_for_test(&self, task: &TaskId) -> Option<TaskState> {
        self.tasks.get(task).map(|slot| slot.state.view())
    }

    /// The run's tool set: the Packs the prompt declares, the roster, the
    /// run catalog, and each Pack's vendor.
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
    /// Returns [`Error::Store`](crate::Error::Store) when the backend
    /// refuses the chain's access, or
    /// [`Error::Internal`](crate::Error::Internal) when the run's chain
    /// count exceeds its bound.
    pub(crate) fn name_before_first_entry_for_test(&mut self, index: usize) -> Result<String> {
        let id = self.start_probe_chain(index)?;
        Ok(self.chains[id.index()].section_name().to_owned())
    }

    /// Starts a chain that has entered no section, so it holds no
    /// suspended block, and answers it through the inline path and then
    /// a wait's wake, returning what each delivery reported.
    ///
    /// # Errors
    /// As [`name_before_first_entry_for_test`](Self::name_before_first_entry_for_test).
    pub(crate) fn answer_unsuspended_for_test(&mut self) -> Result<[Result<()>; 2]> {
        let id = self.start_probe_chain(0)?;
        let inline = self.answer_inline(id, Answer::Note(Ok(())));
        let woken = self.wake_from_wait(id, Answer::Note(Ok(())));
        Ok([inline, woken])
    }

    /// Starts a root chain over the top-level slice at `index` under a
    /// fresh access, entering no section.
    fn start_probe_chain(&mut self, index: usize) -> Result<ChainIndex> {
        let access = self
            .ctx
            .vfs()
            .acquire(Origin::at("probe", self.ctx.prompt().title(), 1))
            .map_err(Error::store)?;
        self.start_chain(ChainStart {
            lineage: ChainId::root(),
            counters: Counters::default(),
            ctx: self.ctx.clone(),
            access: Arc::new(access),
            slice: SlicePath::root(),
            index: SectionIndex::new(index),
            kind: ChainKind::Root,
            var: serde_json::json!({}),
            call_depth: CallDepth::ROOT,
            concurrency: NonZeroUsize::MIN,
        })
    }
}
