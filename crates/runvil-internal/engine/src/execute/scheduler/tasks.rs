//! The task arena, with the `spawn` arm in the `spawn` child. A task is a
//! chain the scheduler runs beside its spawner instead of in place of it,
//! with an entry in the arena keyed by the chain's own hierarchical id.
//!
//! The entry outlives the chain. When the chain ends, its outcome lands in
//! the entry and the entry moves to `Done`; the owner later takes the
//! result through a wait, which moves it to `Delivered`. Cancellation and
//! abandonment (the owner ending first) are the two other terminal states,
//! kept apart because the log and `tasks.status` must tell "stopped on
//! purpose" from "lost its owner". A terminal entry is never removed: the
//! arena is append-only as the chain arena is, so a late `status` can
//! still report how a task ended.
//!
//! The [`TaskArena`] owns every entry and enforces the lifecycle: a task
//! moves only from `Running` to `Done`, `Cancelled`, or `Abandoned`, and
//! from `Done` to `Delivered`. It checks ownership, and a chain's view of
//! the task it runs inside, and it lists an owner's tasks in spawn order
//! two ways: the author-visible tasks, and every live task with the
//! internal timers included.

#[path = "tasks-spawn.rs"]
mod spawn;

use std::collections::BTreeMap;

use runvil_types::ids::TaskId;
use runvil_vfs::ExecId;

use crate::execute::run::EffectId;
use crate::{Error, Result};

use super::ChainIndex;

/// Where a task's work runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TaskBacking {
    /// A chain in the arena: every spawned task.
    Chain {
        /// The backing chain.
        chain: ChainIndex,
        /// The task's happens-before identity: the [`ExecId`] its access
        /// was spawned under, recorded at spawn so a delivery can join the
        /// task after its chain has dropped the access.
        exec: ExecId,
    },
    /// An in-flight leaf effect: the internal timer behind a wait's
    /// timeout, never author-visible, with no identity to join.
    Effect(EffectId),
}

/// One task's lifecycle state as its entry holds it, the outcome included.
#[derive(Debug)]
pub(super) enum EntryState {
    /// The backing chain or request is live.
    Running,
    /// The backing chain or request ended, and its outcome waits here: the
    /// backing chain's final text or failure, or an empty text for a fired
    /// timer, held from its end until the owner takes it.
    Done(Result<String>),
    /// The owner took the outcome through a wait. `ok` is the outcome's,
    /// kept so `status` still reports it after the wait took it.
    Delivered { ok: bool },
    /// The owner cancelled the task.
    Cancelled,
    /// The owner ended while the task was live, so the Engine ended it.
    Abandoned,
}

impl EntryState {
    /// True while the task's backing chain or request is still running:
    /// the one state a chain end must act on.
    pub(super) fn is_live(&self) -> bool {
        matches!(self, EntryState::Running)
    }

    /// The state without its outcome, as `status` and the suites read it.
    pub(super) fn view(&self) -> TaskState {
        match self {
            EntryState::Running => TaskState::Running,
            EntryState::Done(outcome) => TaskState::Done {
                ok: outcome.is_ok(),
            },
            EntryState::Delivered { ok } => TaskState::Delivered { ok: *ok },
            EntryState::Cancelled => TaskState::Cancelled,
            EntryState::Abandoned => TaskState::Abandoned,
        }
    }
}

/// One task's lifecycle state, with whether it ended well once it has
/// ended: the `Copy` view of an entry's [`EntryState`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TaskState {
    /// The backing chain or request is live.
    Running,
    /// The backing chain or request ended; its outcome waits in the entry.
    /// `ok` is whether it returned rather than failed, always `true` for a
    /// fired timer.
    Done { ok: bool },
    /// The owner took the outcome through a wait. Whether the task ended
    /// well, carried so `status` can report it after the outcome is gone.
    Delivered { ok: bool },
    /// The owner cancelled the task.
    Cancelled,
    /// The owner ended while the task was live, so the Engine ended it.
    Abandoned,
}

impl TaskState {
    /// Whether the task ended well, once it has ended: a cancelled or
    /// abandoned task did not.
    pub(super) fn ok(self) -> Option<bool> {
        match self {
            TaskState::Running => None,
            TaskState::Done { ok } | TaskState::Delivered { ok } => Some(ok),
            TaskState::Cancelled | TaskState::Abandoned => Some(false),
        }
    }
}

/// One task's entry in the arena.
#[derive(Debug)]
pub(super) struct TaskEntry {
    /// Where the task's work runs.
    pub(super) backing: TaskBacking,
    /// The chain that owns the task: its spawner, or the root walk for a
    /// task the H1 pass spawned, since the pass and the walk share the root
    /// lineage `0` as two arena records. The only chain allowed to wait on,
    /// inspect, or cancel the task, and the chain whose end ends it.
    /// Recorded only here: admission reads it as the task chain's
    /// enclosing chain.
    pub(super) owner: ChainIndex,
    /// The name of the section the task's chain started at, which its
    /// terminal observations report under; the placeholder `timer` for an
    /// internal timer, which reports none.
    pub(super) target: String,
    /// The task's lifecycle state, holding its outcome once it ends.
    pub(super) state: EntryState,
    /// The latest progress note published through `tasks.note` from inside
    /// the task, reported by `tasks.status`.
    pub(super) note: Option<String>,
}

impl TaskEntry {
    /// True for an entry the author never sees: the effect-backed timer
    /// behind a timed wait. Internal entries are omitted from `pending` and
    /// a status table's `tasks`, report no task observations, and never
    /// count as leaked.
    fn is_internal(&self) -> bool {
        matches!(self.backing, TaskBacking::Effect(_))
    }
}

/// How a live task ends before its own chain or request does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stop {
    /// The owner cancelled the task.
    Cancelled,
    /// The owner ended while the task was live.
    Abandoned,
}

/// The task arena: one entry per task the run has started, keyed by the
/// task's id. Entries leave the arena by shared reference only, so every
/// state change goes through the arena's rules.
#[derive(Default)]
pub(super) struct TaskArena {
    /// Ids order as paths, and one owner's tasks are its direct children,
    /// so one owner's entries iterate in spawn order.
    entries: BTreeMap<TaskId, TaskEntry>,
}

impl TaskArena {
    /// Registers a running task that `owner` started over `backing`,
    /// reporting under `target`.
    pub(super) fn insert(
        &mut self,
        task: TaskId,
        backing: TaskBacking,
        owner: ChainIndex,
        target: String,
    ) {
        let entry = TaskEntry {
            backing,
            owner,
            target,
            state: EntryState::Running,
            note: None,
        };
        self.entries.insert(task, entry);
    }

    /// The entry of a task `caller` owns, or [`Error::TaskNotOwned`] - for
    /// an unknown id too, so a caller learns nothing about tasks it never
    /// started.
    pub(super) fn owned(&self, caller: ChainIndex, task: &TaskId) -> Result<&TaskEntry> {
        match self.entries.get(task) {
            Some(entry) if entry.owner == caller => Ok(entry),
            _ => Err(Error::TaskNotOwned { task: task.clone() }),
        }
    }

    /// The entry of a task `caller` owns or runs inside: `caller_task` is
    /// the caller's `sys.taskid`, which a `call` child of the task chain
    /// shares.
    pub(super) fn visible(
        &self,
        caller: ChainIndex,
        caller_task: &TaskId,
        task: &TaskId,
    ) -> Result<&TaskEntry> {
        match self.entries.get(task) {
            Some(entry) if entry.owner == caller || caller_task == task => Ok(entry),
            _ => Err(Error::TaskNotOwned { task: task.clone() }),
        }
    }

    /// The live tasks `owner` owns that an author can see, in spawn order;
    /// an internal timer is never listed.
    pub(super) fn live_author_tasks(&self, owner: ChainIndex) -> Vec<TaskId> {
        self.live_entries(owner)
            .filter(|(_, entry)| !entry.is_internal())
            .map(|(task, _)| task.clone())
            .collect()
    }

    /// Every live task `owner` owns, internal timers included, in spawn
    /// order: the tasks the owner's end ends.
    pub(super) fn live_all(&self, owner: ChainIndex) -> Vec<TaskId> {
        self.live_entries(owner)
            .map(|(task, _)| task.clone())
            .collect()
    }

    fn live_entries(&self, owner: ChainIndex) -> impl Iterator<Item = (&TaskId, &TaskEntry)> {
        self.entries
            .iter()
            .filter(move |(_, entry)| entry.owner == owner && entry.state.is_live())
    }

    /// Validates a wait set and returns its first terminal member in set
    /// order, or `None` when every member is still running. Every member
    /// must be a task `caller` owns, and none may be delivered already. An
    /// `Abandoned` member cannot pass the ownership check (its owner has
    /// ended), so the terminal arm only ever sees `Done` and `Cancelled`.
    pub(super) fn first_terminal(
        &self,
        caller: ChainIndex,
        tasks: &[TaskId],
    ) -> Result<Option<TaskId>> {
        let mut first = None;
        for task in tasks {
            match self.owned(caller, task)?.state {
                EntryState::Delivered { .. } => {
                    return Err(Error::TaskConsumed { task: task.clone() });
                }
                EntryState::Running => {}
                EntryState::Done(_) | EntryState::Cancelled | EntryState::Abandoned => {
                    if first.is_none() {
                        first = Some(task.clone());
                    }
                }
            }
        }
        Ok(first)
    }

    /// Moves a running task to `Done`, holding `outcome`, and returns its
    /// entry. A task that isn't live, or has no entry, is left as it is and
    /// yields `None`.
    pub(super) fn complete(
        &mut self,
        task: &TaskId,
        outcome: Result<String>,
    ) -> Option<&TaskEntry> {
        let entry = self.entries.get_mut(task)?;
        if !entry.state.is_live() {
            return None;
        }
        entry.state = EntryState::Done(outcome);
        Some(entry)
    }

    /// Takes a terminal task's outcome as its delivery: a `Done` entry's
    /// outcome moves out and the entry to `Delivered`, so it is taken once.
    /// A cancelled entry yields the `cancelled` error value and stays,
    /// having no result to consume. An abandoned entry has no live owner to
    /// wait on it, so its delivery is a scheduler bug.
    pub(super) fn take_delivery(&mut self, task: &TaskId) -> Result<String> {
        let Some(entry) = self.entries.get_mut(task) else {
            return Err(Error::internal("a delivered task has an entry"));
        };
        let ok = matches!(entry.state, EntryState::Done(Ok(_)));
        // Every state but `Done` goes back as it was.
        match std::mem::replace(&mut entry.state, EntryState::Delivered { ok }) {
            EntryState::Done(outcome) => outcome,
            EntryState::Cancelled => {
                entry.state = EntryState::Cancelled;
                Err(Error::TaskCancelled { task: task.clone() })
            }
            EntryState::Abandoned => {
                entry.state = EntryState::Abandoned;
                Err(Error::internal(
                    "an abandoned task's owner ended, so no wait can deliver it",
                ))
            }
            state @ (EntryState::Running | EntryState::Delivered { .. }) => {
                entry.state = state;
                Err(Error::internal(
                    "only a terminal, undelivered entry is delivered",
                ))
            }
        }
    }

    /// Stops a running task: its entry moves to `Cancelled` or `Abandoned`,
    /// and its backing and target come back for the scheduler to end. A
    /// task that isn't live, or has no entry, is left as it is and yields
    /// `None`.
    pub(super) fn stop(&mut self, task: &TaskId, stop: Stop) -> Option<(TaskBacking, String)> {
        let entry = self.entries.get_mut(task)?;
        if !entry.state.is_live() {
            return None;
        }
        entry.state = match stop {
            Stop::Cancelled => EntryState::Cancelled,
            Stop::Abandoned => EntryState::Abandoned,
        };
        Some((entry.backing, entry.target.clone()))
    }

    /// The owners of every live task, internal timers included, each once,
    /// in ascending arena order.
    pub(super) fn live_owners(&self) -> Vec<ChainIndex> {
        let mut owners: Vec<ChainIndex> = self
            .entries
            .values()
            .filter(|entry| entry.state.is_live())
            .map(|entry| entry.owner)
            .collect();
        owners.sort_unstable_by_key(|owner| owner.index());
        owners.dedup();
        owners
    }

    /// Moves every task `from` owns to `to`.
    pub(super) fn reassign(&mut self, from: ChainIndex, to: ChainIndex) {
        for entry in self.entries.values_mut() {
            if entry.owner == from {
                entry.owner = to;
            }
        }
    }

    /// Records `text` as `task`'s latest note. A task with no entry records
    /// nothing.
    pub(super) fn set_note(&mut self, task: &TaskId, text: String) {
        if let Some(entry) = self.entries.get_mut(task) {
            entry.note = Some(text);
        }
    }

    /// The happens-before identity of every task `owner` owns, ended or
    /// not; an internal timer has none.
    pub(super) fn execs_owned_by(&self, owner: ChainIndex) -> impl Iterator<Item = ExecId> + '_ {
        self.entries
            .values()
            .filter(move |entry| entry.owner == owner)
            .filter_map(|entry| match entry.backing {
                TaskBacking::Chain { exec, .. } => Some(exec),
                TaskBacking::Effect(_) => None,
            })
    }

    /// `task`'s happens-before identity, or `None` for an internal timer
    /// and for an id with no entry.
    pub(super) fn exec_of(&self, task: &TaskId) -> Option<ExecId> {
        match self.entries.get(task)?.backing {
            TaskBacking::Chain { exec, .. } => Some(exec),
            TaskBacking::Effect(_) => None,
        }
    }

    /// The chain that owns `task`.
    pub(super) fn owner_of(&self, task: &TaskId) -> Option<ChainIndex> {
        self.entries.get(task).map(|entry| entry.owner)
    }

    /// The section `task`'s observations report under.
    pub(super) fn target_of(&self, task: &TaskId) -> Option<&str> {
        self.entries.get(task).map(|entry| entry.target.as_str())
    }

    /// `task`'s state, or `None` when no task with that id was started.
    #[cfg(test)]
    pub(super) fn state_of(&self, task: &TaskId) -> Option<TaskState> {
        self.entries.get(task).map(|entry| entry.state.view())
    }
}

#[cfg(test)]
#[path = "tasks-tests.rs"]
mod tests;
