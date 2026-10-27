//! The wait, inspection, note, concurrency, and cancel arms over the task arena.
//!
//! `join_any` is the one scheduler wait primitive: a chain names a set of
//! tasks it owns and is resumed with the first member that ends - at once
//! when one already has, otherwise when a member's chain end delivers it.
//! Delivery moves a `Done` entry to `Delivered` (its outcome is taken by
//! exactly one wait; a second wait raises `task_consumed`), and it joins
//! the task: everything the task did happens before the owner's next
//! step, so its store writes are visible from the very next operation. A
//! `Cancelled` entry delivers `ok = false` with the `cancelled` error
//! value and stays as it is, since it holds no result to consume - but
//! its delivery still joins its partial writes. An `Abandoned` entry is
//! never delivered: a task is abandoned because its owner ended, and only
//! the owner may wait on it, so no wait can reach the entry.
//!
//! Ownership is the rule for every arm: only the chain that owns a task -
//! its spawner, or the root walk for a task the H1 pass spawned, since the
//! pass and the walk share the root lineage `0` as two arena records - may
//! wait on, check, list, or cancel it, and an id naming no task
//! is refused the same way so a caller learns nothing about tasks it never
//! started. `status` and `note` add the self exception: a chain may read
//! and annotate the task it runs inside (`sys.taskid`), which is how a
//! task reports progress. The main walk is task `0` with no entry, so its
//! own status is not reportable, and a note outside a task with an entry
//! (the main walk and its `call` children) records nothing.
//!
//! Cancel is idempotent: a live task's entry moves to `Cancelled`, its
//! backing chain aborts with everything it owns, and `TaskCancelled` fires
//! once under its target; a task already in a terminal state is left as it
//! is and reports nothing.

use std::num::{NonZeroU64, NonZeroUsize};
use std::sync::atomic::Ordering;

use runvil_types::event::Event;
use runvil_types::ids::TaskId;

use crate::lua::{Answer, NonEmpty, TaskDelivery, TaskPhase, TaskStatus};
use crate::{Error, Result};

use super::admission::ChainTree;
use super::chain_record::Parked;
use super::tasks::{Stop, TaskBacking, TaskState};
use super::{ChainIndex, Scheduler};

/// The phase `tasks.status` reports: a delivered task reads `done`, since
/// delivery is the owner's bookkeeping, not a lifecycle change.
fn task_phase(state: TaskState) -> TaskPhase {
    match state {
        TaskState::Running => TaskPhase::Running,
        TaskState::Done { .. } | TaskState::Delivered { .. } => TaskPhase::Done,
        TaskState::Cancelled => TaskPhase::Cancelled,
        TaskState::Abandoned => TaskPhase::Abandoned,
    }
}

impl Scheduler {
    /// Dispatches a `join_any` request: every member must be a task the
    /// chain owns and none may be delivered already; the first terminal
    /// member in set order is delivered at once, otherwise the chain parks
    /// on the set until a member's chain end wakes it. Parking on a set
    /// with real members gives the chain's admission slots back, so its
    /// own tasks can run under its limit.
    pub(super) fn dispatch_join_any(
        &mut self,
        id: ChainIndex,
        tasks: NonEmpty<TaskId>,
    ) -> Result<()> {
        match self.tasks.first_terminal(id, &tasks) {
            Ok(Some(task)) => {
                let delivery = self.deliver(&task);
                self.answer_inline(id, Answer::JoinAny(Ok(delivery)))
            }
            Ok(None) => {
                // Only a chain-backed task has an identity; a timer has none.
                let waits_on_tasks = tasks.iter().any(|task| self.tasks.exec_of(task).is_some());
                self.chains[id].parked = Some(Parked::Tasks(tasks.into_vec()));
                let tree = ChainTree {
                    chains: &self.chains,
                    tasks: &self.tasks,
                };
                self.admission.park(id, &tree, waits_on_tasks);
                Ok(())
            }
            Err(error) => self.answer_inline(id, Answer::JoinAny(Err(error))),
        }
    }

    /// Delivers a terminal task to its owner: the join first, then the
    /// arena's [`take_delivery`](super::tasks::TaskArena::take_delivery).
    /// The delivery is the join: everything the task did happens before the
    /// owner's next step, so its store writes - a cancelled task's partial
    /// writes included - are visible from the very next operation.
    fn deliver(&mut self, task: &TaskId) -> TaskDelivery<Error> {
        if let Some(owner) = self.tasks.owner_of(task) {
            self.join_task(owner, task);
        }
        TaskDelivery {
            task: task.clone(),
            outcome: self.tasks.take_delivery(task),
        }
    }

    /// Wakes `task`'s owner if it is parked on a set containing `task`:
    /// the member is delivered as the wait's answer and the owner leaves
    /// its wait.
    pub(super) fn wake_waiter(&mut self, owner: ChainIndex, task: &TaskId) -> Result<()> {
        if !self.chains[owner].end_wait_for(task) {
            return Ok(());
        }
        let delivery = self.deliver(task);
        self.wake_from_wait(owner, Answer::JoinAny(Ok(delivery)))
    }

    /// Dispatches a `ready` request: whether a task the chain owns has
    /// ended, delivered or not.
    pub(super) fn dispatch_ready(&mut self, id: ChainIndex, task: &TaskId) -> Result<()> {
        let answer = self
            .tasks
            .owned(id, task)
            .map(|entry| !entry.state.is_live());
        self.answer_inline(id, Answer::Ready(answer))
    }

    /// Dispatches a `status` request over a task the chain owns or runs
    /// inside.
    pub(super) fn dispatch_status(&mut self, id: ChainIndex, task: &TaskId) -> Result<()> {
        let answer = self.task_status(id, task).map(Box::new);
        self.answer_inline(id, Answer::Status(answer))
    }

    /// Reads one task's status: the entry's facts and note, plus the backing
    /// chain's position while it is live (its section and what it is
    /// parked on) and the chain's counters, which the append-only arena
    /// keeps after the chain ends.
    fn task_status(&self, id: ChainIndex, task: &TaskId) -> Result<TaskStatus> {
        let entry = self.tasks.visible(id, &self.chains[id].task, task)?;
        let state = entry.state.view();
        let mut status = TaskStatus {
            target: entry.target.clone(),
            state: task_phase(state),
            ok: state.ok(),
            section: None,
            blocked: None,
            turns: 0,
            tasks: Vec::new(),
            depth: 0,
            note: entry.note.clone(),
        };
        if let TaskBacking::Chain { chain: backing, .. } = entry.backing {
            let chain = &self.chains[backing];
            status.turns = chain.ctx.turns().load(Ordering::Relaxed);
            status.depth = u32::try_from(chain.call_depth.get()).unwrap_or(u32::MAX);
            if entry.state.is_live() {
                status.section = chain
                    .entered
                    .is_some()
                    .then(|| chain.section_name().to_owned());
                status.blocked = chain.blocked();
                status.tasks = self.tasks.live_of(backing);
            }
        }
        Ok(status)
    }

    /// Dispatches a `pending` request: the chain's live tasks in spawn
    /// order.
    pub(super) fn dispatch_pending(&mut self, id: ChainIndex) -> Result<()> {
        let tasks = self.tasks.live_of(id);
        self.answer_inline(id, Answer::Pending(Ok(tasks)))
    }

    /// Dispatches a `concurrency` request: with a limit, the chain's
    /// effective admission limit becomes the limit clamped to the
    /// parent's (the caller's `RunLimits` ceiling for the root, the
    /// enclosing chain's current limit otherwise), so a prompt stays
    /// portable wherever it runs, tighter ceilings included; without one,
    /// the effective limit is read back. The clamp never preempts a task
    /// already running - the limit gates admissions from here on only.
    pub(super) fn dispatch_concurrency(
        &mut self,
        id: ChainIndex,
        limit: Option<NonZeroU64>,
    ) -> Result<()> {
        let effective = if let Some(limit) = limit {
            let tree = ChainTree {
                chains: &self.chains,
                tasks: &self.tasks,
            };
            let parent_limit = match tree.enclosing(id) {
                Some(parent) => tree.limit(parent),
                None => self.ctx.limits().max_concurrency,
            };
            let limit = NonZeroUsize::try_from(limit).unwrap_or(NonZeroUsize::MAX);
            let effective = limit.min(parent_limit);
            self.chains[id].concurrency = effective;
            effective
        } else {
            self.chains[id].concurrency
        };
        self.answer_inline(id, Answer::Concurrency(Ok(effective.get())))
    }

    /// Dispatches a `note` request: the text becomes the latest note of the
    /// task the chain runs inside, recorded on the task's entry, so a `call`
    /// child's note is the task's. A note outside a task with an entry (the
    /// main walk, task `0`, and its `call` children) records nothing.
    pub(super) fn dispatch_note(&mut self, id: ChainIndex, text: String) -> Result<()> {
        self.tasks.set_note(&self.chains[id].task, text);
        self.answer_inline(id, Answer::Note(Ok(())))
    }

    /// Dispatches a `cancel` request over a task the chain owns.
    pub(super) fn dispatch_cancel(&mut self, id: ChainIndex, task: &TaskId) -> Result<()> {
        let answer = self.cancel_task(id, task);
        self.answer_inline(id, Answer::Cancel(answer))
    }

    /// Cancels a live task `caller` owns: the entry moves to `Cancelled`, the
    /// backing ends (a chain with everything it owns, a request dropped),
    /// and `TaskCancelled` fires once under the target - except for an
    /// internal timer, whose cancel is the wait shim's own bookkeeping and
    /// reports nothing, and a task that was never admitted, which has no
    /// start event and reports no terminal. A task already in a terminal
    /// state is left as it is.
    fn cancel_task(&mut self, caller: ChainIndex, task: &TaskId) -> Result<()> {
        self.tasks.owned(caller, task)?;
        if let Some((backing, target)) = self.tasks.stop(task, Stop::Cancelled) {
            self.end_backing(backing, &target, |execution, section, provenance| {
                Event::TaskCancelled {
                    execution,
                    section,
                    provenance,
                    task: task.clone(),
                }
            });
        }
        Ok(())
    }
}
