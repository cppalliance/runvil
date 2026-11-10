//! A recorder that refuses a write ends the drive: whichever call it
//! refuses, the loop returns the refusal and sends the recorder nothing
//! more; a recorder error under another code is still a `RecorderFailed`
//! refusal; and a refusal mid-run aborts the performers still out.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use harness_runner::display_chain;
use harness_runner::effect_loop::drive_run;
use harness_runner::recorder::{MemoryRecorder, RecordKind, RunRecorder};
use runvil_engine::cancel::CancelHandle;
use runvil_engine::vfs::VfsRef;
use runvil_engine::{ErrorCode, ErrorFact};
use serde_json::json;

use super::{answers, await_raised, cancel_after, meta};
use crate::prepare::RefusingAppends;
use crate::support::{
    FailingRecorder, PendingTimer, TIMED_MAIN, TextTool, Unused, WAITS, loop_setup, run_over,
    run_with_child, run_with_child_and_cancel,
};

/// A run of one store write and one read, over a fresh memory store: its
/// every effect is a Vfs effect, so no performer is ever out.
fn store_round_trip() -> runvil_engine::Run {
    run_over(
        "store.write('a.md', '1')\nreturn store.read('a.md')",
        VfsRef::default(),
    )
}

#[tokio::test]
async fn a_recorder_that_refuses_its_nth_call_ends_the_drive_with_a_recorder_error() {
    // The run's whole call count, from a recorder that refuses nothing.
    let baseline = Arc::new(FailingRecorder::never_failing());
    let run_id = baseline.inner().begin_run(meta()).await.unwrap();
    drive_run(
        store_round_trip(),
        &loop_setup(baseline.clone()),
        Arc::new(Unused),
        Arc::new(Unused),
        run_id,
        CancelHandle::new(),
    )
    .await
    .unwrap();
    let total = baseline.calls();
    assert!(total >= 5, "events, two effects, two answers, and the end");

    for call in 1..=total {
        let recorder = Arc::new(FailingRecorder::failing_on(call));
        let run_id = recorder.inner().begin_run(meta()).await.unwrap();
        let error = drive_run(
            store_round_trip(),
            &loop_setup(recorder.clone()),
            Arc::new(Unused),
            Arc::new(Unused),
            run_id,
            CancelHandle::new(),
        )
        .await
        .expect_err("a refused call ends the drive");

        assert_eq!(
            error.code,
            ErrorCode::RecorderFailed,
            "call {call} of {total}: a refusal, not the run's result"
        );
        assert!(
            display_chain(&error).contains(&format!("refuses call {call}")),
            "call {call} of {total}: {}",
            display_chain(&error)
        );
        assert_eq!(
            recorder.calls(),
            call,
            "the loop sends nothing after the refused call"
        );
        assert_eq!(
            recorder.inner().records(run_id).len(),
            call - 1,
            "the refused call recorded nothing, and every call before it was a record"
        );
        assert_eq!(
            recorder.inner().result(run_id),
            None,
            "an abandoned run is not ended at the recorder"
        );
    }
}

#[tokio::test]
async fn a_recorder_error_under_another_code_ends_the_drive_as_a_recorder_failure() {
    // Every append fails with the recorder's own `Io` error, so the run's
    // first write is refused.
    let recorder = Arc::new(RefusingAppends(MemoryRecorder::new()));
    let run_id = recorder.begin_run(meta()).await.unwrap();
    let error = drive_run(
        store_round_trip(),
        &loop_setup(recorder.clone()),
        Arc::new(Unused),
        Arc::new(Unused),
        run_id,
        CancelHandle::new(),
    )
    .await
    .expect_err("the refused append ends the drive");

    assert_eq!(
        error.code,
        ErrorCode::RecorderFailed,
        "the recorder's `Io` error becomes a refusal, never the run's result"
    );
    assert_eq!(error.message, "the run recorder failed: the disk is full");
    assert_eq!(error.facts.get(&ErrorFact::Run), Some(&json!(run_id.get())));
    assert_eq!(
        recorder.0.result(run_id),
        None,
        "an abandoned run is not ended at the recorder"
    );
}

#[tokio::test]
async fn a_recorder_that_refuses_an_answer_aborts_the_performers_still_out() {
    // Two effects are out in the first step, a timer that never fires and
    // a tool call that answers at once. A run canceled after the answer
    // lands shows which call records it: the first answer in the stream.
    let dry = Arc::new(FailingRecorder::never_failing());
    let dry_run = dry.inner().begin_run(meta()).await.unwrap();
    let timer = Arc::new(PendingTimer {
        dropped: Arc::new(AtomicBool::new(false)),
    });
    let cancel = CancelHandle::new();
    cancel_after(&cancel, Duration::from_millis(50));
    let dry_result = drive_run(
        run_with_child_and_cancel(TIMED_MAIN, WAITS, &cancel),
        &loop_setup(dry.clone()),
        timer,
        Arc::new(TextTool("hi")),
        dry_run,
        cancel,
    )
    .await;
    assert_ne!(
        dry_result.map_err(|error| error.code),
        Err(ErrorCode::RecorderFailed),
        "the dry run is not refused"
    );
    let first_answer = dry
        .inner()
        .records(dry_run)
        .iter()
        .position(|record| record.kind == RecordKind::Answer)
        .expect("the tool call is answered");

    // The same run, with the recorder refusing exactly that call: the
    // timer's performer is still out when the drive fails.
    let timer_dropped = Arc::new(AtomicBool::new(false));
    let timer = Arc::new(PendingTimer {
        dropped: Arc::clone(&timer_dropped),
    });
    let recorder = Arc::new(FailingRecorder::failing_on(first_answer + 1));
    let run_id = recorder.inner().begin_run(meta()).await.unwrap();
    let error = drive_run(
        run_with_child(TIMED_MAIN, WAITS),
        &loop_setup(recorder.clone()),
        timer,
        Arc::new(TextTool("hi")),
        run_id,
        CancelHandle::new(),
    )
    .await
    .expect_err("the refused answer ends the drive");

    assert_eq!(
        error.code,
        ErrorCode::RecorderFailed,
        "a refusal: {error:?}"
    );
    assert!(
        display_chain(&error).contains(&format!("refuses call {}", first_answer + 1)),
        "the chain names the recorder's reason: {}",
        display_chain(&error)
    );
    assert!(
        answers(&recorder.inner().records(run_id)).is_empty(),
        "the refused answer was not recorded"
    );
    await_raised(
        &timer_dropped,
        "the timer's performer is aborted when the drive fails",
    )
    .await;
}
