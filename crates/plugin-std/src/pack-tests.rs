//! Unit tests for the `std` Pack's stub text and the definitions it
//! builds from.

use super::{NOT_IMPLEMENTED, STD};
use crate::spec;

#[test]
fn the_pack_is_runvil_std_and_takes_no_per_run_service() {
    assert_eq!(STD.fqn, "runvil/std");
    assert!(STD.prelude.is_none());
    assert!(STD.needs.is_empty());
}

#[test]
fn the_stub_names_the_tool_in_every_place_the_text_mentions_it() {
    let text = NOT_IMPLEMENTED.replace("<name>", "Grep");
    assert_eq!(
        text.matches("Grep").count(),
        2,
        "the text names the tool twice: {text}"
    );
    assert!(!text.contains("<name>"), "{text}");
}

#[test]
fn the_stub_says_the_implementation_is_missing_and_the_result_is_stable() {
    let text = NOT_IMPLEMENTED.replace("<name>", "Write");
    for phrase in [
        "not implemented in this build",
        "The implementation is missing",
        "nothing in this session will change that",
        "Calling Write again is safe",
        "this same result every time",
        "Carry on without it.",
    ] {
        assert!(text.contains(phrase), "missing {phrase:?}: {text}");
    }
}

#[test]
fn the_stub_says_what_is_missing_before_it_says_anything_else() {
    let text = NOT_IMPLEMENTED.replace("<name>", "Read");
    assert!(
        text.starts_with("The Read tool is not implemented"),
        "the first clause says what is missing: {text}"
    );
}

#[test]
fn the_specs_are_the_six_the_crate_names_in_order() {
    let segments: Vec<&str> = spec::all()
        .expect("every embedded definition parses")
        .iter()
        .map(|definition| definition.segment)
        .collect();
    assert_eq!(segments, ["read", "write", "replace", "rm", "glob", "grep"]);
}
