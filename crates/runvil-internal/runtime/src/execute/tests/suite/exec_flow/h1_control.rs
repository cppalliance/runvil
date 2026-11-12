//! The control globals from H1: `call`, `jump`, and `fanout` naming an
//! unknown section fail the run.

use super::*;

/// Runs a prompt whose only block is the H1 `lua` and returns the run
/// failure's message.
#[track_caller]
fn h1_failure(lua: &str) -> String {
    let source = PromptSource::new().h1(lua).build();
    Scenario::new(source).run().error().to_string()
}

/// What the H1 searched for a name no top-level section has.
const NO_SUCH_SECTION: &str = "section `Nope` not found: the top level has no section `Nope`";

/// The control globals work in H1 (section 0), where a name is a
/// top-level section: a `call` naming none fails the run with the
/// resolution error.
#[test]
fn call_from_h1_to_an_unknown_section_fails_the_run() {
    let rendered = h1_failure("call('Nope')");
    assert!(
        rendered.contains(NO_SUCH_SECTION),
        "the resolution error says where it looked: {rendered}"
    );
}

/// `jump` out of H1 names a top-level section; an unknown target fails the
/// run with the resolution error.
#[test]
fn jump_from_h1_to_an_unknown_section_fails_the_run() {
    let rendered = h1_failure("jump('Nope')");
    assert!(
        rendered.contains(NO_SUCH_SECTION),
        "the resolution error says where it looked: {rendered}"
    );
}

/// `fanout` from H1 resolves its worker against the top-level sections; an
/// unknown worker fails the run with the resolution error.
#[test]
fn fanout_from_h1_to_an_unknown_section_fails_the_run() {
    let rendered = h1_failure("tasks.fanout('Nope', {'a'})");
    assert!(
        rendered.contains(NO_SUCH_SECTION),
        "the resolution error says where it looked: {rendered}"
    );
}

/// The H1 has no siblings, so `../name` names nothing there.
#[test]
fn a_sibling_path_from_h1_names_nothing() {
    let rendered = h1_failure("jump('../Nope')");
    assert!(
        rendered.contains("section `../Nope` not found: the H1 has no siblings"),
        "the resolution error says where it looked: {rendered}"
    );
}
