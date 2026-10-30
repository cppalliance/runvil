//! The one-chain view: the requests that touch only their own chain run
//! in a [`ChainOps`], which borrows that chain and the effect table
//! mutably and owns nothing. A request in the view can change only its own
//! chain and the effect table: it can't reach another chain, the task
//! arena, admission, or the ready queue, and the compiler enforces that.
//! [`Scheduler::with_chain`] is the only way into the view. The view's
//! requests sit beside the arms they belong to, in the eight
//! `impl ChainOps<'_>` blocks this crate holds: the shared `prompt`,
//! `answer`, `issue`, and `number_round` live here in `chain_ops`, and the
//! arms live in `apply`, `chat`, `dispatch`, `tool_call`, its `adhoc` and
//! `discovery` children, and `walk`.

use std::sync::Arc;

use runvil_types::event::ReplyOrigin;

use crate::execute::run::{Effect, EffectId, Round};
use crate::lua::Answer;
use crate::parser::Prompt;
use crate::{Error, Result};

use super::{Chain, ChainIndex, Continuation, EffectTable, Scheduler};

/// A view of one chain for the requests that touch only it. An answer it
/// delivers is recorded rather than queued, because the view can't reach
/// the ready queue; [`Scheduler::with_chain`] requeues the chain once the
/// view's call returns.
pub(super) struct ChainOps<'a> {
    /// The chain's arena index, which keys the effects it issues.
    id: ChainIndex,
    /// The chain the view runs.
    pub(super) chain: &'a mut Chain,
    /// The effect table the view's leaf arms issue into.
    effects: &'a mut EffectTable,
    /// Whether the view delivered an answer to the chain, which
    /// [`Scheduler::with_chain`] requeues once.
    answered: bool,
}

impl Scheduler {
    /// Runs `f` in the view of chain `id`, then pushes the chain onto the
    /// ready queue if the view delivered an answer. The view pushes nothing
    /// else, so no other chain enters the queue between an answer and its
    /// requeue.
    pub(super) fn with_chain<'s, T>(
        &'s mut self,
        id: ChainIndex,
        f: impl FnOnce(&mut ChainOps<'s>) -> T,
    ) -> T {
        let mut ops = ChainOps {
            id,
            chain: &mut self.chains[id],
            effects: &mut self.effects,
            answered: false,
        };
        let result = f(&mut ops);
        if ops.answered {
            self.ready.push_back(id);
        }
        result
    }
}

impl ChainOps<'_> {
    /// The prompt's shared handle: the chain resolves its walk position
    /// against this clone so the tree borrow never pins the chain. The
    /// chain's own context fork is the source, which is the one `RunState`
    /// the chain reads the prompt from.
    pub(super) fn prompt(&self) -> Arc<Prompt> {
        Arc::clone(self.chain.ctx.prompt())
    }

    /// Delivers `answer` to the chain's suspended block: the inline-answer
    /// path of every request in the view. The requeue is recorded for
    /// [`Scheduler::with_chain`] to perform.
    ///
    /// # Errors
    /// Returns [`Error::Internal`] when the chain has no suspended block
    /// to take the answer, or one that already holds an answer.
    pub(super) fn answer(&mut self, answer: Result<Answer<Error>>) -> Result<()> {
        self.chain.deliver(answer)?;
        self.answered = true;
        Ok(())
    }

    /// Issues one leaf effect for the chain, stamped with the chain's task
    /// provenance, into the effect table with `resume`, the rule its answer
    /// is applied by. The one path every leaf arm takes, so no arm writes
    /// the effect table on its own.
    pub(super) fn issue(&mut self, effect: Effect, resume: Continuation) -> EffectId {
        let provenance = self.chain.ctx.emitter().stamp_effect();
        self.effects.issue(self.id, provenance, effect, resume)
    }

    /// Numbers the model round about to be issued, through
    /// [`EffectTable::number_round`].
    pub(super) fn number_round(&mut self, origin: ReplyOrigin) -> Round {
        self.effects.number_round(origin)
    }
}
