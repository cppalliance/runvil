//! Tests for the chain's usage-anchor bookkeeping (what a settled round
//! keeps for the next precheck, what a failed or usage-less round leaves
//! alone, and which model a measurement is offered to), for its walk
//! stack and its id counters, and for the arena that holds the chains,
//! one rule each.

use std::num::NonZeroUsize;
use std::sync::Arc;

use runvil_types::ids::{ChainId, TaskId};
use runvil_types::metrics::Usage;

use super::{
    Admission, CallDepth, Chain, ChainArena, ChainIndex, ChainKind, ChatAnchor, Counters, Walk,
};
use crate::execute::RunContext;
use crate::execute::context::RunState;
use crate::execute::section_context::TaskSeed;
use crate::execute::walk_target::JumpTarget;
use crate::heading_address::{SectionIndex, SectionPos};
use crate::lua::UsageAnchor;
use crate::model::{Message, ModelId};
use crate::parser::Prompt;
use crate::test_support::scenario::{TEST_SEED, TEST_STARTED_AT};

fn usage(prompt: u32, completion: u32) -> Usage {
    Usage::new(prompt, completion, prompt + completion)
}

fn request(text: &str) -> Vec<Message> {
    vec![Message::user(text)]
}

fn model(name: &str) -> ModelId {
    ModelId::gateway(name).expect("the test model name is valid")
}

#[test]
fn a_chain_starts_with_no_measurement() {
    assert!(ChatAnchor::default().measured(&model("a")).is_none());
}

#[test]
fn a_round_that_reported_usage_becomes_the_measurement() {
    let mut anchor = ChatAnchor::default();
    anchor.sending(model("a"), request("first"));
    anchor.settle(Some(&usage(100, 20)));
    let measured = anchor
        .measured(&model("a"))
        .expect("the round reported usage");
    assert_eq!(measured.tokens(), 120);
}

#[test]
fn the_newest_round_that_reported_usage_replaces_the_measurement() {
    let mut anchor = ChatAnchor::default();
    anchor.sending(model("a"), request("first"));
    anchor.settle(Some(&usage(100, 20)));
    anchor.sending(model("a"), request("second"));
    anchor.settle(Some(&usage(300, 40)));
    assert_eq!(
        anchor.measured(&model("a")).map(UsageAnchor::tokens),
        Some(340)
    );
}

#[test]
fn a_round_with_no_usage_keeps_the_older_measurement() {
    let mut anchor = ChatAnchor::default();
    anchor.sending(model("a"), request("first"));
    anchor.settle(Some(&usage(100, 20)));
    anchor.sending(model("a"), request("second"));
    anchor.settle(None);
    assert_eq!(
        anchor.measured(&model("a")).map(UsageAnchor::tokens),
        Some(120),
        "a round that measured nothing leaves the older measurement"
    );
}

#[test]
fn usage_with_no_round_in_flight_measures_nothing() {
    let mut anchor = ChatAnchor::default();
    anchor.sending(model("a"), request("first"));
    anchor.settle(None);
    // The failed round released its messages, so a late usage has nothing
    // to attach to.
    anchor.settle(Some(&usage(100, 20)));
    assert!(anchor.measured(&model("a")).is_none());
}

#[test]
fn the_measurement_is_offered_only_to_the_model_that_made_it() {
    let mut anchor = ChatAnchor::default();
    anchor.sending(model("a"), request("first"));
    anchor.settle(Some(&usage(100, 20)));
    assert_eq!(
        anchor.measured(&model("a")).map(UsageAnchor::tokens),
        Some(120),
        "an id built again from the same parts is the same model"
    );
    assert!(
        anchor.measured(&model("b")).is_none(),
        "another model's tokenizer counted these tokens"
    );
    let other_server = ModelId::new("elsewhere", "a").expect("the test id is valid");
    assert!(
        anchor.measured(&other_server).is_none(),
        "the same name on another server is another model"
    );
}

#[test]
fn a_round_on_another_model_replaces_the_measurement() {
    let mut anchor = ChatAnchor::default();
    anchor.sending(model("a"), request("first"));
    anchor.settle(Some(&usage(100, 20)));
    anchor.sending(model("b"), request("second"));
    anchor.settle(Some(&usage(300, 40)));
    assert_eq!(
        anchor.measured(&model("b")).map(UsageAnchor::tokens),
        Some(340)
    );
    assert!(
        anchor.measured(&model("a")).is_none(),
        "the old model's measurement is gone once the new one settles"
    );
}

#[test]
fn a_round_on_another_model_with_no_usage_keeps_the_older_measurement() {
    let mut anchor = ChatAnchor::default();
    anchor.sending(model("a"), request("first"));
    anchor.settle(Some(&usage(100, 20)));
    anchor.sending(model("b"), request("second"));
    anchor.settle(None);
    assert_eq!(
        anchor.measured(&model("a")).map(UsageAnchor::tokens),
        Some(120),
        "a round that measured nothing leaves the older measurement"
    );
    assert!(anchor.measured(&model("b")).is_none());
}

fn top(index: usize) -> SectionPos {
    SectionPos::top(SectionIndex::new(index))
}

fn descend(jumper: &SectionPos, index: usize) -> JumpTarget {
    JumpTarget {
        pos: jumper.child(SectionIndex::new(index)),
        descends: true,
    }
}

#[test]
fn a_new_walk_is_at_its_start() {
    assert_eq!(Walk::new(top(2)).current(), &top(2));
}

#[test]
fn advancing_a_walk_moves_to_the_next_sibling_at_the_same_level() {
    let start = top(1).child(SectionIndex::FIRST);
    let mut walk = Walk::new(start.clone());
    walk.advance();
    assert_eq!(walk.current(), &start.next());
    assert_eq!(walk.current(), &top(1).child(SectionIndex::new(1)));
}

#[test]
fn a_walk_descends_on_a_child_jump_and_a_pop_resumes_after_the_jumper() {
    let mut walk = Walk::new(top(1));
    walk.jump(descend(&top(1), 0));
    assert_eq!(walk.current(), &top(1).child(SectionIndex::FIRST));
    assert!(walk.pop(), "a child level pops back to its parent");
    assert_eq!(walk.current(), &top(2));
}

#[test]
fn a_sibling_jump_replaces_the_walks_top_without_adding_a_level() {
    let mut walk = Walk::new(top(0));
    walk.jump(descend(&top(0), 0));
    let sibling = top(0).child(SectionIndex::new(2));
    walk.jump(JumpTarget {
        pos: sibling.clone(),
        descends: false,
    });
    assert_eq!(walk.current(), &sibling);
    assert!(walk.pop(), "the child level is still one level down");
    assert_eq!(walk.current(), &top(1));
    assert!(!walk.pop(), "the sibling jump added no level");
}

#[test]
fn popping_a_walk_at_the_root_level_returns_false_and_keeps_the_position() {
    let mut walk = Walk::new(top(3));
    assert!(!walk.pop());
    assert_eq!(walk.current(), &top(3));
}

#[test]
fn a_walk_unwinds_nested_descents_one_level_per_pop() {
    let mut walk = Walk::new(top(1));
    walk.jump(descend(&top(1), 0));
    walk.advance();
    let jumper = top(1).child(SectionIndex::new(1));
    walk.jump(descend(&jumper, 2));
    assert_eq!(walk.current(), &jumper.child(SectionIndex::new(2)));
    assert!(walk.pop());
    assert_eq!(walk.current(), &jumper.next());
    assert!(walk.pop());
    assert_eq!(walk.current(), &top(2));
    assert!(!walk.pop());
    assert_eq!(walk.current(), &top(2));
}

/// A chain of `kind` under `lineage` that holds no access and has entered
/// no section.
fn chain(lineage: ChainId, kind: ChainKind) -> Chain {
    let source = concat!(
        "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n",
        "# Title\n\n## Only\n\ndone\n",
    );
    let prompt = Prompt::parse(source, "chain-arena-test")
        .0
        .expect("the test prompt parses");
    let ctx = RunState::new(
        Arc::new(prompt),
        "",
        RunContext::new("chain-arena-test", TEST_SEED, TEST_STARTED_AT),
    );
    Chain {
        task: TaskId::from(lineage.clone()),
        lineage,
        counters: Counters::default(),
        kind,
        seed: TaskSeed::default(),
        parked: None,
        ctx,
        access: None,
        entered: None,
        walk: Walk::new(top(0)),
        entered_name: "Only".to_owned(),
        var: serde_json::json!({}),
        call_depth: CallDepth::ROOT,
        concurrency: NonZeroUsize::MIN,
        anchor: ChatAnchor::default(),
    }
}

fn call_of(parent: ChainIndex) -> ChainKind {
    ChainKind::Call {
        parent,
        released_holder: None,
    }
}

#[test]
fn a_chain_hands_out_child_and_entry_ids_from_separate_counters() {
    let lineage = ChainId::root().child(4);
    let mut chain = chain(lineage.clone(), ChainKind::Root);
    assert_eq!(
        chain.next_child_id().expect("a child id is left"),
        lineage.child(0)
    );
    assert_eq!(
        chain.next_entry_id().expect("an entry id is left"),
        lineage.entry(0)
    );
    assert_eq!(
        chain.next_child_id().expect("a child id is left"),
        lineage.child(1)
    );
    assert_eq!(
        chain.next_entry_id().expect("an entry id is left"),
        lineage.entry(1)
    );
}

#[test]
fn each_index_a_push_hands_out_names_the_chain_it_pushed() {
    let mut arena = ChainArena::new();
    let ids: Vec<ChainIndex> = (0..3)
        .map(|n| {
            arena
                .push(chain(ChainId::root().child(n), ChainKind::Root))
                .expect("the arena has room")
        })
        .collect();
    assert_eq!(arena.len(), 3);
    for (n, id) in (0..).zip(&ids) {
        assert_eq!(arena[*id].lineage, ChainId::root().child(n));
    }
    let listed: Vec<ChainIndex> = arena.iter().map(|(id, _)| id).collect();
    assert_eq!(listed, ids, "the arena lists its chains in push order");
}

#[test]
fn a_push_at_the_bound_fails_without_adding_a_chain() {
    let mut arena = ChainArena::new();
    arena.set_max_for_test(1);
    let first = arena
        .push(chain(ChainId::root(), ChainKind::Root))
        .expect("the first chain fits");
    assert!(
        arena
            .push(chain(ChainId::root().child(0), ChainKind::Root))
            .is_err()
    );
    assert_eq!(arena.len(), 1);
    assert_eq!(arena[first].lineage, ChainId::root());
}

#[test]
fn call_children_lists_only_the_parents_own_call_children() {
    let mut arena = ChainArena::new();
    let mut push = |lineage: ChainId, kind: ChainKind| {
        arena
            .push(chain(lineage, kind))
            .expect("the arena has room")
    };
    let root = ChainId::root();
    let walk = push(root.clone(), ChainKind::Root);
    let first = push(root.child(0), call_of(walk));
    push(
        root.child(1),
        ChainKind::Task {
            admission: Admission::Admitted { holding: false },
        },
    );
    let grandchild = push(root.child(0).child(0), call_of(first));
    let second = push(root.child(2), call_of(walk));
    let children: Vec<ChainIndex> = arena.call_children(walk).collect();
    assert_eq!(
        children,
        [first, second],
        "a task and a grandchild are not listed"
    );
    let children: Vec<ChainIndex> = arena.call_children(first).collect();
    assert_eq!(children, [grandchild]);
}
