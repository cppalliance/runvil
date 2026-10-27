//! Tests for the chain's usage-anchor bookkeeping (what a settled round
//! keeps for the next precheck, what a failed or usage-less round leaves
//! alone, and which model a measurement is offered to) and for its walk
//! stack, one rule each.

use runvil_types::metrics::Usage;

use super::{ChatAnchor, Walk};
use crate::execute::walk_target::JumpTarget;
use crate::heading_address::{SectionIndex, SectionPos};
use crate::lua::UsageAnchor;
use crate::model::{Message, ModelId};

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
