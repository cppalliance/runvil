//! The retained typed error: a block coroutine's failure becomes the
//! answer's own error only when the block raised that answer's error table,
//! placed on the line of the call the answer resumed.

use runvil_types::error::SourceLoc;

use super::*;
use crate::test_vm::{TEST_SECTION, TestVm};
use crate::tests::recording::null_emitter;
use crate::{CoroStep, LuaProgram};

/// The typed error every case answers the block's `models.infer` with.
fn quota() -> runvil_types::error::Error {
    crate::lua_quota(crate::QuotaResource::LogEvent)
}

/// [`quota`] as it crossed into the block's VM, which names no file, at
/// the call on `line`: placed there, with the author prose of its message.
fn answered_quota(line: u32) -> runvil_types::error::Error {
    let mut quota = quota();
    quota.prose = format!(
        "{}. {}",
        quota.message,
        runvil_types::error::ErrorCode::LuaQuota.prose()
    );
    quota.at = NonZeroU32::new(line).map(|line| SourceLoc::line("prompt", line));
    quota
}

/// Runs `source` on a shim VM to its `models.infer` yield, answers the
/// yield with [`quota`], and returns the call line the yield recorded
/// beside how the block ended.
fn answered(
    source: &str,
) -> (
    Option<NonZeroU32>,
    std::result::Result<CoroStep, runvil_types::error::Error>,
) {
    let vm = TestVm::new().shims().build();
    let program = LuaProgram::compile(
        source,
        "prompt",
        "test program",
        NonZeroU32::MIN,
        &null_emitter(),
        TEST_SECTION,
    )
    .expect("the block compiles");
    let CoroStep::Yielded(thread, _values) =
        vm.start_block_coro(&program).expect("the block suspends")
    else {
        panic!("the infer shim must suspend the block");
    };
    let call_line = vm.call_line();
    let ended = vm.resume_block_coro_answer(&program, &thread, Err(quota()));
    drop(thread);
    vm.teardown();
    (call_line, ended)
}

#[test]
fn a_block_raising_the_answered_error_table_surfaces_the_typed_error() {
    for source in [
        "models.infer('hi')",
        "local ok, err = pcall(models.infer, 'hi')\nerror(err, 0)",
    ] {
        match answered(source) {
            (_, Err(error)) if error == answered_quota(1) => {}
            other => panic!("block {source:?} must surface the typed quota error, got {other:?}"),
        }
    }
}

#[test]
fn a_block_raising_the_answered_errors_text_as_its_own_keeps_its_own_error() {
    // The raise carries the same text as the answered error, but it is the
    // block's own string, not the answer's table.
    match answered("local ok, err = pcall(models.infer, 'hi')\nerror(tostring(err), 0)") {
        (_, Err(error)) if error.code == runvil_types::error::ErrorCode::LuaRuntime => assert!(
            error.message.contains("lua log event quota exceeded"),
            "the block's own raise keeps its message: {}",
            error.message
        ),
        other => panic!("the block's own raise must stay its own error, got {other:?}"),
    }
}

#[test]
fn a_main_chunk_tail_call_keeps_its_line_for_the_call_and_its_answer() {
    let (call_line, ended) = answered("local prompt = 'hi'\nreturn models.infer(prompt)");
    assert_eq!(
        call_line,
        NonZeroU32::new(2),
        "the yield finds the returning line"
    );
    match ended {
        Err(error) => assert_eq!(error, answered_quota(2)),
        other => panic!("the block must fail with the answered error, got {other:?}"),
    }
}
