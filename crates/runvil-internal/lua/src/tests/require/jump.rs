//! `jump` and `require`: a `jump` refuses while a library loads, and a
//! library function's `jump` after the load transfers its caller. Each
//! transfer names the line of its `jump` call.

use super::*;

#[test]
fn jump_during_a_load_raises_and_the_refusal_ends_with_the_load() {
    let (vm, _) = require_vm(
        vec![("## a", None, library("a", "## a", &["jump '## elsewhere'"]))],
        &null_emitter(),
    );
    let error = failure(&vm, "require '## a'");
    assert!(
        error.contains("jump is unavailable while a library loads"),
        "jump must refuse during a load, got {error}"
    );
    let step = vm
        .start_block_coro(&program("pcall(require, '## a')\njump '## after'"))
        .expect("the jump after a failed load must transfer");
    assert!(
        matches!(
            step,
            CoroStep::Done(LuaBlockResult::Jump(ref heading, line))
                if heading == "## after" && line == NonZeroU32::new(2)
        ),
        "a jump after the load must transfer from its own line, got {step:?}"
    );
}

#[test]
fn a_library_function_that_jumps_after_the_load_jumps_for_the_caller() {
    let (vm, _) = require_vm(
        vec![(
            "## a",
            None,
            library("a", "## a", &["function go() jump '## next' end"]),
        )],
        &null_emitter(),
    );
    let step = vm
        .start_block_coro(&program("require '## a'\ngo()"))
        .expect("the jump must transfer");
    assert!(
        matches!(
            step,
            CoroStep::Done(LuaBlockResult::Jump(ref heading, line))
                if heading == "## next" && line == NonZeroU32::new(100)
        ),
        "the library function's jump must transfer the caller from the library's line, got \
         {step:?}"
    );
}
