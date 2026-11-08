//! The inboxes, the delivery rules, and the arms that read and fill them:
//! `receive`, `send`, `reply`, `arrivals`, and the inbox check before a
//! `chat` round. An inbox is a queue of text messages, oldest first. The main
//! chain and each task have one, keyed by task id, and a `call` child reads
//! its caller's, since it runs inside its caller's task. The H1 pass and
//! the walk after it are both the main chain, task `0`, so they read one
//! inbox.
//!
//! Only `Run::deliver` fills the main chain's inbox. It queues a message
//! between steps and returns the `InboxMessage` event that reports it,
//! stamped with the main chain's next provenance, and the next step applies
//! the message before any chain runs, so the answers the caller resumed
//! before that step are applied first. Only a task's owner fills the
//! task's inbox, through `tasks.send`, whose step reports the message
//! under the task's provenance; a send to a task that has ended drops it.
//!
//! A message applies by the delivery rules, and reaches no other chain: a
//! chain waiting in `tasks.receive()` resumes with it; a chain whose
//! `models.loop` round has no answer yet has that round withdrawn and
//! resumes with the message, which the loop appends before it sends again;
//! and otherwise it waits in the inbox, which a running tool batch leaves
//! it in. The loop's next `chat` request takes every queued message
//! before its round goes out, and the loop reads the inbox once more
//! before it returns.
//!
//! A `tasks.reply` publishes the task's latest result for its owner, then
//! reads the inbox as `tasks.receive()` does, and a task that parks there
//! is idle until its owner's next message runs it again.
//!
//! A chain that finds its inbox empty parks on it, reports `InboxWaiting`,
//! and gives back its admission slots as a task wait does, and the wake
//! that delivers a message requeues it for them the same way. A message
//! stays queued until a chain reads it or the run ends, and `Step::Done`
//! hands back the main chain's unread messages, oldest first, the ones a
//! woken reader never resumed with included: the teardown puts those back
//! first.

use std::collections::{BTreeMap, VecDeque};

use runvil_types::event::Event;
use runvil_types::ids::{ChainId, TaskId};

use crate::lua::{Answer, MessageList};
use crate::model::ModelBinding;
use crate::{Result, error};

use super::admission::ChainTree;
use super::chain_record::Parked;
use super::tasks::TaskBacking;
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

    /// Puts `texts` back first in `task`'s inbox, in order: the messages a
    /// reader took from it and never read.
    pub(super) fn put_back(&mut self, task: &TaskId, texts: Vec<String>) {
        if texts.is_empty() {
            return;
        }
        let queue = self.queues.entry(task.clone()).or_default();
        for text in texts.into_iter().rev() {
            queue.push_front(text);
        }
    }

    /// Takes the oldest message in `task`'s inbox.
    fn pop(&mut self, task: &TaskId) -> Option<String> {
        self.queues.get_mut(task)?.pop_front()
    }

    /// Takes every message in `task`'s inbox, oldest first.
    fn take_all(&mut self, task: &TaskId) -> Vec<String> {
        self.queues.remove(task).map(Vec::from).unwrap_or_default()
    }

    /// Takes the main chain's unread messages, oldest first, for `Done`:
    /// what is left in its inbox.
    pub(super) fn take_unread(&mut self) -> Vec<String> {
        self.take_all(&main_task())
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
    /// last step, oldest first, each by the delivery rules.
    ///
    /// # Errors
    /// Returns the first failed wake's `Internal` failure, once every
    /// message is applied.
    pub(super) fn apply_deliveries(&mut self) -> Result<()> {
        let main = main_task();
        let mut applied = Ok(());
        for text in std::mem::take(&mut self.inboxes.delivered) {
            let delivered = self.deliver_to(&main, text);
            applied = applied.and(delivered);
        }
        applied
    }

    /// Applies one message to `task`'s inbox by the delivery rules: a chain
    /// of the task waiting in `tasks.receive()` resumes with it, and so
    /// does one waiting in `tasks.reply`, whose idle task runs again; a
    /// chain of the task whose `models.loop` round has no answer yet has
    /// that round withdrawn and resumes with the message, for the loop to
    /// append before it sends again; and otherwise the message waits in the
    /// inbox. A chain waits only on an empty inbox, so the message a reader
    /// takes is the oldest. A step applies its answers before its
    /// messages, so a round whose answer arrived is not withdrawn, and a
    /// running tool batch is never withdrawn.
    ///
    /// # Errors
    /// Returns an `Internal` failure when the chain the message resumes
    /// has no suspended block to take it.
    fn deliver_to(&mut self, task: &TaskId, text: String) -> Result<()> {
        if let Some(reader) = self.chains.inbox_reader(task) {
            self.chains[reader].end_inbox_wait();
            let answer = if self.tasks.wake(task) {
                Answer::Reply(text)
            } else {
                Answer::Receive(text)
            };
            return self.wake_from_wait(reader, Ok(answer));
        }
        let round = self
            .chains
            .iter()
            .filter(|(_, chain)| &chain.task == task)
            .find_map(|(id, _)| Some((id, self.effects.awaited_round(id)?)));
        let Some((chain, round)) = round else {
            self.inboxes.push(task, text);
            return Ok(());
        };
        self.abort_effect(round);
        self.chains[chain].anchor.settle(None);
        self.answer_inline(chain, Ok(Answer::Arrivals(vec![text])))
    }

    /// Dispatches a `receive` request: the oldest message in the inbox of
    /// the chain's task answers it at once. With the inbox empty, the
    /// chain parks on it until a message arrives, reports `InboxWaiting`,
    /// and gives back its admission slots as a task wait does.
    pub(super) fn dispatch_receive(&mut self, id: ChainIndex) -> Result<()> {
        if let Some(text) = self.inboxes.pop(&self.chains[id].task) {
            return self.answer_inline(id, Ok(Answer::Receive(text)));
        }
        self.park_on_inbox(id);
        Ok(())
    }

    /// Dispatches a `reply` request: `text` becomes the latest result of
    /// the task the chain runs inside, a `call` child's caller's task
    /// included, and an owner waiting on the task takes it at once. Then
    /// the oldest message in the task's inbox answers the reply at once, or,
    /// with the inbox empty, the task idles: the chain parks on the inbox as
    /// a `receive` does, and a reply past the run's idle cap closes the
    /// task idle longest. The main chain has no owner to reply to, so its
    /// reply is refused with a `lua` error.
    ///
    /// # Errors
    /// Returns an `Internal` failure when the chain's task has no running
    /// entry, or when the woken owner has no suspended block.
    pub(super) fn dispatch_reply(&mut self, id: ChainIndex, text: String) -> Result<()> {
        let task = self.chains[id].task.clone();
        if task == main_task() {
            return self.answer_inline(
                id,
                Err(error::make_lua_runtime_error(
                    "tasks.reply has no owner to reply to on the main chain",
                )),
            );
        }
        let Some(owner) = self.tasks.reply(&task, text).map(|entry| entry.owner) else {
            return Err(error::make_internal_error(
                "a replying chain's task is running",
            ));
        };
        self.wake_waiter(owner, &task)?;
        if let Some(text) = self.inboxes.pop(&task) {
            return self.answer_inline(id, Ok(Answer::Reply(text)));
        }
        self.tasks.idle(&task);
        self.park_on_inbox(id);
        self.close_past_idle_cap(&task);
        Ok(())
    }

    /// Parks the chain on the empty inbox of its task until a message
    /// arrives: it reports `InboxWaiting` and gives back its admission
    /// slots, as a task wait does.
    fn park_on_inbox(&mut self, id: ChainIndex) {
        let chain = &mut self.chains[id];
        chain.parked = Some(Parked::Inbox { waiting: true });
        chain
            .ctx
            .emitter()
            .emit(chain.section_name(), |section, provenance| {
                Event::InboxWaiting {
                    section,
                    provenance,
                }
            });
        let tree = ChainTree {
            chains: &self.chains,
            tasks: &self.tasks,
        };
        self.admission.park(id, &tree, true);
    }

    /// Dispatches a `send` request: `text` enters the inbox of a live task
    /// the chain owns, by the delivery rules, under an `InboxMessage` event
    /// stamped with the task's provenance in the section that reads it,
    /// and the send answers true; a task that has ended drops it and
    /// answers false. A task the chain does not own, and the internal
    /// timer behind a timed wait, which has no inbox, refuse it with
    /// `task_not_owned`.
    pub(super) fn dispatch_send(
        &mut self,
        id: ChainIndex,
        task: &TaskId,
        text: String,
    ) -> Result<()> {
        let backing = self
            .tasks
            .owned(id, task)
            .and_then(|entry| match entry.backing {
                TaskBacking::Chain { chain, .. } => Ok(entry.state.is_live().then_some(chain)),
                TaskBacking::Effect(_) => Err(error::make_task_not_owned_error(task)),
            });
        let chain = match backing {
            Ok(Some(chain)) => chain,
            Ok(None) => return self.answer_inline(id, Ok(Answer::Send(false))),
            Err(refused) => return self.answer_inline(id, Err(refused)),
        };
        let reader = self.chains.leaf_of(task).unwrap_or(chain);
        let section = self.chains[reader].section_name().to_owned();
        let reported = text.clone();
        self.chains[chain]
            .ctx
            .emitter()
            .emit(&section, |section, provenance| Event::InboxMessage {
                section,
                provenance,
                text: reported,
            });
        self.deliver_to(task, text)?;
        self.answer_inline(id, Ok(Answer::Send(true)))
    }

    /// Dispatches an `arrivals` request, the loop's last read before it
    /// returns: every message queued in the inbox of the chain's task
    /// answers it at once, oldest first, and an empty inbox answers it
    /// with none.
    pub(super) fn dispatch_arrivals(&mut self, id: ChainIndex) -> Result<()> {
        let arrived = self.inboxes.take_all(&self.chains[id].task);
        self.answer_inline(id, Ok(Answer::Arrivals(arrived)))
    }

    /// Dispatches a `chat` request, the loop's next model round over
    /// `list` under `binding`: the messages queued in the inbox of the
    /// chain's task answer it at once, oldest first, for the loop to append
    /// before it sends again, so every round holds the messages that
    /// arrived before it. With none queued, the chat arm sends the round.
    pub(super) fn dispatch_round(
        &mut self,
        id: ChainIndex,
        list: &MessageList,
        binding: Option<ModelBinding>,
    ) -> Result<()> {
        let arrived = self.inboxes.take_all(&self.chains[id].task);
        if arrived.is_empty() {
            return self.with_chain(id, |ops| ops.dispatch_chat(list, binding));
        }
        self.answer_inline(id, Ok(Answer::Arrivals(arrived)))
    }
}
