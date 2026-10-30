//! How tasks end: a task chain's own end, and the chain-end rules that
//! end every task an ending chain owns.
//!
//! The chain-end rules: a task ends with its owner. When a chain ends,
//! every live task it owns is abandoned - its entry moves to `Abandoned`,
//! its terminal observation names how the owner ended, and its backing
//! chain aborts with everything it owns in turn. A task left live is the
//! author's bug, so the owner's own outcome becomes `tasks_live` naming the
//! leaked ids. Tasks survive a section's fall-through and a `jump` - those
//! move the walk within one chain - and end with the chain itself: the root
//! walk, a `call` child, or another task (a fanout arm among them). The H1
//! hand-off reassigns the pass's tasks to the walk (see the `h1` module).

use std::sync::Arc;

use runvil_types::event::Event;
use runvil_types::ids::{AbandonReason, Provenance, TaskId};
use runvil_vfs::Access;

use crate::{Error, Result};

use super::tasks::{Stop, TaskBacking};
use super::{ChainIndex, Scheduler};

impl Scheduler {
    /// Applies a task chain's end to its entry: the outcome lands in the
    /// entry, the entry moves to `Done`, the task's terminal observation
    /// fires under its target section, and an owner parked on a set
    /// containing the task is woken with it delivered. Otherwise the entry
    /// holds the outcome until a wait takes it.
    ///
    /// # Errors
    /// Returns [`Error::Internal`] when the chain has no live entry - every
    /// task chain registers its entry before it is enqueued, and a stopped
    /// task's chain is aborted before it can end - or when the woken owner
    /// has no suspended block.
    pub(super) fn complete_task(&mut self, id: ChainIndex, outcome: Result<String>) -> Result<()> {
        let chain = &self.chains[id];
        let task = chain.task.clone();
        // The terminal is the task's own last word, stamped with its task.
        let emitter = Arc::clone(chain.ctx.emitter());
        let succeeded = outcome.is_ok();
        let Some(entry) = self.tasks.complete(&task, outcome) else {
            return Err(Error::internal("a task chain's end implies its entry"));
        };
        let owner = entry.owner;
        emitter.emit(&entry.target, |execution, section, provenance| {
            let task = task.clone();
            if succeeded {
                Event::TaskSucceeded {
                    execution,
                    section,
                    provenance,
                    task,
                }
            } else {
                Event::TaskFailed {
                    execution,
                    section,
                    provenance,
                    task,
                }
            }
        });
        self.wake_waiter(owner, &task)
    }

    /// Applies the chain-end rules for tasks to `owner`'s `outcome`: every
    /// live task the chain owns is abandoned (the reason names how the
    /// owner ended: the section ended, the tool loop was exhausted, or the
    /// owner failed some other way), and an `Ok` outcome that leaked
    /// tasks becomes [`Error::TasksLive`] naming them in
    /// spawn order. A failing chain keeps its own error - the leak is the
    /// lesser fault - but its tasks end all the same.
    pub(super) fn settle_owned_tasks(
        &mut self,
        owner: ChainIndex,
        outcome: Result<String>,
    ) -> Result<String> {
        let reason = match &outcome {
            Ok(_) => AbandonReason::OwnerReturned,
            Err(Error::ToolLoopExhausted) => AbandonReason::ToolLoopExhausted,
            Err(_) => AbandonReason::OwnerFailed,
        };
        let leaked = self.abandon_owned_tasks(owner, reason);
        match outcome {
            Ok(_) if !leaked.is_empty() => Err(Error::TasksLive { tasks: leaked }),
            outcome => outcome,
        }
    }

    /// Joins `task` into `owner`: merges the task's final clock into the
    /// owner's access, so everything the task did happens before the
    /// owner's next step. A timer (no identity) and an owner already
    /// ended (no access) need no join.
    pub(super) fn join_task(&self, owner: ChainIndex, task: &TaskId) {
        let Some(exec) = self.tasks.exec_of(task) else {
            return;
        };
        let Ok(access) = self.chains[owner].access() else {
            return;
        };
        runvil_vfs::detail::access_join(access, exec);
    }

    /// Joins every task `owner` owns into `access`: the chain-end join,
    /// so the chain's own final clock transitively covers everything its
    /// tasks did even when no wait delivered them.
    pub(super) fn join_owned_tasks(&self, owner: ChainIndex, access: &Access) {
        for exec in self.tasks.execs_owned_by(owner) {
            runvil_vfs::detail::access_join(access, exec);
        }
    }

    /// Ends every live task `owner` owns because `owner` is ending: each
    /// entry moves to `Abandoned`, its backing chain aborts with everything
    /// it owns in turn (or its in-flight request is dropped), and its
    /// terminal observation fires under its target with `reason` - the
    /// observation is the reason's record, since no wait can reach an
    /// abandoned entry once its owner is gone. An internal timer's entry ends
    /// the same way but reports nothing and never counts as leaked: it is
    /// the wait's detail, not a task the author started. Returns the
    /// abandoned ids in spawn order, for the owner's `tasks_live` outcome;
    /// the caller discards them for an owner that is itself being aborted,
    /// whose outcome no one receives.
    pub(super) fn abandon_owned_tasks(
        &mut self,
        owner: ChainIndex,
        reason: AbandonReason,
    ) -> Vec<TaskId> {
        let mut leaked = Vec::new();
        for task in self.tasks.live_all(owner) {
            let Some((backing, target)) = self.tasks.stop(&task, Stop::Abandoned) else {
                continue;
            };
            let chain_backed =
                self.end_backing(backing, &target, |execution, section, provenance| {
                    Event::TaskAbandoned {
                        execution,
                        section,
                        provenance,
                        task: task.clone(),
                        reason,
                    }
                });
            if chain_backed {
                leaked.push(task);
            }
        }
        // The abandoned tasks' slots are free: the drain admits the
        // queued chains that now fit at its next edge.
        leaked
    }

    /// Ends the backing of a task the arena just stopped, the one path a
    /// cancel and an abandonment share: an internal timer's effect is
    /// aborted and reports nothing; a chain aborts with everything it owns,
    /// then `terminal` fires under `target`. The backing ends first, so
    /// anything it owned reports before the task's own terminal event,
    /// which is the last word on it. A task still queued for a slot is
    /// silent: it has no start event, so it reports no terminal, and the
    /// abort is all that happens. Returns whether the backing was a chain.
    pub(super) fn end_backing(
        &mut self,
        backing: TaskBacking,
        target: &str,
        terminal: impl FnOnce(String, String, Provenance) -> Event,
    ) -> bool {
        let chain = match backing {
            TaskBacking::Chain { chain, .. } => chain,
            TaskBacking::Effect(effect) => {
                self.abort_effect(effect);
                return false;
            }
        };
        // The terminal is stamped with the stopped task's own provenance:
        // its backing chain's emitter, taken before the abort clears the
        // chain's state. A task that was never admitted (still queued for
        // a slot) has no start event, so it reports no terminal either.
        let admitted = !self.admission.is_queued(chain);
        let emitter = Arc::clone(self.chains[chain].ctx.emitter());
        self.abort_subtree(chain);
        if admitted {
            emitter.emit(target, terminal);
        }
        true
    }

    /// Ends every live task in the arena because the run itself is ending:
    /// the whole-run counterpart of the per-owner chain-end rule, so a
    /// task stranded by a run cancel or a fatal answer still receives its
    /// one terminal before the run's end boundary. Each live entry's owner
    /// is passed to [`abandon_owned_tasks`](Self::abandon_owned_tasks)
    /// with `reason`, in ascending arena order. No entry reports twice:
    /// abandoning a task aborts its backing chain, and `abort_subtree`
    /// abandons that chain's own tasks (as `OwnerAborted`) on the way, so
    /// a nested entry is already terminal when its owner's turn comes and
    /// `is_live()` skips it. The leaked list is discarded: no one
    /// receives an outcome for a run that is ending.
    pub(super) fn settle_all_tasks(&mut self, reason: AbandonReason) {
        for owner in self.tasks.live_owners() {
            self.abandon_owned_tasks(owner, reason);
        }
    }

    /// Moves every task `from` owns to `to`: the H1 hand-off, so a task the
    /// pass spawned is waited on, inspected, cancelled, or leaked by the
    /// walk exactly as if the walk had spawned it. The pass's admission
    /// accounting moves with the owner links: the slots the pass's tasks
    /// hold count against the walk from here, under the limit the walk
    /// inherited from the pass.
    pub(super) fn reassign_tasks(&mut self, from: ChainIndex, to: ChainIndex) {
        self.tasks.reassign(from, to);
        // An effect-backed task's effect is awaited under its owner in the
        // effect table; the pass awaits no effect of its own at the
        // hand-off, so every effect-table entry under it is such a task's.
        self.effects.reassign(from, to);
    }
}
