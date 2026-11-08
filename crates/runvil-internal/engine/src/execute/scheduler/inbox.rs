//! The inboxes and the `receive` arm. An inbox is a queue of text
//! messages, oldest first. The main chain and each task have one, keyed by
//! task id, and a `call` child reads its caller's, since it runs inside its
//! caller's task. The H1 pass and the walk after it are both the main
//! chain, task `0`, so they read one inbox.
//!
//! `Run::deliver` queues a message for the main chain between steps and
//! returns the `InboxMessage` event that reports it, stamped with the main
//! chain's next provenance. The next step applies the message before any
//! chain runs, so the answers the caller resumed before that step are
//! applied first. Applying a message wakes the chain waiting on that
//! inbox, if one is, and otherwise leaves the message queued for the next
//! `tasks.receive()`.
//!
//! A chain that finds its inbox empty parks on it and gives back its
//! admission slots as a task wait does, and the wake that delivers a
//! message requeues it for them the same way. A message stays queued until
//! a chain reads it or the run ends, and `Step::Done` hands back the main
//! chain's unread messages, oldest first, the one a woken reader never
//! resumed with included: the teardown puts that one back first.

use std::collections::{BTreeMap, VecDeque};

use runvil_types::event::Event;
use runvil_types::ids::{ChainId, TaskId};

use crate::Result;
use crate::lua::Answer;

use super::admission::ChainTree;
use super::chain_record::Parked;
use super::{ChainIndex, Scheduler};

/// The main chain's task, `0`, which the H1 pass, the walk, and their
/// `call` children share.
pub(super) fn main_task() -> TaskId {
    TaskId::from(ChainId::root())
}

/// Every inbox of the run, and the main chain's messages `Run::deliver`
/// queued since the last step.
#[derive(Debug, Default)]
pub(super) struct Inboxes {
    /// Each task's unread messages, oldest first. An inbox no message has
    /// entered has no entry.
    queues: BTreeMap<TaskId, VecDeque<String>>,
    /// The main chain's messages `Run::deliver` queued since the last
    /// step, oldest first, for the next step to apply.
    delivered: Vec<String>,
}

impl Inboxes {
    /// Queues `text` last in `task`'s inbox.
    fn push(&mut self, task: &TaskId, text: String) {
        self.queues.entry(task.clone()).or_default().push_back(text);
    }

    /// Puts `text` back first in `task`'s inbox: the message a reader
    /// took from it and never read.
    pub(super) fn put_back(&mut self, task: &TaskId, text: String) {
        self.queues
            .entry(task.clone())
            .or_default()
            .push_front(text);
    }

    /// Takes the oldest message in `task`'s inbox.
    fn pop(&mut self, task: &TaskId) -> Option<String> {
        self.queues.get_mut(task)?.pop_front()
    }

    /// Takes the main chain's unread messages, oldest first, for `Done`:
    /// what is left in its inbox.
    pub(super) fn take_unread(&mut self) -> Vec<String> {
        self.queues
            .remove(&main_task())
            .map(Vec::from)
            .unwrap_or_default()
    }
}

impl Scheduler {
    /// Queues `text` for the main chain's inbox, for the next step to
    /// apply, and returns the `InboxMessage` event that reports it. The
    /// event takes the main chain's next provenance and reports under the
    /// section the main chain is in, or the prompt's title while no main
    /// chain is live, before the run starts and after it ends.
    pub(crate) fn deliver_message(&mut self, text: String) -> Event {
        let section = self
            .chains
            .leaf_of(&main_task())
            .map_or_else(
                || self.ctx.prompt().title(),
                |leaf| self.chains[leaf].section_name(),
            )
            .to_owned();
        // Returned, never pushed: the caller records it before the step
        // that applies the message, in the place its stamp gives it.
        let event = Event::InboxMessage {
            section,
            provenance: self.ctx.emitter().stamp_effect(),
            text: text.clone(),
        };
        self.inboxes.delivered.push(text);
        event
    }

    /// Applies the main chain's messages `Run::deliver` queued since the
    /// last step, oldest first: each goes to the chain waiting on the main
    /// chain's inbox, which resumes with it, or into the inbox when no
    /// chain waits. A chain waits only on an empty inbox, and the first
    /// message to arrive ends its wait, so the one it takes is the oldest.
    ///
    /// # Errors
    /// Returns the first failed wake's `Internal` failure, once every
    /// message is applied.
    pub(super) fn apply_deliveries(&mut self) -> Result<()> {
        let main = main_task();
        let mut applied = Ok(());
        for text in std::mem::take(&mut self.inboxes.delivered) {
            let Some(reader) = self.chains.inbox_reader(&main) else {
                self.inboxes.push(&main, text);
                continue;
            };
            self.chains[reader].end_inbox_wait();
            let woken = self.wake_from_wait(reader, Ok(Answer::Receive(text)));
            applied = applied.and(woken);
        }
        applied
    }

    /// Dispatches a `receive` request: the oldest message in the inbox of
    /// the chain's task answers it at once. With the inbox empty, the
    /// chain parks on it until a message arrives, giving back its
    /// admission slots as a task wait does.
    pub(super) fn dispatch_receive(&mut self, id: ChainIndex) -> Result<()> {
        if let Some(text) = self.inboxes.pop(&self.chains[id].task) {
            return self.answer_inline(id, Ok(Answer::Receive(text)));
        }
        self.chains[id].parked = Some(Parked::Inbox { waiting: true });
        let tree = ChainTree {
            chains: &self.chains,
            tasks: &self.tasks,
        };
        self.admission.park(id, &tree, true);
        Ok(())
    }
}
