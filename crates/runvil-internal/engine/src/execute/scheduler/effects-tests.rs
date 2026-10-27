//! Tests for the effect table alone, one rule each: an issued effect takes
//! exactly one answer, an orphan's answer is discarded, unknown ids and
//! second answers are reported, only orphaned tool calls are withdrawn, in
//! id order, and an effect orphaned in its own step still goes out with
//! that step.

use std::time::Duration;

use runvil_types::ids::{ChainId, Provenance, TaskId};
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
    table.issue(chain, provenance(), timer(), Continuation::Timer)
}

/// Issues a script tool call for `chain` under a fresh identity and returns
/// the effect's id with that identity. The table reads only the
/// continuation, so the effect itself is a timer's.
fn issue_tool_call(table: &mut EffectTable, chain: ChainIndex) -> (EffectId, ExecId) {
    let access = VfsRef::default()
        .acquire(Origin::new("effects-test"))
        .expect("the default backend acquires");
    let exec = runvil_vfs::detail::access_id(&access);
    let tool = ToolId::parse("example/test/echo").expect("a fixture id is a tool id");
    let binding = ToolBinding::from_info(
        "echo",
        &ToolInfo::new(tool, "echo", json!({ "type": "object" })),
    );
    let resume = Continuation::ToolCall(ToolCallContinuation {
        binding,
        report: ScriptReport { turn: 0 },
        site: CallSite::Script { turn: None },
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
    assert!(matches!(pending.resume, Continuation::Timer));
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
fn only_orphaned_tool_calls_are_withdrawn_in_id_order() {
    let mut table = EffectTable::default();
    let timer = issue_timer(&mut table, chain(0));
    let (first, first_exec) = issue_tool_call(&mut table, chain(1));
    let (second, second_exec) = issue_tool_call(&mut table, chain(1));
    assert_eq!(table.orphan(second), Some((chain(1), second_exec)));
    assert_eq!(table.orphan(timer), None);
    assert_eq!(table.orphan(first), Some((chain(1), first_exec)));
    let (_, withdrawn) = table.take_step();
    assert_eq!(withdrawn, [first, second]);
    let (_, withdrawn) = table.take_step();
    assert!(
        withdrawn.is_empty(),
        "a step lists only the calls orphaned since the last one: {withdrawn:?}"
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
fn teardown_orphans_every_awaited_effect_and_withdraws_none() {
    let mut table = EffectTable::default();
    let (call, _) = issue_tool_call(&mut table, chain(0));
    let timer = issue_timer(&mut table, chain(1));
    table.orphan_all();
    let (_, withdrawn) = table.take_step();
    assert!(withdrawn.is_empty(), "{withdrawn:?}");
    assert!(!table.has_awaited());
    assert!(matches!(table.arrive(call), Arrival::Orphaned));
    assert!(matches!(table.arrive(timer), Arrival::Orphaned));
    assert!(table.all_answered());
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
