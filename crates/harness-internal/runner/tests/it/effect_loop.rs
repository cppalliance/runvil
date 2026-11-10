//! The effect loop against fake performers and an in-memory recorder: the
//! record stream is events, then effects, then answers per step; a cancel
//! drops every outstanding effect with one `Dropped` answer each; a
//! performer that panics drops its effect rather than stranding the run;
//! a run left with nothing to await ends recorded with an `Internal`
//! error, unless its main chain waits for input, which only a cancel ends
//! here; and a refused recorder write ends the drive with the recorder's
//! error and tears down the performers still out. The Vfs effect the loop
//! answers inline has its own module, `vfs`, the `Chat` effect the
//! inference broker answers has `broker`, and the tool-call cap has `cap`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use harness_runner::display_chain;
use harness_runner::effect_loop::drive_run;
use harness_runner::recorder::{MemoryRecorder, Record, RecordKind, RunId, RunMeta, RunRecorder};
use runvil_engine::Step;
use runvil_engine::cancel::CancelHandle;
use runvil_engine::vfs::{MemoryBackend, Origin, VfsRef};
use runvil_engine::{Error, ErrorCode, Timestamp};
use serde_json::json;

use crate::support::{
    ClosingTool, PanickingTool, PendingTimer, PendingTool, TEST_SEED, TIMED_MAIN, TextTool, Unused,
    WAITS, assert_canceled, loop_setup, run, run_over, run_with_cancel, run_with_child,
    run_with_child_and_cancel,
};

#[path = "effect_loop-broker.rs"]
mod broker;
#[path = "effect_loop-cap.rs"]
mod cap;
#[path = "effect_loop-recorder.rs"]
mod recorder_failures;
#[path = "effect_loop-vfs.rs"]
mod vfs;

/// A run's opening metadata; the loop ends the run.
fn meta() -> RunMeta {
    RunMeta {
        seed: TEST_SEED,
        flags: 0,
        started_at: Timestamp::UNIX_EPOCH,
    }
}

/// An in-memory recorder with one run begun in it.
async fn begun_log() -> (Arc<MemoryRecorder>, RunId) {
    let recorder = Arc::new(MemoryRecorder::new());
    let run_id = recorder.begin_run(meta()).await.unwrap();
    (recorder, run_id)
}

/// Fires `cancel` from another thread after `delay`: the Host's cancel
/// arriving while the loop waits, from outside the run's own future.
fn cancel_after(cancel: &CancelHandle, delay: Duration) {
    let trigger = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(delay);
        trigger.cancel();
    });
}

/// The kinds of `records`, in order.
fn kinds(records: &[Record]) -> Vec<RecordKind> {
    records.iter().map(|record| record.kind).collect()
}

/// Asserts every effect record has exactly one answer record, that the
/// answer comes after its effect, and that the two share one provenance.
fn assert_one_answer_per_effect(records: &[Record]) {
    let effects: Vec<(usize, &Record)> = records
        .iter()
        .enumerate()
        .filter(|(_, record)| record.kind == RecordKind::Effect)
        .collect();
    let answers: Vec<(usize, &Record)> = records
        .iter()
        .enumerate()
        .filter(|(_, record)| record.kind == RecordKind::Answer)
        .collect();
    assert_eq!(effects.len(), answers.len(), "one answer per effect");
    for (position, effect) in effects {
        let id = effect.effect_id.expect("an effect record names its id");
        let matching: Vec<&(usize, &Record)> = answers
            .iter()
            .filter(|(_, answer)| answer.effect_id == Some(id))
            .collect();
        assert_eq!(matching.len(), 1, "effect {id} has exactly one answer");
        let (answer_position, answer) = *matching[0];
        assert!(answer_position > position, "the answer follows its effect");
        assert_eq!(answer.task_id, effect.task_id);
        assert_eq!(answer.task_seq, effect.task_seq);
    }
}

#[tokio::test]
async fn records_are_events_then_effects_then_answers_per_step() {
    let (recorder, run_id) = begun_log().await;
    let vfs = VfsRef::builder().store("/", MemoryBackend::new()).build();

    let result = drive_run(
        run_over(
            &format!("store.write('notes.md', 'kept')\n{WAITS}"),
            vfs.clone(),
        ),
        &loop_setup(recorder.clone()),
        Arc::new(Unused),
        Arc::new(TextTool("hi")),
        run_id,
        CancelHandle::new(),
    )
    .await;
    assert_eq!(result, Ok("hi".to_owned()));
    let store = vfs.acquire_store(Origin::new("effect loop test")).unwrap();
    assert_eq!(
        store.read("notes.md").unwrap(),
        b"kept",
        "the inline answer performed the write on the run's store"
    );

    let records = recorder.records(run_id);
    let kinds = kinds(&records);
    // The run opens with its own events before the first effect; each
    // effect is followed by its answer before the next step's events.
    assert_eq!(kinds[0], RecordKind::Event, "a step's events come first");
    let effect_positions: Vec<usize> = kinds
        .iter()
        .enumerate()
        .filter(|(_, kind)| **kind == RecordKind::Effect)
        .map(|(position, _)| position)
        .collect();
    assert_eq!(effect_positions.len(), 2, "one store effect, one tool call");
    for position in &effect_positions {
        assert_eq!(
            kinds[position + 1],
            RecordKind::Answer,
            "a serial run's answer follows its effect"
        );
    }
    assert_eq!(
        *kinds.last().unwrap(),
        RecordKind::Event,
        "the run's end is an event"
    );
    assert_one_answer_per_effect(&records);

    let payload = |position: usize| records[position].payload.clone();
    assert_eq!(
        payload(effect_positions[0]),
        json!({ "Vfs": { "op": { "Write": { "path": "notes.md", "contents": "kept" } } } })
    );
    assert_eq!(
        payload(effect_positions[0] + 1),
        json!({ "Vfs": { "Ok": "Unit" } })
    );
    assert_eq!(
        payload(effect_positions[1]),
        json!({ "ToolCall": {
            "tool": "tests/runner/wait",
            "alias": "tests/runner/wait",
            "args": {},
            "origin": { "section": "Only", "caller": "script", "entry": "0.1" }
        } })
    );
    assert_eq!(
        payload(effect_positions[1] + 1),
        json!({ "ToolCall": { "Ok": { "text": "hi", "trust": "trusted" } } })
    );

    // The run ended once, with the loop's result.
    assert_eq!(recorder.result(run_id), Some(result));
}

/// The answer records of `records`, in loop order.
fn answers(records: &[Record]) -> Vec<&Record> {
    records
        .iter()
        .filter(|record| record.kind == RecordKind::Answer)
        .collect()
}

/// Waits until `flag` is raised, or fails after a bounded wait. Under
/// paused time each sleep is a yield that advances the clock, so the wait
/// costs no wall time.
async fn await_raised(flag: &AtomicBool, what: &str) {
    for _ in 0..200 {
        if flag.load(Ordering::SeqCst) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("{what} within a second");
}

#[tokio::test]
async fn a_cancel_writes_one_dropped_answer_per_outstanding_effect() {
    let (recorder, run_id) = begun_log().await;
    let timer_dropped = Arc::new(AtomicBool::new(false));
    let timer = Arc::new(PendingTimer {
        dropped: Arc::clone(&timer_dropped),
    });
    let cancel = CancelHandle::new();
    cancel_after(&cancel, Duration::from_millis(50));

    // Two effects are out when the cancel lands: the main section's
    // timeout timer and its child's tool call.
    let result = drive_run(
        run_with_child_and_cancel(TIMED_MAIN, WAITS, &cancel),
        &loop_setup(recorder.clone()),
        timer,
        Arc::new(PendingTool),
        run_id,
        cancel,
    )
    .await;
    assert_canceled(&result);

    let records = recorder.records(run_id);
    assert_one_answer_per_effect(&records);
    let effects: Vec<serde_json::Value> = records
        .iter()
        .filter(|record| record.kind == RecordKind::Effect)
        .map(|record| record.payload.clone())
        .collect();
    assert_eq!(
        effects,
        vec![
            json!({ "Timer": { "seconds": 30.0 } }),
            json!({ "ToolCall": {
                "tool": "tests/runner/wait",
                "alias": "tests/runner/wait",
                "args": {},
                "origin": { "section": "Child", "caller": "script", "entry": "0.0.0" }
            } }),
        ],
        "the timer and the child's tool call are the two effects out"
    );
    let answers = answers(&records);
    assert_eq!(answers.len(), 2, "one drop per outstanding effect");
    for answer in &answers {
        assert_eq!(answer.payload, json!("Dropped"));
    }
    assert_ne!(
        answers[0].effect_id, answers[1].effect_id,
        "each drop answers its own effect"
    );
    assert!(
        timer_dropped.load(Ordering::SeqCst),
        "the parked timer's performer was aborted and joined before the run ended"
    );
    assert_eq!(recorder.result(run_id), Some(result));
}

#[tokio::test]
async fn a_panicking_performer_drops_its_effect_instead_of_stranding_the_run() {
    let (recorder, run_id) = begun_log().await;

    // No cancel fires: only the lost performer's own drop can end the
    // wait, so a loop that never hears from it hangs here.
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        drive_run(
            run(WAITS),
            &loop_setup(recorder.clone()),
            Arc::new(Unused),
            Arc::new(PanickingTool),
            run_id,
            CancelHandle::new(),
        ),
    )
    .await
    .expect("a lost performer does not strand the loop");
    assert!(
        result.as_ref().is_err_and(Error::is_canceled),
        "the chain resumed with the canceled error of a dropped effect: {result:?}"
    );

    let records = recorder.records(run_id);
    assert_one_answer_per_effect(&records);
    let answers = answers(&records);
    assert_eq!(answers.len(), 1, "the one panicked tool call");
    assert_eq!(answers[0].payload, json!("Dropped"));
    assert_eq!(recorder.result(run_id), Some(result));
}

#[tokio::test(start_paused = true)]
async fn a_refused_recorder_write_returns_the_recorder_error_and_aborts_the_parked_performers() {
    let (recorder, run_id) = begun_log().await;
    let timer_dropped = Arc::new(AtomicBool::new(false));
    let tool = Arc::new(ClosingTool {
        recorder: Arc::clone(&recorder),
        run_id,
    });
    let timer = Arc::new(PendingTimer {
        dropped: Arc::clone(&timer_dropped),
    });

    // The child's tool performer ends the run at the recorder before it
    // answers, so recording its answer is the loop's first refused write;
    // the main section's timer is still parked at that moment.
    let error = drive_run(
        run_with_child(TIMED_MAIN, WAITS),
        &loop_setup(recorder.clone()),
        timer,
        tool,
        run_id,
        CancelHandle::new(),
    )
    .await
    .expect_err("a refused write ends the drive");
    assert_eq!(
        error.code,
        ErrorCode::RecorderFailed,
        "a refusal: {error:?}"
    );
    assert!(
        display_chain(&error).contains(&format!("run {run_id} has ended")),
        "the chain names the recorder's reason: {}",
        display_chain(&error)
    );

    let records = recorder.records(run_id);
    assert_eq!(
        records
            .iter()
            .filter(|record| record.kind == RecordKind::Effect)
            .count(),
        2,
        "both effects were recorded before the run ended"
    );
    assert!(
        answers(&records).is_empty(),
        "the refused answer was not recorded, and nothing after it"
    );
    await_raised(
        &timer_dropped,
        "the parked timer's performer is aborted when the driver is dropped",
    )
    .await;
}

#[tokio::test]
async fn an_ended_run_refuses_the_first_write_before_any_performer_starts() {
    let (recorder, run_id) = begun_log().await;
    recorder
        .end_run(run_id, Err(Error::canceled()))
        .await
        .unwrap();

    // The run's opening events are the first write; nothing is issued
    // after a refused write, so the unused performers are never reached.
    let error = drive_run(
        run(WAITS),
        &loop_setup(recorder.clone()),
        Arc::new(Unused),
        Arc::new(PendingTool),
        run_id,
        CancelHandle::new(),
    )
    .await
    .expect_err("an ended run refuses its first write");
    assert_eq!(
        error.code,
        ErrorCode::RecorderFailed,
        "a refusal: {error:?}"
    );
    assert!(
        display_chain(&error).contains(&format!("run {run_id} has ended")),
        "the chain names the recorder's reason: {}",
        display_chain(&error)
    );
    assert!(recorder.records(run_id).is_empty());
}

#[tokio::test]
async fn a_run_left_with_nothing_to_await_ends_recorded_with_an_internal_error() {
    let (recorder, run_id) = begun_log().await;
    // The test steps the run once and keeps its tool call from the loop:
    // the run waits on an effect the loop never saw, the fault a lost
    // effect would leave.
    let mut lost = run(WAITS);
    let Step::Pending { effects, .. } = lost.step() else {
        panic!("the run parks on its tool call");
    };
    assert_eq!(effects.len(), 1, "the one tool call the loop never sees");

    let result = drive_run(
        lost,
        &loop_setup(recorder.clone()),
        Arc::new(Unused),
        Arc::new(Unused),
        run_id,
        CancelHandle::new(),
    )
    .await;
    let error = result.as_ref().expect_err("a stall ends the run");
    assert_eq!(error.code, ErrorCode::Internal);
    assert_eq!(
        error.located_message(),
        "the effect loop has nothing to await for a pending run"
    );
    assert_eq!(
        recorder.result(run_id),
        Some(result.clone()),
        "the recorder ends the stalled run with its result"
    );
    assert!(
        recorder.records(run_id).is_empty(),
        "the stalled step reported nothing and issued nothing"
    );
}

#[tokio::test]
async fn a_run_waiting_for_input_with_nothing_in_flight_waits_for_a_cancel_instead_of_stalling() {
    let (recorder, run_id) = begun_log().await;
    let cancel = CancelHandle::new();
    cancel_after(&cancel, Duration::from_millis(50));

    // Nothing can send to a run `drive_run` drives, so only the cancel
    // ends the main chain's wait on its inbox.
    let result = drive_run(
        run_with_cancel("return tasks.receive()", &cancel),
        &loop_setup(recorder.clone()),
        Arc::new(Unused),
        Arc::new(Unused),
        run_id,
        cancel,
    )
    .await;
    assert_canceled(&result);
    assert_eq!(recorder.result(run_id), Some(result));
    let records = recorder.records(run_id);
    assert!(
        records
            .iter()
            .any(|record| record.task_id == "0" && record.payload["kind"] == "inbox_waiting"),
        "the main chain waited on its inbox: {records:?}"
    );
    assert!(
        !kinds(&records).contains(&RecordKind::Effect),
        "the wait issued nothing: {records:?}"
    );
}
