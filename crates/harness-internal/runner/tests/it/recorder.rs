//! `MemoryRecorder` keeps each run's records apart and in order, and it
//! refuses a write to a run that is unknown or already ended. The record
//! types keep the public shape a durable recorder builds on.

use std::error::Error;
use std::fmt;
use std::sync::Arc;

use harness_runner::display_chain;
use harness_runner::recorder::{
    MemoryRecorder, Record, RecordKind, RunId, RunMeta, RunRecorder, make_recorder_error,
};
use runvil_engine::{ErrorCode, ErrorFact, Timestamp};
use serde_json::json;

use crate::support::TEST_SEED;

/// The result of a canceled run.
fn canceled() -> Result<String, runvil_engine::Error> {
    Err(runvil_engine::Error::canceled())
}

/// Asserts `error` is the memory recorder's refusal of run `run` for
/// `reason`.
fn assert_refused(error: &runvil_engine::Error, run: i64, reason: &str) {
    assert_eq!(error.code, ErrorCode::RecorderFailed);
    assert_eq!(error.facts.get(&ErrorFact::Reason), Some(&json!(reason)));
    assert_eq!(error.facts.get(&ErrorFact::Run), Some(&json!(run)));
}

fn meta() -> RunMeta {
    RunMeta {
        seed: TEST_SEED,
        flags: 0,
        started_at: Timestamp::from_unix_millis(1_700_000_000_000),
    }
}

fn record(kind: RecordKind, task_seq: u32, label: &str) -> Record {
    Record {
        task_id: "0".to_owned(),
        task_seq,
        kind,
        effect_id: (kind != RecordKind::Event).then_some(u64::from(task_seq)),
        payload: json!({ "label": label }),
    }
}

fn labels(records: &[Record]) -> Vec<String> {
    records
        .iter()
        .map(|record| {
            record.payload["label"]
                .as_str()
                .expect("every fixture record carries a label")
                .to_owned()
        })
        .collect()
}

#[tokio::test]
async fn records_come_back_in_append_order() {
    let recorder = MemoryRecorder::new();
    let run = recorder.begin_run(meta()).await.unwrap();

    let appended = [
        record(RecordKind::Event, 0, "started"),
        record(RecordKind::Effect, 1, "chat issued"),
        record(RecordKind::Answer, 1, "chat answered"),
        record(RecordKind::Event, 2, "finished"),
    ];
    for item in &appended {
        recorder.append(run, item.clone()).await.unwrap();
    }

    assert_eq!(
        recorder.records(run),
        appended,
        "the records read back whole and in the order they were appended"
    );
}

#[tokio::test]
async fn two_interleaved_runs_keep_their_own_records() {
    let recorder = MemoryRecorder::new();
    let first = recorder.begin_run(meta()).await.unwrap();
    let second = recorder.begin_run(meta()).await.unwrap();

    recorder
        .append(first, record(RecordKind::Event, 0, "first-a"))
        .await
        .unwrap();
    recorder
        .append(second, record(RecordKind::Event, 0, "second-a"))
        .await
        .unwrap();
    let (one, two) = tokio::join!(
        recorder.append(first, record(RecordKind::Effect, 1, "first-b")),
        recorder.append(second, record(RecordKind::Effect, 1, "second-b")),
    );
    one.unwrap();
    two.unwrap();
    recorder
        .append(first, record(RecordKind::Answer, 1, "first-c"))
        .await
        .unwrap();

    assert_eq!(
        labels(&recorder.records(first)),
        ["first-a", "first-b", "first-c"]
    );
    assert_eq!(labels(&recorder.records(second)), ["second-a", "second-b"]);
}

#[tokio::test]
async fn run_ids_start_at_one_and_are_distinct() {
    let recorder = MemoryRecorder::new();
    let first = recorder.begin_run(meta()).await.unwrap();
    let second = recorder.begin_run(meta()).await.unwrap();
    let third = recorder.begin_run(meta()).await.unwrap();

    assert_eq!(first, RunId::from_raw(1), "the first run is run 1");
    assert_eq!(second, RunId::from_raw(2));
    assert_eq!(third, RunId::from_raw(3));
}

#[tokio::test]
async fn a_run_keeps_the_meta_it_began_with() {
    let recorder = MemoryRecorder::new();
    let run = recorder.begin_run(meta()).await.unwrap();

    assert_eq!(recorder.meta(run), Some(meta()));
    assert_eq!(recorder.meta(RunId::from_raw(99)), None);
}

#[tokio::test]
async fn result_is_none_while_a_run_is_open_and_set_when_it_ends() {
    let recorder = MemoryRecorder::new();
    let run = recorder.begin_run(meta()).await.unwrap();
    assert_eq!(recorder.result(run), None, "a fresh run is open");

    recorder
        .append(run, record(RecordKind::Event, 0, "started"))
        .await
        .unwrap();
    assert_eq!(
        recorder.result(run),
        None,
        "a run with records is still open"
    );

    recorder.end_run(run, Ok("done".to_owned())).await.unwrap();
    assert_eq!(recorder.result(run), Some(Ok("done".to_owned())));
}

#[tokio::test]
async fn ending_one_run_leaves_the_other_open() {
    let recorder = MemoryRecorder::new();
    let first = recorder.begin_run(meta()).await.unwrap();
    let second = recorder.begin_run(meta()).await.unwrap();

    recorder.end_run(first, canceled()).await.unwrap();

    assert_eq!(recorder.result(first), Some(canceled()));
    assert_eq!(recorder.result(second), None);
    recorder
        .append(second, record(RecordKind::Event, 0, "still open"))
        .await
        .expect("the second run still takes records");
}

#[tokio::test]
async fn an_unknown_run_has_no_records_and_no_result() {
    let recorder = MemoryRecorder::new();
    let unknown = RunId::from_raw(41);

    assert!(recorder.records(unknown).is_empty());
    assert_eq!(recorder.result(unknown), None);
}

#[tokio::test]
async fn an_append_to_an_unknown_run_fails() {
    let recorder = MemoryRecorder::new();
    recorder.begin_run(meta()).await.unwrap();

    let error = recorder
        .append(RunId::from_raw(99), record(RecordKind::Event, 0, "lost"))
        .await
        .expect_err("run 99 was never begun");

    assert_refused(&error, 99, "unknown_run");
    let text = display_chain(&error);
    assert!(
        text.contains("unknown run 99"),
        "the failure names the unknown run: {text}"
    );
}

#[tokio::test]
async fn ending_an_unknown_run_fails() {
    let recorder = MemoryRecorder::new();

    let error = recorder
        .end_run(RunId::from_raw(5), canceled())
        .await
        .expect_err("run 5 was never begun");

    assert_refused(&error, 5, "unknown_run");
    let text = display_chain(&error);
    assert!(
        text.contains("unknown run 5"),
        "the failure names the unknown run: {text}"
    );
}

#[tokio::test]
async fn an_append_after_end_run_fails_and_changes_nothing() {
    let recorder = MemoryRecorder::new();
    let run = recorder.begin_run(meta()).await.unwrap();
    recorder
        .append(run, record(RecordKind::Event, 0, "before"))
        .await
        .unwrap();
    recorder.end_run(run, canceled()).await.unwrap();

    let error = recorder
        .append(run, record(RecordKind::Event, 1, "after"))
        .await
        .expect_err("an ended run accepts no more records");

    assert_refused(&error, 1, "run_ended");
    let text = display_chain(&error);
    assert!(
        text.contains("run 1 has ended"),
        "the failure says the run ended: {text}"
    );
    assert_eq!(
        labels(&recorder.records(run)),
        ["before"],
        "the refused record is not kept"
    );
}

#[tokio::test]
async fn a_second_end_run_fails_and_keeps_the_first_result() {
    let recorder = MemoryRecorder::new();
    let run = recorder.begin_run(meta()).await.unwrap();
    recorder.end_run(run, canceled()).await.unwrap();

    let code = ErrorCode::Internal;
    let second = runvil_engine::Error::new(code, "a second end", code.prose());
    let error = recorder
        .end_run(run, Err(second))
        .await
        .expect_err("a run ends exactly once");

    assert_refused(&error, 1, "run_ended");
    let text = display_chain(&error);
    assert!(
        text.contains("run 1 has ended"),
        "the failure says the run ended: {text}"
    );
    assert_eq!(
        recorder.result(run),
        Some(canceled()),
        "the first result stands"
    );
}

#[tokio::test]
async fn an_arc_dyn_recorder_drives_a_whole_run() {
    let memory = Arc::new(MemoryRecorder::new());
    let recorder: Arc<dyn RunRecorder> = memory.clone();

    let run = recorder.begin_run(meta()).await.unwrap();
    recorder
        .append(
            run,
            record(RecordKind::Effect, 0, "through the trait object"),
        )
        .await
        .unwrap();
    recorder.end_run(run, Ok("ok".to_owned())).await.unwrap();

    assert_eq!(
        labels(&memory.records(run)),
        ["through the trait object"],
        "writes through the trait object reach the concrete recorder"
    );
    assert_eq!(memory.result(run), Some(Ok("ok".to_owned())));
}

#[derive(Debug)]
struct DiskFull;

impl fmt::Display for DiskFull {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the disk is full")
    }
}

impl Error for DiskFull {}

#[test]
fn a_recorder_error_keeps_its_cause_in_the_message_only() {
    let error = make_recorder_error(DiskFull);

    assert_eq!(error.code, ErrorCode::RecorderFailed);
    assert_eq!(
        display_chain(&error),
        "the run recorder failed: the disk is full",
        "the rendered chain shows the recorder's cause"
    );
    assert_eq!(error.prose, ErrorCode::RecorderFailed.prose());
}

#[test]
fn a_recorder_error_takes_a_plain_message() {
    let error = make_recorder_error("the store is read-only");

    assert_eq!(
        display_chain(&error),
        "the run recorder failed: the store is read-only"
    );
}

#[test]
fn a_run_id_wraps_and_renders_its_raw_value() {
    let id = RunId::from_raw(-3);

    assert_eq!(id.get(), -3);
    assert_eq!(id.to_string(), "-3");
    assert!(
        RunId::from_raw(1) < RunId::from_raw(2),
        "ids order by value"
    );
}

#[test]
fn record_kind_text_round_trips_and_rejects_unknown_text() {
    for (kind, text) in [
        (RecordKind::Effect, "effect"),
        (RecordKind::Answer, "answer"),
        (RecordKind::Event, "event"),
    ] {
        assert_eq!(kind.as_str(), text);
        assert_eq!(RecordKind::parse(text), Some(kind));
    }
    assert_eq!(RecordKind::parse("Effect"), None, "the text is exact");
    assert_eq!(RecordKind::parse("nope"), None);
}
