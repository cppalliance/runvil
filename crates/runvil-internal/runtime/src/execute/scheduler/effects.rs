//! The effect table: every effect the run issued that the caller has not
//! answered yet, and the step's outgoing queue.
//!
//! Every issued effect is answered exactly once. An answer to an orphan,
//! an effect whose chain stopped waiting, is discarded. An answer for an
//! id the run never issued, or a second answer to one effect, is reported
//! to the scheduler, which fails the run loudly rather than passing it
//! silently. An effect issued in a step goes out with that step even when
//! the run's end orphaned it in the same step, because the caller still
//! owes it an answer. The step also lists the effects withdrawn since the
//! last step, in id order: every effect orphaned while the run goes on, of
//! every kind, and every outstanding effect once the run's end tears it
//! down.
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
use runvil_types::ids::{Provenance, RoundId, TaskId};
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
    /// A [`Vfs`](Effect::Vfs) effect: the operation the chain
    /// yielded, from which the answer's succeeded/failed observation, its
    /// failure's model-facing message, and its make_determinism_error classification
    /// render under the call surface the author used.
    Vfs(VfsOp),
    /// The internal timer behind a timed wait, naming the timer's task: the
    /// firing completes that task's entry and wakes its waiting owner; no
    /// chain resumes.
    Timer(TaskId),
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
    /// A script's `tools.call`, under the turn counter at dispatch. The
    /// answer resumes the chain directly, classified by the binding's
    /// declared output kind, and reports under no call id.
    Script,
    /// A model-issued call sent alone, under the turn of the round that
    /// requested it. The answer resumes the chain directly as content
    /// under the model's call id.
    Model { call_id: String, turn: u32 },
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
            Self::Script => None,
            Self::Model { call_id, .. } | Self::Batch { call_id, .. } => Some(call_id.as_str()),
        }
    }

    /// The turn of the round that requested a model-issued call, or
    /// `None` for a script call.
    pub(super) fn turn(&self) -> Option<u32> {
        match self {
            Self::Script => None,
            Self::Model { turn, .. } | Self::Batch { turn, .. } => Some(*turn),
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
    /// or abandonment, a message that withdrew a loop's round, or the run's
    /// teardown. An entry leaves when its answer arrives.
    outstanding: HashMap<EffectId, Outstanding>,
    /// The effects issued since the step began, in issue order, each with
    /// the provenance of the task that built it; the step returns them.
    issued: Vec<IssuedEffect>,
    /// Effects withdrawn since the last step, drained into
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

    /// Orphans `id`, whose chain stopped waiting, and lists it as
    /// withdrawn: the caller still owes the answer, which is discarded on
    /// arrival. Only a tool call has an identity to end, so only a tool
    /// call returns its chain and identity; a `chat` round, an `infer`, a
    /// render, a store operation, and a timer return `None`. An id with no
    /// entry stays unknown, so an answer for it still fails loudly, and an
    /// orphan stays as it is.
    pub(super) fn orphan(&mut self, id: EffectId) -> Option<(ChainIndex, ExecId)> {
        let entry = self.outstanding.get_mut(&id)?;
        let Outstanding::Awaited(Pending { chain, resume }) =
            std::mem::replace(entry, Outstanding::Orphaned)
        else {
            return None;
        };
        self.withdrawn.push(id);
        match resume {
            Continuation::ToolCall(call) => Some((chain, call.exec)),
            Continuation::Infer(_)
            | Continuation::Chat(_)
            | Continuation::Render(_)
            | Continuation::Vfs(_)
            | Continuation::Timer(_) => None,
        }
    }

    /// Orphans every awaited effect at the run's teardown and withdraws
    /// every outstanding effect, the earlier orphans included, so the
    /// caller may drop all it holds. No identity is returned, because
    /// those end when the run's scope closes.
    pub(super) fn orphan_all(&mut self) {
        self.withdrawn.clear();
        for (id, entry) in &mut self.outstanding {
            *entry = Outstanding::Orphaned;
            self.withdrawn.push(*id);
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

    /// The `models.loop` round `chain` awaits, if it awaits one: a `chat`
    /// request that has no answer yet.
    pub(super) fn awaited_round(&self, chain: ChainIndex) -> Option<EffectId> {
        self.outstanding.iter().find_map(|(id, entry)| match entry {
            Outstanding::Awaited(Pending {
                chain: awaiting,
                resume: Continuation::Chat(_),
            }) if *awaiting == chain => Some(*id),
            _ => None,
        })
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
    /// began, in issue order, and the effects withdrawn since the last
    /// step that still owe their answer, in id order.
    pub(super) fn take_step(&mut self) -> (Vec<IssuedEffect>, Vec<EffectId>) {
        let issued = std::mem::take(&mut self.issued);
        let mut withdrawn = std::mem::take(&mut self.withdrawn);
        withdrawn.retain(|id| self.outstanding.contains_key(id));
        withdrawn.sort_unstable();
        (issued, withdrawn)
    }

    /// Whether `id` is recorded as orphaned.
    #[cfg(test)]
    pub(super) fn is_orphaned(&self, id: EffectId) -> bool {
        matches!(self.outstanding.get(&id), Some(Outstanding::Orphaned))
    }
}

#[cfg(test)]
mod tests {
    //! Tests for the effect table alone, one rule each: an issued effect takes
    //! exactly one answer, an orphan's answer is discarded, unknown ids and
    //! second answers are reported, every orphaned effect is withdrawn, in id
    //! order, and teardown withdraws every outstanding one, a step lists only
    //! withdrawals still owed an answer, and an effect orphaned in its own
    //! step still goes out with that step.

    use std::time::Duration;

    use runvil_types::ids::{ChainId, Provenance, RoundId, TaskId};
    use runvil_types::tools::{ToolId, ToolInfo};
    use runvil_vfs::{ExecId, Origin, VfsRef};
    use serde_json::json;

    use super::super::ChainIndex;
    use super::{Arrival, CallSite, Continuation, EffectTable, ToolCallContinuation};
    use crate::execute::run::{Effect, EffectId};
    use crate::lua::{ScriptReport, ToolBinding};

    fn chain(raw: u32) -> ChainIndex {
        ChainIndex::for_test(raw)
    }

    fn provenance() -> Provenance {
        Provenance::new(TaskId::from(ChainId::root()), 0)
    }

    fn timer() -> Effect {
        Effect::Timer {
            duration: Duration::from_secs(1),
        }
    }

    /// Issues a timer for `chain`: the simplest effect to build.
    fn issue_timer(table: &mut EffectTable, chain: ChainIndex) -> EffectId {
        table.issue(
            chain,
            provenance(),
            timer(),
            Continuation::Timer(TaskId::from(ChainId::root().child(0))),
        )
    }

    /// Issues a `chat` round for `chain`. The table reads only the
    /// continuation, so the effect itself is a timer's.
    fn issue_round(table: &mut EffectTable, chain: ChainIndex) -> EffectId {
        table.issue(
            chain,
            provenance(),
            timer(),
            Continuation::Chat(RoundId::new(0)),
        )
    }

    /// Issues a script tool call for `chain` under a fresh identity and returns
    /// the effect's id with that identity. The table reads only the
    /// continuation, so the effect itself is a timer's.
    fn issue_tool_call(table: &mut EffectTable, chain: ChainIndex) -> (EffectId, ExecId) {
        let access = VfsRef::default()
            .acquire(Origin::new("effects-test"))
            .expect("the default backend acquires");
        let exec = access.id();
        let tool = ToolId::parse("example/test/echo").expect("a fixture id is a tool id");
        let binding = ToolBinding::from_info(
            "echo",
            &ToolInfo::new(tool, "echo", json!({ "type": "object" })),
        );
        let resume = Continuation::ToolCall(ToolCallContinuation {
            binding,
            report: ScriptReport { turn: 0 },
            site: CallSite::Script,
            exec,
        });
        (table.issue(chain, provenance(), timer(), resume), exec)
    }

    #[test]
    fn an_issued_effect_is_awaited_until_its_one_answer_arrives() {
        let mut table = EffectTable::default();
        let effect = issue_timer(&mut table, chain(3));
        assert!(table.has_awaited());
        assert!(!table.all_answered());
        let Arrival::Awaited(pending) = table.arrive(effect) else {
            panic!("the first answer finds its chain awaiting it");
        };
        assert_eq!(pending.chain, chain(3));
        assert!(matches!(pending.resume, Continuation::Timer(_)));
        assert!(!table.has_awaited());
        assert!(table.all_answered());
    }

    #[test]
    fn an_orphans_answer_is_discarded_on_arrival() {
        let mut table = EffectTable::default();
        let effect = issue_timer(&mut table, chain(0));
        assert_eq!(table.orphan(effect), None, "a timer has no call identity");
        assert!(!table.has_awaited(), "no chain awaits an orphan");
        assert!(
            !table.all_answered(),
            "the caller still owes the orphan's answer"
        );
        assert!(matches!(table.arrive(effect), Arrival::Orphaned));
        assert!(table.all_answered());
    }

    #[test]
    fn an_unknown_id_and_a_second_answer_are_reported() {
        let mut table = EffectTable::default();
        assert!(
            matches!(table.arrive(EffectId(7)), Arrival::Unknown),
            "the table never issued 7"
        );
        let answered = issue_timer(&mut table, chain(0));
        assert!(matches!(table.arrive(answered), Arrival::Awaited(_)));
        assert!(matches!(table.arrive(answered), Arrival::Unknown));
        let orphan = issue_timer(&mut table, chain(0));
        table.orphan(orphan);
        assert!(matches!(table.arrive(orphan), Arrival::Orphaned));
        assert!(
            matches!(table.arrive(orphan), Arrival::Unknown),
            "an orphan takes one answer too"
        );
    }

    #[test]
    fn orphaning_an_id_that_is_not_outstanding_records_nothing() {
        let mut table = EffectTable::default();
        let (answered, _) = issue_tool_call(&mut table, chain(0));
        assert!(matches!(table.arrive(answered), Arrival::Awaited(_)));
        assert_eq!(table.orphan(answered), None);
        assert_eq!(table.orphan(EffectId(9)), None);
        let (_, withdrawn) = table.take_step();
        assert!(withdrawn.is_empty(), "{withdrawn:?}");
        assert!(
            matches!(table.arrive(answered), Arrival::Unknown),
            "orphaning an answered id leaves it unknown"
        );
        assert!(matches!(table.arrive(EffectId(9)), Arrival::Unknown));
    }

    #[test]
    fn every_orphaned_effect_is_withdrawn_in_id_order() {
        let mut table = EffectTable::default();
        let timer = issue_timer(&mut table, chain(0));
        let (first, first_exec) = issue_tool_call(&mut table, chain(1));
        let round = issue_round(&mut table, chain(2));
        let (second, second_exec) = issue_tool_call(&mut table, chain(1));
        assert_eq!(table.orphan(second), Some((chain(1), second_exec)));
        assert_eq!(table.orphan(round), None, "a round has no call identity");
        assert_eq!(table.orphan(timer), None, "a timer has no call identity");
        assert_eq!(table.orphan(first), Some((chain(1), first_exec)));
        let (_, withdrawn) = table.take_step();
        assert_eq!(
            withdrawn,
            [timer, first, round, second],
            "the timer and the round are withdrawn beside the calls"
        );
        assert_eq!(
            table.orphan(round),
            None,
            "an orphan orphaned again stays as it is"
        );
        let (_, withdrawn) = table.take_step();
        assert!(
            withdrawn.is_empty(),
            "a step lists only the effects orphaned since the last one: {withdrawn:?}"
        );
    }

    #[test]
    fn an_effect_orphaned_in_its_own_step_still_goes_out_with_that_step() {
        let mut table = EffectTable::default();
        let effect = issue_timer(&mut table, chain(0));
        table.orphan_all();
        let (issued, _) = table.take_step();
        let ids: Vec<EffectId> = issued.iter().map(|issued| issued.id).collect();
        assert_eq!(ids, [effect], "the caller still owes this effect an answer");
        let (issued, _) = table.take_step();
        assert!(issued.is_empty(), "an effect goes out with one step only");
    }

    #[test]
    fn teardown_orphans_every_awaited_effect_and_withdraws_every_outstanding_one() {
        let mut table = EffectTable::default();
        let earlier = issue_timer(&mut table, chain(2));
        let (call, _) = issue_tool_call(&mut table, chain(0));
        let timer = issue_timer(&mut table, chain(1));
        let answered = issue_timer(&mut table, chain(1));
        assert_eq!(table.orphan(earlier), None);
        assert!(matches!(table.arrive(answered), Arrival::Awaited(_)));
        table.orphan(call);
        table.orphan_all();
        let (_, withdrawn) = table.take_step();
        assert_eq!(
            withdrawn,
            [earlier, call, timer],
            "every effect still owed an answer, an earlier orphan included, once each, in id order"
        );
        assert!(!table.has_awaited());
        assert!(matches!(table.arrive(earlier), Arrival::Orphaned));
        assert!(matches!(table.arrive(call), Arrival::Orphaned));
        assert!(matches!(table.arrive(timer), Arrival::Orphaned));
        assert!(table.all_answered());
    }

    #[test]
    fn a_step_lists_no_withdrawal_already_answered() {
        let mut table = EffectTable::default();
        let (answered, _) = issue_tool_call(&mut table, chain(0));
        let (owed, _) = issue_tool_call(&mut table, chain(0));
        table.orphan(answered);
        table.orphan(owed);
        assert!(matches!(table.arrive(answered), Arrival::Orphaned));
        let (_, withdrawn) = table.take_step();
        assert_eq!(withdrawn, [owed], "the caller already answered {answered}");
    }

    #[test]
    fn awaited_by_lists_one_chains_awaited_effects_in_issue_order() {
        let mut table = EffectTable::default();
        let mut expected = Vec::new();
        for _ in 0..16 {
            expected.push(issue_timer(&mut table, chain(0)));
            issue_timer(&mut table, chain(1));
        }
        let orphan = expected.remove(3);
        table.orphan(orphan);
        assert_eq!(table.awaited_by(chain(0)), expected);
    }

    #[test]
    fn reassigning_moves_only_the_awaited_effects_of_the_one_chain() {
        let mut table = EffectTable::default();
        let moved = issue_timer(&mut table, chain(0));
        let kept = issue_timer(&mut table, chain(1));
        let orphan = issue_timer(&mut table, chain(0));
        table.orphan(orphan);
        table.reassign(chain(0), chain(2));
        assert_eq!(table.awaited_by(chain(2)), [moved]);
        assert!(table.awaited_by(chain(0)).is_empty());
        assert_eq!(table.awaited_by(chain(1)), [kept]);
        assert!(matches!(table.arrive(orphan), Arrival::Orphaned));
    }
}
