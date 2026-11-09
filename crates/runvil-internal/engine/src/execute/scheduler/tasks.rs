//! The task arena and the `spawn` arm. A task is a chain the scheduler
//! runs beside its spawner instead of in place of it, with an entry in the
//! arena keyed by the chain's own hierarchical id. `tasks.spawn` (and the
//! `fanout` shim, once per arm) resumes the spawner at once with the id;
//! the child waits in the admission queue, holding no Lua VM, until the
//! run's concurrency limits admit it.
//!
//! The entry outlives the chain, and it holds two things apart: the
//! task's state, whether its backing chain is live and how it ended, and
//! its outcome, the result the owner's waits take. When the chain ends,
//! its outcome lands in the entry and the entry moves to `Ended`. A live
//! task publishes a result through `tasks.reply`, and idles on its inbox
//! until its owner's next message runs it again. The owner takes each
//! result once, through a wait, which marks the outcome delivered.
//! Cancellation and abandonment (the owner ending first) are the two other
//! ways a task ends, kept apart because the log and `tasks.status` must
//! tell "stopped on purpose" from "lost its owner". An ended entry is never
//! removed: the arena is append-only as the chain arena is, so a late
//! `status` can still report how a task ended.
//!
//! The [`TaskArena`] owns every entry and enforces the lifecycle: a live
//! task moves between `Running` and `Idle`, and from either to `Canceled`
//! or `Abandoned`; a running task ends on its own, and an idle one when the
//! idle cap closes it, both to `Ended`. Its outcome moves from unpublished
//! or delivered to waiting as the task publishes a result, and from
//! waiting to delivered as a wait takes it. The arena checks ownership,
//! and a chain's view of the task it runs inside, and it lists an owner's
//! tasks in spawn order two ways: the author-visible tasks, and every live
//! task with the internal timers included.

use std::collections::{BTreeMap, BTreeSet};
use std::num::{NonZeroU32, NonZeroUsize};
use std::sync::Arc;
use std::sync::atomic::AtomicU32;

use runvil_types::ids::TaskId;
use runvil_vfs::ExecId;

use crate::execute::run::EffectId;
use crate::execute::section_context::TaskSeed;
use crate::lua::Answer;
use crate::{Result, error};

use super::admission::SpawnRecord;
use super::chain::ChainStart;
use super::chain_record::{ChainKind, Parked};
use super::{ChainIndex, Counters, Scheduler, prompt_origin};

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

/// One task's lifecycle state as its entry holds it: whether its backing
/// chain or request is live, and how it ended once it has. The result a
/// wait takes is the entry's [`Outcome`], held apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EntryState {
    /// The backing chain or request is live and running.
    Running,
    /// The backing chain is live and idle: it replied through
    /// `tasks.reply` and waits on the task's inbox for its owner's next
    /// message. `since` orders the run's idle tasks by when each went
    /// idle, so the idle cap can tell which one is idle longest.
    Idle { since: u64 },
    /// The backing chain or request ended and left its outcome: the chain
    /// returned or failed, a timer fired, or the idle cap closed an idle
    /// task, whose latest reply stays its result.
    Ended,
    /// The owner canceled the task.
    Canceled,
    /// The owner ended while the task was live, so the Engine ended it.
    Abandoned,
}

impl EntryState {
    /// True while the task's backing chain or request is still live,
    /// running or idle: the states a chain end must act on.
    pub(super) fn is_live(&self) -> bool {
        matches!(self, EntryState::Running | EntryState::Idle { .. })
    }
}

/// The result a task leaves for its owner's waits, held apart from whether
/// the task still runs.
#[derive(Debug)]
pub(super) enum Outcome {
    /// The task has published no result: it has neither replied nor
    /// ended.
    Unpublished,
    /// A result no wait has taken: the task's latest reply, its backing
    /// chain's final text or failure, or an empty text for a fired timer.
    Waiting(Result<String>),
    /// A wait took the task's latest result. `ok` is the result's, kept so
    /// `status` still reports it after the wait took it.
    Delivered { ok: bool },
}

/// One task's lifecycle state, with whether it ended well once it has
/// ended: the `Copy` view of an entry's [`EntryState`] and [`Outcome`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TaskState {
    /// The backing chain or request is live and running.
    Running,
    /// The backing chain is live and idle after a reply, waiting on its
    /// inbox.
    Idle,
    /// The backing chain or request ended; its outcome waits in the entry.
    /// `ok` is whether it returned rather than failed, always `true` for a
    /// fired timer and for a task the idle cap closed.
    Done { ok: bool },
    /// The owner took the outcome through a wait. Whether the task ended
    /// well, carried so `status` can report it after the outcome is gone.
    Delivered { ok: bool },
    /// The owner canceled the task.
    Canceled,
    /// The owner ended while the task was live, so the Engine ended it.
    Abandoned,
}

/// One task's entry in the arena.
#[derive(Debug)]
pub(super) struct TaskEntry {
    /// Where the task's work runs.
    pub(super) backing: TaskBacking,
    /// The chain that owns the task: its spawner, or the root walk for a
    /// task the H1 pass spawned. The only chain allowed to wait on,
    /// inspect, or cancel the task, and the chain whose end ends it.
    /// Recorded only here: admission reads it as the task chain's
    /// enclosing chain.
    pub(super) owner: ChainIndex,
    /// The name of the section the task's chain started at, which its
    /// terminal observations report under; the placeholder `timer` for an
    /// internal timer, which reports none.
    pub(super) target: String,
    /// The task's lifecycle state.
    pub(super) state: EntryState,
    /// The result the task left for its owner's waits.
    pub(super) outcome: Outcome,
    /// The latest progress note published through `tasks.note` from inside
    /// the task, reported by `tasks.status`.
    pub(super) note: Option<String>,
    /// The prompt line of the author's call that spawned the task, which a
    /// `make_tasks_live_error` failure that names it first is placed on: `None` for
    /// an internal timer, and for a spawn whose yield found no author
    /// frame.
    pub(super) spawn_line: Option<NonZeroU32>,
}

impl TaskEntry {
    /// True for an entry the author never sees: the effect-backed timer
    /// behind a timed wait. Internal entries are omitted from `pending` and
    /// a status table's `tasks`, report no task observations, and never
    /// count as leaked.
    fn is_internal(&self) -> bool {
        matches!(self.backing, TaskBacking::Effect(_))
    }

    /// True while the task is idle after a reply.
    pub(super) fn is_idle(&self) -> bool {
        matches!(self.state, EntryState::Idle { .. })
    }

    /// What `tasks.ready` reports: true once the task has ended, delivered
    /// or not, and while a result it published waits undelivered, so a
    /// wait would return at once.
    pub(super) fn is_ready(&self) -> bool {
        !self.state.is_live() || matches!(self.outcome, Outcome::Waiting(_))
    }

    /// The state with whether the task ended well, as `status` and the
    /// suites read it.
    pub(super) fn view(&self) -> TaskState {
        match self.state {
            EntryState::Running => TaskState::Running,
            EntryState::Idle { .. } => TaskState::Idle,
            EntryState::Canceled => TaskState::Canceled,
            EntryState::Abandoned => TaskState::Abandoned,
            EntryState::Ended => match &self.outcome {
                Outcome::Waiting(outcome) => TaskState::Done {
                    ok: outcome.is_ok(),
                },
                Outcome::Delivered { ok } => TaskState::Delivered { ok: *ok },
                // Every path to `Ended` leaves the task an outcome.
                Outcome::Unpublished => TaskState::Done { ok: false },
            },
        }
    }
}

/// How a live task ends before its own chain or request does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stop {
    /// The owner canceled the task.
    Canceled,
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
    /// The `since` stamp the next task to go idle takes.
    next_idle: u64,
}

impl TaskArena {
    /// Registers a running task that `owner` started over `backing`,
    /// reporting under `target`, by the call on `spawn_line`.
    pub(super) fn insert(
        &mut self,
        task: TaskId,
        backing: TaskBacking,
        owner: ChainIndex,
        target: String,
        spawn_line: Option<NonZeroU32>,
    ) {
        let entry = TaskEntry {
            backing,
            owner,
            target,
            state: EntryState::Running,
            outcome: Outcome::Unpublished,
            note: None,
            spawn_line,
        };
        self.entries.insert(task, entry);
    }

    /// The entry of a task `caller` owns, or a `TaskNotOwned` failure - for
    /// an unknown id too, so a caller learns nothing about tasks it never
    /// started.
    pub(super) fn owned(&self, caller: ChainIndex, task: &TaskId) -> Result<&TaskEntry> {
        match self.entries.get(task) {
            Some(entry) if entry.owner == caller => Ok(entry),
            _ => Err(error::make_task_not_owned_error(task)),
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
            _ => Err(error::make_task_not_owned_error(task)),
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

    /// Validates a wait set and returns its first member in set order that
    /// a wait takes at once, or `None` when no member has a result for it
    /// yet. Every member must be a task `caller` owns, and none may be
    /// consumed: a wait took its latest result, and it has ended or idles
    /// with no message since, so no other result will come. A member is
    /// taken at once when a result it published waits undelivered, or when
    /// it was canceled. An `Abandoned` member cannot pass the ownership
    /// check, since its owner has ended.
    pub(super) fn first_deliverable(
        &self,
        caller: ChainIndex,
        tasks: &[TaskId],
    ) -> Result<Option<TaskId>> {
        let mut first = None;
        for task in tasks {
            let entry = self.owned(caller, task)?;
            match (entry.state, &entry.outcome) {
                (EntryState::Ended | EntryState::Idle { .. }, Outcome::Delivered { .. }) => {
                    return Err(error::make_task_consumed_error(task));
                }
                (EntryState::Canceled, _) | (_, Outcome::Waiting(_)) if first.is_none() => {
                    first = Some(task.clone());
                }
                _ => {}
            }
        }
        Ok(first)
    }

    /// Moves a live task to `Ended`, holding `outcome`, and returns its
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
        entry.state = EntryState::Ended;
        entry.outcome = Outcome::Waiting(outcome);
        Some(entry)
    }

    /// Publishes `text` as a running task's latest result, in place of one
    /// no wait took, and returns its entry. A task that isn't running, or
    /// has no entry, is left as it is and yields `None`.
    pub(super) fn reply(&mut self, task: &TaskId, text: String) -> Option<&TaskEntry> {
        let entry = self.entries.get_mut(task)?;
        if entry.state != EntryState::Running {
            return None;
        }
        entry.outcome = Outcome::Waiting(Ok(text));
        Some(entry)
    }

    /// Moves a running task to `Idle`, idle since after every task that
    /// went idle before it. Any other task is left as it is.
    pub(super) fn idle(&mut self, task: &TaskId) {
        let Some(entry) = self.entries.get_mut(task) else {
            return;
        };
        if entry.state == EntryState::Running {
            entry.state = EntryState::Idle {
                since: self.next_idle,
            };
            self.next_idle += 1;
        }
    }

    /// Moves an idle task back to `Running`, as its owner's message
    /// resumes its reply, and reports whether it was idle. Any other task
    /// is left as it is.
    pub(super) fn wake(&mut self, task: &TaskId) -> bool {
        match self.entries.get_mut(task) {
            Some(entry) if entry.is_idle() => {
                entry.state = EntryState::Running;
                true
            }
            _ => false,
        }
    }

    /// The task the idle cap closes once more than `cap` tasks are idle:
    /// among the idle tasks other than `except`, the one idle longest that
    /// owns no live task, or the one idle longest when each of them owns
    /// one. `task_of` names the task an owner chain runs inside, so a task
    /// owns what its `call` children spawned too. `None` while the run is
    /// within the cap.
    pub(super) fn idle_longest_past(
        &self,
        cap: NonZeroUsize,
        except: &TaskId,
        task_of: impl Fn(ChainIndex) -> TaskId,
    ) -> Option<TaskId> {
        let mut idle: Vec<(u64, &TaskId)> = self
            .entries
            .iter()
            .filter_map(|(task, entry)| match entry.state {
                EntryState::Idle { since } => Some((since, task)),
                _ => None,
            })
            .collect();
        if idle.len() <= cap.get() {
            return None;
        }
        idle.retain(|(_, task)| *task != except);
        idle.sort_unstable_by_key(|(since, _)| *since);
        let owning: BTreeSet<TaskId> = self.live_owners().into_iter().map(task_of).collect();
        idle.iter()
            .find(|(_, task)| !owning.contains(*task))
            .or_else(|| idle.first())
            .map(|(_, task)| (*task).clone())
    }

    /// Closes an idle task for the idle cap: its entry moves to `Ended`
    /// with its outcome as it is, so its latest reply is its result as if
    /// it had returned it, and its backing and target come back for the
    /// scheduler to end. A task that isn't idle, or has no entry, is left
    /// as it is and yields `None`.
    pub(super) fn close(&mut self, task: &TaskId) -> Option<(TaskBacking, String)> {
        let entry = self.entries.get_mut(task)?;
        if !entry.is_idle() {
            return None;
        }
        entry.state = EntryState::Ended;
        Some((entry.backing, entry.target.clone()))
    }

    /// Takes a task's waiting outcome as its delivery: the outcome moves out
    /// and is marked delivered, so each result is taken once, and the
    /// task's state stays as it is, so a live task stays live. A canceled
    /// entry yields the `canceled` error value and stays, having no result
    /// to consume. An abandoned entry has no live owner to wait on it, so
    /// its delivery is a scheduler bug.
    pub(super) fn take_delivery(&mut self, task: &TaskId) -> Result<String> {
        let Some(entry) = self.entries.get_mut(task) else {
            return Err(error::make_internal_error("a delivered task has an entry"));
        };
        match entry.state {
            EntryState::Canceled => return Err(error::make_task_canceled_error(task)),
            EntryState::Abandoned => {
                return Err(error::make_internal_error(
                    "an abandoned task's owner ended, so no wait can deliver it",
                ));
            }
            EntryState::Running | EntryState::Idle { .. } | EntryState::Ended => {}
        }
        let Outcome::Waiting(outcome) = &mut entry.outcome else {
            return Err(error::make_internal_error(
                "only a published, undelivered outcome is delivered",
            ));
        };
        let outcome = std::mem::replace(outcome, Ok(String::new()));
        entry.outcome = Outcome::Delivered {
            ok: outcome.is_ok(),
        };
        outcome
    }

    /// Stops a live task: its entry moves to `Canceled` or `Abandoned`, a
    /// result it published is dropped, and its backing and target come
    /// back for the scheduler to end. A task that isn't live, or has no
    /// entry, is left as it is and yields `None`.
    pub(super) fn stop(&mut self, task: &TaskId, stop: Stop) -> Option<(TaskBacking, String)> {
        let entry = self.entries.get_mut(task)?;
        if !entry.state.is_live() {
            return None;
        }
        entry.state = match stop {
            Stop::Canceled => EntryState::Canceled,
            Stop::Abandoned => EntryState::Abandoned,
        };
        entry.outcome = Outcome::Unpublished;
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

    /// The prompt line of the call that spawned `task`, when known.
    pub(super) fn spawn_line_of(&self, task: &TaskId) -> Option<NonZeroU32> {
        self.entries.get(task)?.spawn_line
    }

    /// `task`'s state, or `None` when no task with that id was started.
    #[cfg(test)]
    pub(super) fn state_of(&self, task: &TaskId) -> Option<TaskState> {
        self.entries.get(task).map(TaskEntry::view)
    }
}

impl Scheduler {
    /// Dispatches a `spawn` request: constructs the task's chain, registers
    /// its entry, and resumes the spawner with the task's id; the child is
    /// enqueued behind the spawner, so `spawn` returns before the child
    /// runs. Its preparation fails on the depth cap, target resolution,
    /// the worker check, or chain construction.
    pub(in crate::execute::scheduler) fn dispatch_spawn(
        &mut self,
        id: ChainIndex,
        target: &str,
        input: Option<&str>,
        seed: TaskSeed,
        var: serde_json::Value,
    ) -> Result<()> {
        match self.prepare_spawn(id, target, input, seed, var) {
            Ok((task, child, record)) => {
                // The child waits for admission, queued behind the
                // spawner; the drain admits it when a slot frees up at
                // its owner and every ancestor.
                self.admission.enqueue_spawn(child, record);
                self.answer_inline(id, Ok(Answer::Spawn(task)))
            }
            Err(error) => self.answer_inline(id, Err(error)),
        }
    }

    /// The fallible half of spawn dispatch: `call`'s depth cap against the
    /// spawner's call-depth field, `call`'s target resolution over the
    /// spawner's visible set, the worker-template check (a list section is
    /// not a target), then the task chain one level deeper under the
    /// spawn's `args` and `var` snapshot, with its own access capability (a
    /// concurrent thread of execution, forked from the spawner's so the
    /// spawn is the happens-before edge; every delivery of the task joins
    /// it back) and a fresh turn counter. The task's entry keeps the
    /// spawner's call line, the prompt line it spawned from. The child
    /// leaves this call holding no Lua VM yet, with its `var` snapshot
    /// moved into the chain, and its spawn record comes back for the
    /// admission queue; the drain admits it when an admission slot frees
    /// up.
    fn prepare_spawn(
        &mut self,
        id: ChainIndex,
        target: &str,
        input: Option<&str>,
        seed: TaskSeed,
        var: serde_json::Value,
    ) -> Result<(TaskId, ChainIndex, SpawnRecord)> {
        let chain = &self.chains[id];
        let depth = chain.call_depth.deeper()?;
        // The spawning section, captured now: the start event reports it
        // at admission, which may come after the spawner's walk moved to
        // another section.
        let spawner_section = chain.section_name().to_owned();
        let spawn_line = chain.frame()?.vm().call_line();
        // An explicit input forks the chain's args (and `argv` re-derives
        // from them), as a `call` with input does; otherwise the chain
        // inherits the spawner's context whole, but for the turn counter,
        // which `with_task` replaces below.
        let child_ctx = match input {
            Some(input) => chain.ctx.with_args(input),
            None => chain.ctx.clone(),
        };
        // A spawned chain's effective limit starts with its spawner's:
        // its tasks run within the spawner's share and every ancestor's.
        let child_concurrency = chain.concurrency;
        // The task starts on a copy of its spawner's current model, taken
        // now, at dispatch: a queued task holds no VM until it is admitted,
        // so a copy taken then would see a later switch.
        let model = chain.current_model()?;
        let spawner_access = Arc::clone(chain.access()?);
        // `chain`'s arena borrow ends here; the resolution names the
        // target's slice by path, resolved against the shared tree.
        let prompt = self.prompt();
        let resolved = self.with_chain(id, |ops| ops.resolve_chain_target(target))?;
        let worker = resolved
            .pos
            .section(&prompt)
            .ok_or(error::make_internal_error(
                "a resolved worker is absent from its slice",
            ))?;
        if worker.is_list() {
            return Err(error::make_lua_runtime_error(format!(
                "section `{}` is a list section, not a worker template",
                worker.name()
            )));
        }
        // The access spawns before the chain exists: a store refusal here
        // is the last fallible step that can leave nothing behind, so it
        // runs ahead of the id allocation and the arena push rather than
        // orphaning a started chain that is neither enqueued nor entered
        // in the arena.
        let access = self
            .run_scope()?
            .spawn(
                &spawner_access,
                prompt_origin(&prompt, worker.name(), worker.blocks()),
            )
            .map_err(|cause| error::make_store_error(&cause))?;
        // The task's happens-before identity, recorded before the access
        // moves into the chain: every delivery joins it, and the chain
        // end joins it last.
        let exec = access.id();
        // The task's id is the spawner's next child id; the task is its
        // chain, named from the other side.
        let lineage = self.chains[id].next_child_id()?;
        let task = TaskId::from(lineage.clone());
        // The task's context reports under its own task id with its own
        // turn counter, so its events report its provenance and its turn
        // numbers, and `tasks.status`'s `turns`, count only its own rounds.
        let child_ctx = child_ctx.with_task(task.clone(), Arc::new(AtomicU32::new(0)));
        let child = self.start_chain(ChainStart {
            lineage,
            counters: Counters::default(),
            ctx: child_ctx,
            model,
            access: Arc::new(access),
            start: resolved.pos,
            kind: ChainKind::Task,
            var,
            call_depth: depth,
            concurrency: child_concurrency,
        })?;
        let spawned = &mut self.chains[child];
        spawned.seed = seed;
        // `blocked` reads `queued` in `tasks.status` while the entry's
        // state is still `Running`.
        spawned.parked = Some(Parked::Queued);
        self.tasks.insert(
            task.clone(),
            TaskBacking::Chain { chain: child, exec },
            id,
            worker.name().to_owned(),
            spawn_line,
        );
        // The task waits for admission from here, and the spawn record
        // holds what the start event reports once the task first runs.
        let record = SpawnRecord {
            input: input.map(str::to_owned),
            section: spawner_section,
        };
        Ok((task, child, record))
    }
}

#[cfg(test)]
mod tests {
    //! Tests for the task arena alone, one rule each: a stop ends only a live
    //! task, a completed task's outcome is delivered once, a reply's outcome
    //! is delivered once while its task stays live, a wait on an idle task
    //! whose reply was taken is refused, the idle cap names the task idle
    //! longest that owns no live task or else the one idle longest, a close
    //! keeps an idle task's reply as its result, entries are never removed,
    //! `owned` admits only the owner, `visible` adds the chain running inside
    //! the task, and `live_author_tasks` lists an owner's live tasks in spawn
    //! order without internal timers.

    use std::num::NonZeroUsize;

    use runvil_types::error::ErrorCode;
    use runvil_types::ids::{ChainId, TaskId};
    use runvil_vfs::{Origin, VfsRef};

    use super::super::ChainIndex;
    use super::{Stop, TaskArena, TaskBacking, TaskState};
    use crate::execute::run::EffectId;
    use crate::{Error, error};

    fn chain(raw: u32) -> ChainIndex {
        ChainIndex::for_test(raw)
    }

    /// Child task `index` of the root walk.
    fn task(index: u32) -> TaskId {
        TaskId::from(ChainId::root().child(index))
    }

    /// A backing chain at arena index `raw` under a fresh identity.
    fn chain_backing(raw: u32) -> TaskBacking {
        let access = VfsRef::default()
            .acquire(Origin::new("tasks-test"))
            .expect("the default backend acquires");
        TaskBacking::Chain {
            chain: chain(raw),
            exec: access.id(),
        }
    }

    /// Registers child task `index` of the root walk, owned by `owner` and
    /// backed by the chain at arena index `100 + index`.
    fn spawn(arena: &mut TaskArena, owner: ChainIndex, index: u32) -> (TaskId, TaskBacking) {
        let backing = chain_backing(100 + index);
        arena.insert(task(index), backing, owner, format!("worker {index}"), None);
        (task(index), backing)
    }

    /// The task a test chain runs inside: the chain at arena index
    /// `100 + index` backs child task `index` of the root walk, as `spawn`
    /// registers it, and every other chain runs the root walk.
    fn task_of(owner: ChainIndex) -> TaskId {
        match owner.index().checked_sub(100) {
            Some(index) => task(u32::try_from(index).expect("a test index fits u32")),
            None => TaskId::from(ChainId::root()),
        }
    }

    /// Registers child task `index` of the root walk as an internal timer
    /// owned by `owner`.
    fn timer(arena: &mut TaskArena, owner: ChainIndex, index: u32) -> TaskId {
        let backing = TaskBacking::Effect(EffectId(u64::from(index)));
        arena.insert(task(index), backing, owner, "timer".to_owned(), None);
        task(index)
    }

    #[test]
    fn stopping_a_running_task_ends_it_and_returns_its_backing_and_target() {
        let mut arena = TaskArena::default();
        let (canceled, canceled_backing) = spawn(&mut arena, chain(0), 0);
        let (abandoned, abandoned_backing) = spawn(&mut arena, chain(0), 1);
        assert_eq!(
            arena.stop(&canceled, Stop::Canceled),
            Some((canceled_backing, "worker 0".to_owned()))
        );
        assert_eq!(
            arena.stop(&abandoned, Stop::Abandoned),
            Some((abandoned_backing, "worker 1".to_owned()))
        );
        assert_eq!(arena.state_of(&canceled), Some(TaskState::Canceled));
        assert_eq!(arena.state_of(&abandoned), Some(TaskState::Abandoned));
    }

    #[test]
    fn stopping_a_task_that_is_not_live_returns_none_and_changes_nothing() {
        let mut arena = TaskArena::default();
        let (done, _) = spawn(&mut arena, chain(0), 0);
        let (canceled, _) = spawn(&mut arena, chain(0), 1);
        assert!(arena.complete(&done, Ok("text".to_owned())).is_some());
        assert!(arena.stop(&canceled, Stop::Canceled).is_some());
        for stop in [Stop::Canceled, Stop::Abandoned] {
            assert_eq!(arena.stop(&done, stop), None);
            assert_eq!(arena.stop(&canceled, stop), None);
            assert_eq!(arena.stop(&task(9), stop), None, "task 9 has no entry");
        }
        assert_eq!(arena.state_of(&done), Some(TaskState::Done { ok: true }));
        assert_eq!(arena.state_of(&canceled), Some(TaskState::Canceled));
        assert_eq!(arena.state_of(&task(9)), None);
    }

    #[test]
    fn a_completed_task_is_done_and_its_outcome_is_taken_once() {
        let mut arena = TaskArena::default();
        let (task, _) = spawn(&mut arena, chain(0), 0);
        let completed = arena
            .complete(&task, Ok("text".to_owned()))
            .expect("a running task completes");
        assert_eq!(completed.owner, chain(0));
        assert_eq!(arena.state_of(&task), Some(TaskState::Done { ok: true }));
        assert_eq!(arena.take_delivery(&task).expect("the outcome"), "text");
        assert_eq!(
            arena.state_of(&task),
            Some(TaskState::Delivered { ok: true })
        );
        assert!(matches!(
            arena.take_delivery(&task),
            Err(error) if error.code == ErrorCode::Internal
        ));
        assert_eq!(
            arena.state_of(&task),
            Some(TaskState::Delivered { ok: true })
        );
    }

    #[test]
    fn a_reply_is_delivered_once_and_its_task_stays_live() {
        let mut arena = TaskArena::default();
        let (task, _) = spawn(&mut arena, chain(0), 0);
        assert_eq!(
            arena
                .first_deliverable(chain(0), std::slice::from_ref(&task))
                .ok(),
            Some(None)
        );
        let replied = arena
            .reply(&task, "draft".to_owned())
            .expect("a running task replies");
        assert_eq!(replied.owner, chain(0));
        assert!(replied.is_ready(), "the reply waits undelivered");
        assert_eq!(
            arena
                .first_deliverable(chain(0), std::slice::from_ref(&task))
                .ok(),
            Some(Some(task.clone()))
        );
        assert_eq!(arena.take_delivery(&task).expect("the reply"), "draft");
        assert_eq!(arena.state_of(&task), Some(TaskState::Running));
        assert_eq!(
            arena
                .first_deliverable(chain(0), std::slice::from_ref(&task))
                .ok(),
            Some(None),
            "a running task whose reply was taken has its next result to come"
        );
        assert!(arena.complete(&task, Ok("final".to_owned())).is_some());
        assert_eq!(arena.take_delivery(&task).expect("the final text"), "final");
    }

    #[test]
    fn a_wait_on_an_idle_task_whose_reply_was_taken_is_consumed_until_it_wakes() {
        let mut arena = TaskArena::default();
        let (task, _) = spawn(&mut arena, chain(0), 0);
        assert!(arena.reply(&task, "draft".to_owned()).is_some());
        arena.idle(&task);
        assert_eq!(arena.state_of(&task), Some(TaskState::Idle));
        assert!(
            arena.reply(&task, "again".to_owned()).is_none(),
            "an idle task cannot reply"
        );
        assert_eq!(arena.take_delivery(&task).expect("the reply"), "draft");
        assert!(matches!(
            arena.first_deliverable(chain(0), std::slice::from_ref(&task)),
            Err(refused) if refused.code == ErrorCode::TaskConsumed
        ));
        assert!(arena.wake(&task), "the owner's message wakes it");
        assert!(!arena.wake(&task), "a running task is not idle");
        assert_eq!(arena.state_of(&task), Some(TaskState::Running));
        assert_eq!(arena.first_deliverable(chain(0), &[task]).ok(), Some(None));
    }

    #[test]
    fn the_idle_cap_names_the_task_idle_longest_other_than_the_one_replying() {
        let mut arena = TaskArena::default();
        let (first, _) = spawn(&mut arena, chain(0), 0);
        let (second, _) = spawn(&mut arena, chain(0), 1);
        let (third, _) = spawn(&mut arena, chain(0), 2);
        for task in [&second, &first, &third] {
            assert!(arena.reply(task, "reply".to_owned()).is_some());
            arena.idle(task);
        }
        let cap = |n| NonZeroUsize::new(n).expect("the cap is non-zero");
        assert_eq!(
            arena.idle_longest_past(cap(3), &third, task_of),
            None,
            "within the cap"
        );
        assert_eq!(
            arena.idle_longest_past(cap(2), &third, task_of),
            Some(second.clone()),
            "the second went idle first"
        );
        assert_eq!(
            arena.idle_longest_past(cap(2), &second, task_of),
            Some(first),
            "the one replying is never the one closed"
        );
    }

    #[test]
    fn the_idle_cap_passes_over_a_task_that_owns_a_live_task_unless_each_one_does() {
        let mut arena = TaskArena::default();
        let (first, _) = spawn(&mut arena, chain(0), 0);
        let (second, _) = spawn(&mut arena, chain(0), 1);
        let (third, _) = spawn(&mut arena, chain(0), 2);
        for task in [&first, &second, &third] {
            assert!(arena.reply(task, "reply".to_owned()).is_some());
            arena.idle(task);
        }
        let worker = |index: u32| TaskId::from(ChainId::root().child(index).child(0));
        let cap = NonZeroUsize::new(2).expect("the cap is non-zero");
        arena.insert(
            worker(0),
            chain_backing(200),
            chain(100),
            "w".to_owned(),
            None,
        );
        assert_eq!(
            arena.idle_longest_past(cap, &third, task_of),
            Some(second.clone()),
            "the first owns a live worker"
        );
        arena.insert(
            worker(1),
            chain_backing(201),
            chain(101),
            "w".to_owned(),
            None,
        );
        assert_eq!(
            arena.idle_longest_past(cap, &third, task_of),
            Some(first.clone()),
            "each owns a live worker, so the one idle longest"
        );
        assert!(arena.stop(&worker(0), Stop::Canceled).is_some());
        assert_eq!(
            arena.idle_longest_past(cap, &third, task_of),
            Some(first),
            "an ended worker is not live"
        );
    }

    #[test]
    fn closing_an_idle_task_ends_it_with_its_reply_as_its_result() {
        let mut arena = TaskArena::default();
        let (task, backing) = spawn(&mut arena, chain(0), 0);
        assert_eq!(arena.close(&task), None, "a running task is not idle");
        assert!(arena.reply(&task, "draft".to_owned()).is_some());
        arena.idle(&task);
        assert_eq!(arena.close(&task), Some((backing, "worker 0".to_owned())));
        assert_eq!(arena.state_of(&task), Some(TaskState::Done { ok: true }));
        assert!(
            arena.stop(&task, Stop::Canceled).is_none(),
            "the closed task has ended"
        );
        assert_eq!(arena.take_delivery(&task).expect("the reply"), "draft");
        assert_eq!(
            arena.state_of(&task),
            Some(TaskState::Delivered { ok: true })
        );
        assert!(matches!(
            arena.first_deliverable(chain(0), &[task]),
            Err(refused) if refused.code == ErrorCode::TaskConsumed
        ));
    }

    #[test]
    fn stopping_an_idle_task_drops_its_reply() {
        let mut arena = TaskArena::default();
        let (task, _) = spawn(&mut arena, chain(0), 0);
        assert!(arena.reply(&task, "draft".to_owned()).is_some());
        arena.idle(&task);
        assert!(arena.stop(&task, Stop::Canceled).is_some());
        assert_eq!(arena.state_of(&task), Some(TaskState::Canceled));
        assert!(matches!(
            arena.take_delivery(&task),
            Err(error) if error.code == ErrorCode::TaskCanceled
        ));
    }

    #[test]
    fn a_refused_delivery_leaves_the_entry_unchanged() {
        let mut arena = TaskArena::default();
        let (canceled, _) = spawn(&mut arena, chain(0), 0);
        let (abandoned, _) = spawn(&mut arena, chain(0), 1);
        let (running, _) = spawn(&mut arena, chain(0), 2);
        assert!(arena.stop(&canceled, Stop::Canceled).is_some());
        assert!(arena.stop(&abandoned, Stop::Abandoned).is_some());
        for (task, state) in [
            (&canceled, TaskState::Canceled),
            (&abandoned, TaskState::Abandoned),
            (&running, TaskState::Running),
        ] {
            assert!(arena.take_delivery(task).is_err(), "{task}");
            assert_eq!(arena.state_of(task), Some(state), "{task}");
        }
    }

    #[test]
    fn completing_a_task_that_is_not_live_changes_nothing() {
        let mut arena = TaskArena::default();
        let (canceled, _) = spawn(&mut arena, chain(0), 0);
        assert!(arena.stop(&canceled, Stop::Canceled).is_some());
        assert!(arena.complete(&canceled, Ok("late".to_owned())).is_none());
        assert!(arena.complete(&task(9), Ok("unknown".to_owned())).is_none());
        assert_eq!(arena.state_of(&canceled), Some(TaskState::Canceled));
        assert_eq!(arena.state_of(&task(9)), None);
    }

    #[test]
    fn an_ended_or_delivered_task_keeps_its_entry() {
        let mut arena = TaskArena::default();
        let (canceled, _) = spawn(&mut arena, chain(0), 0);
        let (abandoned, _) = spawn(&mut arena, chain(0), 1);
        let (delivered, _) = spawn(&mut arena, chain(0), 2);
        assert!(arena.stop(&canceled, Stop::Canceled).is_some());
        assert!(arena.stop(&abandoned, Stop::Abandoned).is_some());
        assert!(arena.complete(&delivered, Err(Error::canceled())).is_some());
        assert!(arena.take_delivery(&delivered).is_err());
        for task in [&canceled, &abandoned, &delivered] {
            assert_eq!(arena.owner_of(task), Some(chain(0)), "{task}");
        }
        assert_eq!(
            arena.state_of(&delivered),
            Some(TaskState::Delivered { ok: false })
        );
    }

    #[test]
    fn owned_admits_only_the_tasks_owner() {
        let mut arena = TaskArena::default();
        let (task, _) = spawn(&mut arena, chain(1), 0);
        let entry = arena.owned(chain(1), &task).expect("the owner reaches it");
        assert_eq!(entry.target, "worker 0");
        assert!(matches!(
            arena.owned(chain(2), &task),
            Err(refused) if refused == error::make_task_not_owned_error(&task)
        ));
        let unknown = TaskId::from(ChainId::root().child(9));
        assert!(matches!(
            arena.owned(chain(1), &unknown),
            Err(refused) if refused == error::make_task_not_owned_error(&unknown)
        ));
    }

    #[test]
    fn visible_also_admits_a_caller_running_inside_the_task() {
        let mut arena = TaskArena::default();
        let (task, _) = spawn(&mut arena, chain(1), 0);
        let outside = TaskId::from(ChainId::root());
        assert!(arena.visible(chain(1), &outside, &task).is_ok());
        assert!(
            arena.visible(chain(7), &task, &task).is_ok(),
            "a call child of the task chain shares its sys.taskid"
        );
        assert!(matches!(
            arena.visible(chain(7), &outside, &task),
            Err(refused) if refused == error::make_task_not_owned_error(&task)
        ));
        let unknown = TaskId::from(ChainId::root().child(9));
        assert!(matches!(
            arena.visible(chain(7), &unknown, &unknown),
            Err(refused) if refused.code == ErrorCode::TaskNotOwned
        ));
    }

    #[test]
    fn live_author_tasks_lists_an_owners_live_tasks_in_spawn_order_without_timers() {
        let mut arena = TaskArena::default();
        let (tenth, _) = spawn(&mut arena, chain(0), 10);
        let (second, _) = spawn(&mut arena, chain(0), 2);
        timer(&mut arena, chain(0), 1);
        let (ended, _) = spawn(&mut arena, chain(0), 3);
        spawn(&mut arena, chain(5), 4);
        let (first, _) = spawn(&mut arena, chain(0), 0);
        assert!(arena.stop(&ended, Stop::Canceled).is_some());
        assert_eq!(arena.live_author_tasks(chain(0)), [first, second, tenth]);
    }
}
