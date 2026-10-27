//! The `timer` arm: the internal timeout behind a timed wait (an author's
//! `opts.timeout`), as an effect-backed task entry.
//!
//! A timer is the one task backed by an in-flight leaf request rather than
//! a chain: the request sleeps and posts back, and its entry sits in the
//! task arena beside the chain-backed ones so one wait primitive serves
//! both - the shim lists the timer's id in its `join_any` set, and the
//! timer's firing completes its entry and wakes the waiter exactly as a
//! task chain's end does. Cancel is the ordinary cancel arm: the entry
//! moves to `Cancelled` and the sleep is dropped through the shared
//! in-flight abort path.
//!
//! The timer is never author-visible. The shim keeps its id, `pending`
//! and a status table's `tasks` list omit effect-backed entries, and no task
//! observation fires for one: it is the wait's implementation detail, not
//! a task the author started. Its id still consumes the owner's next child
//! index so that every id the owner hands out stays a function of the
//! owner's own dispatch order.
//!
//! The sleep is a `Timer` effect issued under the owner: keyed in the
//! effect table under the owner so the stall check and the abort paths
//! see it as the in-flight effect it is, and performed by the caller.

use std::time::Duration;

use runvil_types::ids::TaskId;

use crate::execute::run::Effect;
use crate::lua::Answer;
use crate::{Error, Result};

use super::tasks::TaskBacking;
use super::{ChainIndex, Continuation, Scheduler};

impl Scheduler {
    /// Dispatches a `timer` request: allocates the timer's id under the
    /// caller, registers its effect-backed entry, issues the sleep as an
    /// effect, and resumes the caller at once with the id. The wait shim
    /// raises a dispatch failure before any wait.
    pub(super) fn dispatch_timer(&mut self, id: ChainIndex, duration: Duration) -> Result<()> {
        let answer = Answer::Timer(self.prepare_timer(id, duration));
        self.answer_inline(id, answer)
    }

    /// The fallible half of timer dispatch: the id allocation, the issued
    /// effect, and the entry.
    fn prepare_timer(&mut self, id: ChainIndex, duration: Duration) -> Result<TaskId> {
        let task = TaskId::from(self.chains[id].next_child_id()?);
        let effect = self.with_chain(id, |ops| {
            ops.issue(
                Effect::Timer { duration },
                Continuation::Timer(task.clone()),
            )
        });
        self.tasks.insert(
            task.clone(),
            TaskBacking::Effect(effect),
            id,
            "timer".to_owned(),
        );
        Ok(task)
    }

    /// Applies a timer's firing: `task`'s entry moves to `Done` with an
    /// empty outcome and its owner is woken if it is parked on a set
    /// containing the timer. Otherwise the entry holds until the owner's
    /// next wait delivers it - the shim's `join` rounds may be between
    /// waits when the timer fires.
    ///
    /// # Errors
    /// Returns [`Error::Internal`] when `task`'s entry isn't live - a
    /// cancelled or abandoned timer's effect is aborted and its late firing
    /// discarded before it reaches here - or when the woken owner has no
    /// suspended block.
    pub(super) fn fire_timer(&mut self, task: &TaskId) -> Result<()> {
        let Some(entry) = self.tasks.complete(task, Ok(String::new())) else {
            return Err(Error::internal("a fired timer has a live entry"));
        };
        let owner = entry.owner;
        self.wake_waiter(owner, task)
    }
}
