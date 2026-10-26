//! The wait, inspection, note, concurrency, and cancel arms over the task arena.
//!
//! `join_any` is the one scheduler wait primitive: a chain names a set of
//! tasks it owns and is resumed with the first member that ends - at once
//! when one already has, otherwise when a member's chain end delivers it.
//! Delivery moves a `Done` slot to `Delivered` (its outcome is taken by
//! exactly one wait; a second wait raises `task_consumed`), and it joins
//! the task: everything the task did happens before the owner's next
//! step, so its store writes are visible from the very next operation. A
//! `Cancelled` slot delivers `ok = false` with the `cancelled` error
//! value and stays as it is, since it holds no result to consume - but
//! its delivery still joins its partial writes. An `Abandoned` slot is
//! never delivered: a task is abandoned because its owner ended, and only
//! the owner may wait on it, so no wait can reach the slot.
//!
//! Ownership is the rule for every arm: only the chain that owns a task -
//! its spawner, or the root walk for a task the H1 pass spawned, since the
//! pass and the walk share the root lineage `0` as two arena records - may
//! wait on, check, list, or cancel it, and an id naming no task
//! is refused the same way so a caller learns nothing about tasks it never
//! started. `status` and `note` add the self exception: a chain may read
//! and annotate the task it runs inside (`sys.taskid`), which is how a
//! task reports progress. The main walk is task `0` with no slot, so its
//! own status is not reportable, and a note outside a slotted task (the
//! main walk and its `call` children) records nothing.
//!
//! Cancel is idempotent: a live task's slot moves to `Cancelled`, its
//! backing chain aborts with everything it owns, and `TaskCancelled` fires
//! once under its target; a task already in a terminal state is left as it
//! is and reports nothing.

use std::num::{NonZeroU64, NonZeroUsize};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use runvil_types::event::Event;
use runvil_types::ids::TaskId;

use crate::lua::{Answer, NonEmpty, TaskDelivery, TaskPhase, TaskStatus};
use crate::{Error, Result};

use super::chain_record::Parked;
use super::tasks::{SlotState, TaskBacking, TaskSlot, TaskState};
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
    /// The slot of a task `caller` owns, or [`Error::TaskNotOwned`] - for
    /// an unknown id too.
    fn owned_slot(&self, caller: ChainIndex, task: &TaskId) -> Result<&TaskSlot> {
        match self.tasks.get(task) {
            Some(slot) if slot.owner == caller => Ok(slot),
            _ => Err(Error::TaskNotOwned { task: task.clone() }),
        }
    }

    /// The slot of a task `caller` owns or runs inside (its `sys.taskid`,
    /// which a `call` child of the task chain shares).
    fn visible_slot(&self, caller: ChainIndex, task: &TaskId) -> Result<&TaskSlot> {
        match self.tasks.get(task) {
            Some(slot) if slot.owner == caller || self.chains[caller.index()].task == *task => {
                Ok(slot)
            }
            _ => Err(Error::TaskNotOwned { task: task.clone() }),
        }
    }

    /// The live tasks `owner` owns in spawn order; an internal timer slot
    /// is never listed.
    fn live_tasks_of(&self, owner: ChainIndex) -> Vec<TaskId> {
        self.tasks
            .iter()
            .filter(|(_, slot)| slot.owner == owner && slot.state.is_live() && !slot.is_internal())
            .map(|(task, _)| task.clone())
            .collect()
    }

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
        match self.first_terminal(id, &tasks) {
            Ok(Some(task)) => {
                let delivery = self.deliver(&task);
                self.answer_inline(id, Answer::JoinAny(Ok(delivery)))
            }
            Ok(None) => {
                self.chains[id.index()].parked = Some(Parked::Tasks(tasks.into_vec()));
                self.park_wait(id);
                Ok(())
            }
            Err(error) => self.answer_inline(id, Answer::JoinAny(Err(error))),
        }
    }

    /// Validates a wait set and returns its first terminal member in set
    /// order, or `None` when every member is still running. An `Abandoned`
    /// member cannot pass the ownership check (its owner has ended), so
    /// the terminal arm only ever sees `Done` and `Cancelled`.
    fn first_terminal(&self, id: ChainIndex, tasks: &[TaskId]) -> Result<Option<TaskId>> {
        let mut first = None;
        for task in tasks {
            let slot = self.owned_slot(id, task)?;
            match slot.state {
                SlotState::Delivered { .. } => {
                    return Err(Error::TaskConsumed { task: task.clone() });
                }
                SlotState::Running => {}
                SlotState::Done(_) | SlotState::Cancelled | SlotState::Abandoned => {
                    if first.is_none() {
                        first = Some(task.clone());
                    }
                }
            }
        }
        Ok(first)
    }

    /// Takes a terminal slot's outcome as a delivery: a `Done` slot's
    /// outcome moves out and the slot to `Delivered`; a cancelled slot
    /// yields the `cancelled` error value and stays, having no result to
    /// consume. An abandoned slot has no live owner to wait on it, so its
    /// delivery is a scheduler bug. The delivery is the join: everything
    /// the task did happens before the owner's next step, so its store
    /// writes - a cancelled task's partial writes included - are visible
    /// from the very next operation.
    fn deliver(&mut self, task: &TaskId) -> TaskDelivery<Error> {
        if let Some(slot) = self.tasks.get(task) {
            let owner = slot.owner;
            self.join_task(owner, task);
        }
        let outcome = match self.tasks.get_mut(task) {
            Some(slot) => {
                let ok = matches!(slot.state, SlotState::Done(Ok(_)));
                // Every state but `Done` goes back as it was.
                match std::mem::replace(&mut slot.state, SlotState::Delivered { ok }) {
                    SlotState::Done(outcome) => outcome,
                    SlotState::Cancelled => {
                        slot.state = SlotState::Cancelled;
                        Err(Error::TaskCancelled { task: task.clone() })
                    }
                    SlotState::Abandoned => {
                        slot.state = SlotState::Abandoned;
                        Err(Error::internal(
                            "an abandoned task's owner ended, so no wait can deliver it",
                        ))
                    }
                    state @ (SlotState::Running | SlotState::Delivered { .. }) => {
                        slot.state = state;
                        Err(Error::internal(
                            "only a terminal, undelivered slot is delivered",
                        ))
                    }
                }
            }
            None => Err(Error::internal("a delivered task has a slot")),
        };
        TaskDelivery {
            task: task.clone(),
            outcome,
        }
    }

    /// Wakes `task`'s owner if it is parked on a set containing `task`:
    /// the member is delivered as the wait's answer and the owner leaves
    /// its wait.
    pub(super) fn wake_waiter(&mut self, owner: ChainIndex, task: &TaskId) -> Result<()> {
        if !self.chains[owner.index()].end_wait_for(task) {
            return Ok(());
        }
        let delivery = self.deliver(task);
        self.wake_from_wait(owner, Answer::JoinAny(Ok(delivery)))
    }

    /// Dispatches a `ready` request: whether a task the chain owns has
    /// ended, delivered or not.
    pub(super) fn dispatch_ready(&mut self, id: ChainIndex, task: &TaskId) -> Result<()> {
        let answer = self.owned_slot(id, task).map(|slot| !slot.state.is_live());
        self.answer_inline(id, Answer::Ready(answer))
    }

    /// Dispatches a `status` request over a task the chain owns or runs
    /// inside.
    pub(super) fn dispatch_status(&mut self, id: ChainIndex, task: &TaskId) -> Result<()> {
        let answer = self.task_status(id, task).map(Box::new);
        self.answer_inline(id, Answer::Status(answer))
    }

    /// Reads one task's status: the slot's facts and note, plus the backing
    /// chain's position while it is live (its section and what it is
    /// parked on) and the chain's counters, which the append-only arena
    /// keeps after the chain ends.
    fn task_status(&self, id: ChainIndex, task: &TaskId) -> Result<TaskStatus> {
        let slot = self.visible_slot(id, task)?;
        let state = slot.state.view();
        let mut status = TaskStatus {
            target: slot.target.clone(),
            state: task_phase(state),
            ok: state.ok(),
            section: None,
            blocked: None,
            turns: 0,
            tasks: Vec::new(),
            depth: 0,
            note: slot.note.clone(),
        };
        if let TaskBacking::Chain { chain: backing, .. } = slot.backing {
            let chain = &self.chains[backing.index()];
            status.turns = chain.ctx.turns().load(Ordering::Relaxed);
            status.depth = u32::try_from(chain.call_depth.get()).unwrap_or(u32::MAX);
            if slot.state.is_live() {
                status.section = chain
                    .entered
                    .is_some()
                    .then(|| chain.section_name().to_owned());
                status.blocked = chain.blocked();
                status.tasks = self.live_tasks_of(backing);
            }
        }
        Ok(status)
    }

    /// Dispatches a `pending` request: the chain's live tasks in spawn
    /// order.
    pub(super) fn dispatch_pending(&mut self, id: ChainIndex) -> Result<()> {
        let tasks = self.live_tasks_of(id);
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
            let parent_limit = match self.enclosing(id) {
                Some(parent) => self.chains[parent.index()].concurrency,
                None => self.ctx.limits().max_concurrency,
            };
            let limit = NonZeroUsize::try_from(limit).unwrap_or(NonZeroUsize::MAX);
            let effective = limit.min(parent_limit);
            self.chains[id.index()].concurrency = effective;
            effective
        } else {
            self.chains[id.index()].concurrency
        };
        self.answer_inline(id, Answer::Concurrency(Ok(effective.get())))
    }

    /// Dispatches a `note` request: the text becomes the latest note of the
    /// task the chain runs inside, recorded on the task's slot, so a `call`
    /// child's note is the task's. A note outside a slotted task (the main
    /// walk, task `0`, and its `call` children) records nothing.
    pub(super) fn dispatch_note(&mut self, id: ChainIndex, text: String) -> Result<()> {
        if let Some(slot) = self.tasks.get_mut(&self.chains[id.index()].task) {
            slot.note = Some(text);
        }
        self.answer_inline(id, Answer::Note(Ok(())))
    }

    /// Dispatches a `cancel` request over a task the chain owns.
    pub(super) fn dispatch_cancel(&mut self, id: ChainIndex, task: &TaskId) -> Result<()> {
        let answer = self.cancel_task(id, task);
        self.answer_inline(id, Answer::Cancel(answer))
    }

    /// Cancels a live task `caller` owns: the slot moves to `Cancelled`, the
    /// backing ends (a chain with everything it owns, a request dropped),
    /// and `TaskCancelled` fires once under the target - except for an
    /// internal timer, whose cancel is the wait shim's own bookkeeping and
    /// reports nothing, and a task that was never admitted, which has no
    /// start event and reports no terminal. A task already in a terminal
    /// state is left as it is.
    fn cancel_task(&mut self, caller: ChainIndex, task: &TaskId) -> Result<()> {
        let slot = self.owned_slot(caller, task)?;
        if !slot.state.is_live() {
            return Ok(());
        }
        let backing = slot.backing;
        let target = slot.target.clone();
        if let Some(slot) = self.tasks.get_mut(task) {
            slot.state = SlotState::Cancelled;
        }
        // The backing ends first, so anything it owned reports before the
        // task's own terminal event, which is the last word on it.
        let backing_chain = match backing {
            TaskBacking::Chain { chain, .. } => chain,
            TaskBacking::Effect(effect) => {
                self.abort_effect(effect);
                return Ok(());
            }
        };
        // The terminal is stamped with the cancelled task's own
        // provenance: its backing chain's emitter, taken before the abort
        // clears the chain's state. A task that was never admitted (still
        // queued for a slot) has no start event, so it reports no
        // terminal either.
        let admitted = self.chains[backing_chain.index()].kind.admitted();
        let emitter = Arc::clone(self.chains[backing_chain.index()].ctx.emitter());
        self.abort_subtree(backing_chain);
        if admitted {
            emitter.emit(&target, |execution, section, provenance| {
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
