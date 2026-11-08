//! `TestCall`: the context a Plugin crate's own test lends a call it makes
//! directly.

use runvil_plugin::testing::TestCall;
use runvil_plugin::{ToolCaller, ToolId};

fn inspect() -> ToolId {
    ToolId::parse("acme/inspect").expect("a two-segment tool id parses")
}

#[test]
fn a_test_call_lends_its_tool_a_script_origin_and_a_working_filesystem() {
    let call = TestCall::new(inspect());
    let cx = call.context();
    assert_eq!(cx.tool(), &inspect());
    assert_eq!(cx.origin().caller, ToolCaller::Script);
    cx.access()
        .write("notes.md", b"hello")
        .expect("the in-memory filesystem accepts a write");
    assert_eq!(
        cx.access()
            .read("notes.md")
            .expect("the written file reads back"),
        b"hello"
    );
}

#[test]
fn a_test_call_with_a_caller_lends_that_caller() {
    let call = TestCall::new(inspect()).with_caller(ToolCaller::Model);
    let cx = call.context();
    assert_eq!(cx.tool(), &inspect());
    assert_eq!(cx.origin().caller, ToolCaller::Model);
    assert_eq!(cx.origin().section, "Test");
}

#[test]
fn each_test_call_has_a_filesystem_of_its_own() {
    let first = TestCall::new(inspect());
    first
        .context()
        .access()
        .write("left.txt", b"1")
        .expect("the in-memory filesystem accepts a write");
    let second = TestCall::new(inspect());
    assert!(
        !second
            .context()
            .access()
            .exists("left.txt")
            .expect("an existence check succeeds"),
        "a second call starts from an empty filesystem"
    );
}
