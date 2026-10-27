//! The effect table: every effect the run issued that the caller has not
//! answered yet, and the step's outgoing queue.
//!
//! Every issued effect is answered exactly once. An answer to an orphan,
//! an effect whose chain stopped waiting, is discarded. An answer for an
//! id the run never issued, or a second answer to one effect, is reported
//! to the scheduler, which fails the run loudly rather than passing it
//! silently. An effect issued in a step goes out with that step even when
//! the run's end orphaned it in the same step, because the caller still
//! owes it an answer. The step also lists the tool calls orphaned since
//! the last step, in id order.
//!
//! An awaited entry holds what its chain asked for, in the terms
//! `apply_answer` needs to turn the caller's raw [`EffectAnswer`] into the
//! chain's protocol [`Answer`] and emit the round's events. The effect
//! itself holds none of this: it describes the work, the continuation
//! describes what the work means to the chain.
//!
//! [`EffectAnswer`]: super::EffectAnswer
//! [`Answer`]: super::Answer

use std::collections::HashMap;

use runvil_types::event::ReplyOrigin;
use runvil_types::ids::{Provenance, RoundId};
use runvil_vfs::ExecId;

use crate::execute::run::{Effect, EffectId, IssuedEffect, Round};
use crate::lua::{ScriptReport, ToolBinding, VfsOp};
use crate::model::ModelBinding;

use super::ChainIndex;

/// The driver-side half of one issued leaf effect: how its answer resumes
/// the chain awaiting it.
#[derive(Debug)]
pub(super) enum Continuation {
    /// A nested `models.infer`: the completion becomes the round's text
    /// under the single-prose-round reporting rules. The round's id waits
    /// here for the answer, whose events it stamps.
    Infer(RoundId),
    /// A `chat` round: the completion is classified against the frozen
    /// sets of the section the chain is parked in and reported as one
    /// model turn. The round's id waits here for the answer, whose events
    /// it stamps.
    Chat(RoundId),
    /// The render a section entry's first `chat` round issued: the
    /// rendered tools are merged into the entry's frozen sets, with the
    /// offers and the list read back from the parked frame, and the round
    /// is then dispatched under the binding held here. Only the binding
    /// waits here, because a handle round's binding exists nowhere in VM
    /// state, and resolving the section's model again could pick a
    /// default another chain set meanwhile.
    Render(ModelBinding),
    /// A bound tool call: the tool's own output goes through the shared
    /// dispatch body (counts already taken at dispatch, then the
    /// succeeded/failed event, the trust rule, and the `ToolResult`).
    ToolCall(ToolCallContinuation),
    /// A [`Vfs`](super::Effect::Vfs) effect: the operation the chain
    /// yielded, from which the answer's succeeded/failed observation, its
    /// failure's model-facing message, and its determinism classification
    /// render under the call surface the author used.
    Vfs(VfsOp),
    /// The internal timer behind a timed wait: the firing completes the
    /// task entry backed by the effect and wakes its waiting owner; no chain
    /// resumes.
    Timer,
}

/// What a bound `tool_call`'s answer is applied with: the binding the call
/// resolved to (its alias, output kind, and trust rules), the coordinates
/// the `ToolResult` reports under, the call's site, and the call's
/// identity the answer joins.
#[derive(Debug)]
pub(super) struct ToolCallContinuation {
    /// The binding the alias resolved to at dispatch.
    pub(super) binding: ToolBinding,
    /// The turn the call fired in.
    pub(super) report: ScriptReport,
    /// Who asked for the call, and where its answer goes.
    pub(super) site: CallSite,
    /// The call's own identity, forked from the chain's access at
    /// dispatch. The answer, or the call's abort, ends it and joins it
    /// back into the chain.
    pub(super) exec: ExecId,
}

/// Who asked for one tool call, and where its answer goes. Only a
/// model-issued call carries a call id, and only a batched one a slot.
#[derive(Debug)]
pub(super) enum CallSite {
    /// A script's `tools.call`, under the turn its request named, if any.
    /// The answer resumes the chain directly, classified by the binding's
    /// declared output kind, and reports under no call id.
    Script { turn: Option<u32> },
    /// A model-issued call sent alone, under the turn of the round that
    /// requested it, if the request named one. The answer resumes the
    /// chain directly as content under the model's call id.
    Model { call_id: String, turn: Option<u32> },
    /// A model-issued call sent in a `tool_calls` batch, under the round's
    /// turn. The answer fills `slot` of the batch the chain is parked on.
    Batch {
        call_id: String,
        turn: u32,
        slot: usize,
    },
}

impl CallSite {
    /// The model's call id, or `None` for a script call.
    pub(super) fn call_id(&self) -> Option<&str> {
        match self {
            Self::Script { .. } => None,
            Self::Model { call_id, .. } | Self::Batch { call_id, .. } => Some(call_id.as_str()),
        }
    }

    /// The turn the request named, if any.
    pub(super) fn turn(&self) -> Option<u32> {
        match self {
            Self::Script { turn } | Self::Model { turn, .. } => *turn,
            Self::Batch { turn, .. } => Some(*turn),
        }
    }
}

/// One issued effect the caller has not answered yet.
#[derive(Debug)]
#[expect(
    clippy::large_enum_variant,
    reason = "most entries are awaited, and an orphan's unused room lasts only until its answer lands"
)]
enum Outstanding {
    /// A chain awaits the effect's answer.
    Awaited(Pending),
    /// The chain stopped waiting before the caller answered: the answer is
    /// still owed, and it is discarded on arrival.
    Orphaned,
}

/// An awaited effect's entry: the chain awaiting it and how its answer
/// resumes that chain.
#[derive(Debug)]
pub(super) struct Pending {
    /// The awaiting chain: for a timer, the owner whose wait the timer
    /// serves, which need not be suspended.
    pub(super) chain: ChainIndex,
    /// How the answer is applied.
    pub(super) resume: Continuation,
}

/// What an answer finds in the table on arrival.
#[derive(Debug)]
#[expect(
    clippy::large_enum_variant,
    reason = "an arrival lives only until `resume` matches it, and most arrivals are awaited"
)]
pub(super) enum Arrival {
    /// A chain awaits the answer: its entry, taken out of the table.
    Awaited(Pending),
    /// The chain stopped waiting before the caller answered, so the answer
    /// is discarded.
    Orphaned,
    /// The run never issued the id, or its answer already arrived.
    Unknown,
}

/// The effect table and the step's outgoing queue.
#[derive(Default)]
pub(super) struct EffectTable {
    /// One entry per issued effect the caller has not answered yet, keyed
    /// by the effect's id. An entry is awaited while its chain waits, and
    /// orphaned once the chain stopped waiting: a chain end, a task cancel
    /// or abandonment, or the run's teardown. An entry leaves when its
    /// answer arrives.
    outstanding: HashMap<EffectId, Outstanding>,
    /// The effects issued since the step began, in issue order, each with
    /// the provenance of the task that built it; the step returns them.
    issued: Vec<IssuedEffect>,
    /// Tool-call effects orphaned since the last step, drained into
    /// `Step::Pending`.
    withdrawn: Vec<EffectId>,
    /// The next effect id: a run-wide counter, so every effect the run
    /// issues has a distinct in-flight handle.
    next_effect: u64,
    /// The next round id: a run-wide counter numbering the model rounds
    /// in dispatch order, chat and nested-infer rounds alike.
    next_round: u64,
}

impl EffectTable {
    /// Issues one leaf effect for `chain` under `provenance`: allocates its
    /// id, queues it for the step's return, and records `chain` awaiting it
    /// with `resume`, the rule its answer is applied by.
    pub(super) fn issue(
        &mut self,
        chain: ChainIndex,
        provenance: Provenance,
        effect: Effect,
        resume: Continuation,
    ) -> EffectId {
        let id = EffectId(self.next_effect);
        self.next_effect += 1;
        self.issued.push(IssuedEffect {
            id,
            provenance,
            effect,
        });
        self.outstanding
            .insert(id, Outstanding::Awaited(Pending { chain, resume }));
        id
    }

    /// Numbers the model round about to be issued with `origin`: the run's
    /// next round id, in dispatch order.
    pub(super) fn number_round(&mut self, origin: ReplyOrigin) -> Round {
        let id = RoundId::new(self.next_round);
        self.next_round += 1;
        Round { id, origin }
    }

    /// Takes `id`'s entry out of the table as its answer arrives.
    pub(super) fn arrive(&mut self, id: EffectId) -> Arrival {
        match self.outstanding.remove(&id) {
            Some(Outstanding::Awaited(pending)) => Arrival::Awaited(pending),
            Some(Outstanding::Orphaned) => Arrival::Orphaned,
            None => Arrival::Unknown,
        }
    }

    /// Orphans `id`, whose chain stopped waiting: the caller still owes the
    /// answer, which is discarded on arrival. A tool call is also listed
    /// as withdrawn, and its chain and identity are returned so the call
    /// can be ended. An id with no entry stays unknown, so an answer for it
    /// still fails loudly.
    pub(super) fn orphan(&mut self, id: EffectId) -> Option<(ChainIndex, ExecId)> {
        let entry = self.outstanding.get_mut(&id)?;
        let Outstanding::Awaited(Pending {
            chain,
            resume: Continuation::ToolCall(call),
        }) = std::mem::replace(entry, Outstanding::Orphaned)
        else {
            return None;
        };
        self.withdrawn.push(id);
        Some((chain, call.exec))
    }

    /// Orphans every awaited effect at the run's teardown, silently: no
    /// call is listed as withdrawn and no identity is returned, because
    /// those end when the run's scope closes.
    pub(super) fn orphan_all(&mut self) {
        for entry in self.outstanding.values_mut() {
            *entry = Outstanding::Orphaned;
        }
    }

    /// The effects `chain` awaits, in issue order.
    pub(super) fn awaited_by(&self, chain: ChainIndex) -> Vec<EffectId> {
        let mut effects: Vec<EffectId> = self
            .outstanding
            .iter()
            .filter(|(_, entry)| matches!(entry, Outstanding::Awaited(pending) if pending.chain == chain))
            .map(|(effect, _)| *effect)
            .collect();
        effects.sort_unstable();
        effects
    }

    /// Whether any chain awaits an effect's answer.
    pub(super) fn has_awaited(&self) -> bool {
        self.outstanding
            .values()
            .any(|entry| matches!(entry, Outstanding::Awaited(_)))
    }

    /// Whether every issued effect has had its answer, the orphans'
    /// included.
    pub(super) fn all_answered(&self) -> bool {
        self.outstanding.is_empty()
    }

    /// Moves every effect `from` awaits to `to`.
    pub(super) fn reassign(&mut self, from: ChainIndex, to: ChainIndex) {
        for entry in self.outstanding.values_mut() {
            if let Outstanding::Awaited(pending) = entry
                && pending.chain == from
            {
                pending.chain = to;
            }
        }
    }

    /// Takes the step's outgoing queue: the effects issued since the step
    /// began, in issue order, and the tool calls withdrawn since the last
    /// step, in id order.
    pub(super) fn take_step(&mut self) -> (Vec<IssuedEffect>, Vec<EffectId>) {
        let issued = std::mem::take(&mut self.issued);
        let mut withdrawn = std::mem::take(&mut self.withdrawn);
        withdrawn.sort_unstable();
        (issued, withdrawn)
    }

    /// Drops the withdrawals listed since the last step.
    pub(super) fn clear_withdrawn(&mut self) {
        self.withdrawn.clear();
    }

    /// Whether `id` is recorded as orphaned.
    #[cfg(test)]
    pub(super) fn is_orphaned(&self, id: EffectId) -> bool {
        matches!(self.outstanding.get(&id), Some(Outstanding::Orphaned))
    }
}

#[cfg(test)]
#[path = "effects-tests.rs"]
mod tests;
