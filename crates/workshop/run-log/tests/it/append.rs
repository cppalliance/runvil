//! The write path: a run begins, records append in loop order, the run ends.

use std::num::NonZeroU32;

use runvil::{Error, ErrorCode, ErrorFact, RunSeed, SourceLoc};
use serde_json::json;
use workshop_run_log::{LogError, Record, RecordFilter, RecordKind, RunId, RunLog, RunMeta};

/// Every record of a run, in loop order.
const ALL: RecordFilter = RecordFilter {
    kind: None,
    last: None,
};

/// The agent the Host launched, stored beside the Harness's metadata.
const AGENT: &str = "chat";

/// A run's opening row as the Harness would write it.
fn meta() -> RunMeta {
    RunMeta {
        seed: RunSeed::from_bytes([0xfe; 32]),
        flags: 0,
        started_at: 1_700_000_000_000,
    }
}

/// One record of `kind` on task 0 at `task_seq`.
fn record(kind: RecordKind, task_seq: u32, effect_id: Option<u64>) -> Record {
    Record {
        task_id: "0".to_owned(),
        task_seq,
        kind,
        effect_id,
        payload: json!({ "task_seq": task_seq }),
    }
}

#[tokio::test]
async fn a_new_log_has_neither_a_name_nor_a_prompt_hash_column() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("runs.db");
    drop(RunLog::open(&path).await.unwrap());

    let database = turso::Builder::new_local(path.to_str().unwrap())
        .build()
        .await
        .unwrap();
    let conn = database.connect().unwrap();
    let mut rows = conn.query("PRAGMA table_info(runs)", ()).await.unwrap();
    let mut columns = Vec::new();
    while let Some(row) = rows.next().await.unwrap() {
        columns.push(row.get::<String>(1).unwrap());
    }
    assert!(
        columns.iter().any(|column| column == "agent"),
        "the pragma lists the runs table's columns: {columns:?}"
    );
    assert!(
        !columns
            .iter()
            .any(|column| column == "name" || column == "prompt_hash"),
        "the runs table holds neither the run's name nor a prompt hash: {columns:?}"
    );
}

#[tokio::test]
async fn a_run_with_three_records_round_trips_through_an_in_memory_log() {
    let mut log = RunLog::in_memory().await.unwrap();
    let run = log.begin_run(meta(), AGENT).await.unwrap();

    log.append(run, record(RecordKind::Effect, 0, Some(7)))
        .await
        .unwrap();
    log.append(run, record(RecordKind::Answer, 1, Some(7)))
        .await
        .unwrap();
    log.append(run, record(RecordKind::Event, 2, None))
        .await
        .unwrap();

    let row = log.run(run).await.unwrap();
    assert_eq!(row.id, run);
    assert_eq!(row.meta, meta());
    assert_eq!(row.agent, AGENT, "the agent is stored beside the meta");
    assert_eq!(row.ended_at, None);
    assert_eq!(row.result, None);

    let records = log.records(run, ALL).await.unwrap();
    assert_eq!(records.len(), 3);
    let kinds: Vec<RecordKind> = records.iter().map(|stored| stored.record.kind).collect();
    assert_eq!(
        kinds,
        [RecordKind::Effect, RecordKind::Answer, RecordKind::Event]
    );
    let effect_ids: Vec<Option<u64>> = records
        .iter()
        .map(|stored| stored.record.effect_id)
        .collect();
    assert_eq!(effect_ids, [Some(7), Some(7), None]);
    for (index, stored) in records.iter().enumerate() {
        let task_seq = u32::try_from(index).unwrap();
        assert_eq!(stored.record.task_id, "0");
        assert_eq!(stored.record.task_seq, task_seq);
        assert_eq!(stored.record.payload, json!({ "task_seq": task_seq }));
        assert!(stored.at >= row.meta.started_at);
    }
}

#[tokio::test]
async fn seq_is_assigned_in_call_order_and_strictly_increases() {
    let mut log = RunLog::in_memory().await.unwrap();
    let run = log.begin_run(meta(), AGENT).await.unwrap();

    let mut seqs = Vec::new();
    for task_seq in 0..5 {
        let seq = log
            .append(run, record(RecordKind::Event, task_seq, None))
            .await
            .unwrap();
        seqs.push(seq);
    }
    let raw: Vec<u64> = seqs.iter().map(|seq| seq.get()).collect();
    assert_eq!(raw, [0, 1, 2, 3, 4]);

    let stored: Vec<u64> = log
        .records(run, ALL)
        .await
        .unwrap()
        .iter()
        .map(|stored| stored.seq.get())
        .collect();
    assert_eq!(stored, raw);
}

#[tokio::test]
async fn seq_is_per_run_so_two_runs_each_start_at_zero() {
    let mut log = RunLog::in_memory().await.unwrap();
    let first = log.begin_run(meta(), AGENT).await.unwrap();
    let second = log.begin_run(meta(), AGENT).await.unwrap();
    assert_ne!(first, second);

    log.append(first, record(RecordKind::Event, 0, None))
        .await
        .unwrap();
    let seq = log
        .append(second, record(RecordKind::Event, 0, None))
        .await
        .unwrap();
    assert_eq!(seq.get(), 0);
    assert_eq!(log.records(first, ALL).await.unwrap().len(), 1);
    assert_eq!(log.records(second, ALL).await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_filter_selects_by_kind_and_keeps_the_last_n() {
    let mut log = RunLog::in_memory().await.unwrap();
    let run = log.begin_run(meta(), AGENT).await.unwrap();
    // Two tasks interleaved (the main walk and its first child); records
    // come back in loop order, not in any one task's `task_seq` order.
    let appended = [
        ("0", 0, RecordKind::Event),
        ("0.0", 1, RecordKind::Event),
        ("0", 1, RecordKind::Effect),
        ("0.0", 0, RecordKind::Event),
        ("0", 2, RecordKind::Event),
    ];
    for (task_id, task_seq, kind) in appended {
        let mut record = record(kind, task_seq, None);
        record.task_id = task_id.to_owned();
        log.append(run, record).await.unwrap();
    }
    let positions = |records: Vec<workshop_run_log::StoredRecord>| -> Vec<(String, u32)> {
        records
            .into_iter()
            .map(|stored| (stored.record.task_id, stored.record.task_seq))
            .collect()
    };
    let expected = |positions: &[(&str, u32)]| -> Vec<(String, u32)> {
        positions
            .iter()
            .map(|(task, seq)| ((*task).to_owned(), *seq))
            .collect()
    };

    let events = RecordFilter {
        kind: Some(RecordKind::Event),
        ..ALL
    };
    let all_events = log.records(run, events.clone()).await.unwrap();
    assert_eq!(
        positions(all_events),
        expected(&[("0", 0), ("0.0", 1), ("0.0", 0), ("0", 2)])
    );

    let last_two = log
        .records(
            run,
            RecordFilter {
                last: Some(2),
                ..ALL
            },
        )
        .await
        .unwrap();
    assert_eq!(positions(last_two), expected(&[("0.0", 0), ("0", 2)]));

    let last_event = log
        .records(
            run,
            RecordFilter {
                last: Some(1),
                ..events
            },
        )
        .await
        .unwrap();
    assert_eq!(positions(last_event), expected(&[("0", 2)]));
}

#[tokio::test]
async fn end_run_fills_ended_at_and_the_result() {
    let mut log = RunLog::in_memory().await.unwrap();
    let run = log.begin_run(meta(), AGENT).await.unwrap();
    log.end_run(run, Ok("done".to_owned())).await.unwrap();

    let row = log.run(run).await.unwrap();
    assert!(row.ended_at.is_some_and(|at| at >= row.meta.started_at));
    assert_eq!(row.result, Some(Ok("done".to_owned())));
}

#[tokio::test]
async fn a_failed_run_keeps_its_code_and_located_message() {
    let mut log = RunLog::in_memory().await.unwrap();
    let run = log.begin_run(meta(), AGENT).await.unwrap();
    let mut error = Error::new(
        ErrorCode::ToolBackend,
        "expected a reply, got nothing",
        "the tool's own prose",
    );
    error.at = Some(SourceLoc::line("prompt.md", NonZeroU32::new(7).unwrap()));
    error.facts.insert(ErrorFact::Path, json!("/notes.md"));
    log.end_run(run, Err(error.clone())).await.unwrap();

    let Some(Err(read)) = log.run(run).await.unwrap().result else {
        panic!("a failed run reads back as an error");
    };
    assert_eq!(read.code, error.code);
    assert_eq!(read.located_message(), error.located_message());
    assert!(
        read.located_message().ends_with("(at prompt.md:7)"),
        "the location reads back inside the message: {read:?}"
    );
    assert_eq!(
        read.prose,
        error.code.prose(),
        "the prose is the code's default"
    );
    assert_eq!(read.at, None, "the location is stored only in the message");
    assert!(read.facts.is_empty(), "facts are not stored: {read:?}");
}

#[tokio::test]
async fn a_canceled_run_reads_back_as_a_cancel() {
    let mut log = RunLog::in_memory().await.unwrap();
    let run = log.begin_run(meta(), AGENT).await.unwrap();
    log.end_run(run, Err(Error::canceled())).await.unwrap();

    let Some(Err(read)) = log.run(run).await.unwrap().result else {
        panic!("a canceled run reads back as an error");
    };
    assert!(
        read.is_canceled(),
        "the error read back is a cancel: {read:?}"
    );
    assert_eq!(read.located_message(), Error::canceled().located_message());
}

#[tokio::test]
async fn a_run_ends_exactly_once() {
    let mut log = RunLog::in_memory().await.unwrap();
    let run = log.begin_run(meta(), AGENT).await.unwrap();
    log.end_run(run, Err(Error::canceled())).await.unwrap();

    let again = log.end_run(run, Err(Error::canceled())).await;
    assert!(matches!(again, Err(LogError::RunEnded(id)) if id == run));

    let late = log.append(run, record(RecordKind::Event, 0, None)).await;
    assert!(matches!(late, Err(LogError::RunEnded(id)) if id == run));
}

#[tokio::test]
async fn an_unknown_run_is_refused() {
    let mut log = RunLog::in_memory().await.unwrap();
    let ghost = RunId::from_raw(41);

    let appended = log.append(ghost, record(RecordKind::Event, 0, None)).await;
    assert!(matches!(appended, Err(LogError::UnknownRun(id)) if id == ghost));

    let ended = log.end_run(ghost, Err(Error::canceled())).await;
    assert!(matches!(ended, Err(LogError::UnknownRun(id)) if id == ghost));

    let read = log.run(ghost).await;
    assert!(matches!(read, Err(LogError::UnknownRun(id)) if id == ghost));
}

#[tokio::test]
async fn a_32_byte_seed_survives_a_run_log_round_trip() {
    // Distinct bytes from a leading `07` to a trailing `ff`, so a dropped
    // leading zero, a swapped pair, or a sign bit shows.
    let seed = RunSeed::from_bytes(std::array::from_fn(|i| u8::try_from(i * 8 + 7).unwrap()));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("runs.db");
    let run = {
        let mut log = RunLog::open(&path).await.unwrap();
        let meta = RunMeta { seed, ..meta() };
        log.begin_run(meta, AGENT).await.unwrap()
    };

    let log = RunLog::open(&path).await.unwrap();
    let read = log.run(run).await.unwrap().meta.seed;
    assert_eq!(read.as_bytes(), seed.as_bytes());
    drop(log);

    let raw = turso::Builder::new_local(path.to_str().unwrap())
        .build()
        .await
        .unwrap()
        .connect()
        .unwrap();
    let mut rows = raw
        .query("SELECT seed FROM runs WHERE run_id = ?1", (run.get(),))
        .await
        .unwrap();
    let stored: String = rows.next().await.unwrap().unwrap().get(0).unwrap();
    assert_eq!(
        stored,
        seed.to_string(),
        "the column holds the 64 hex digits"
    );
    drop(rows);
    raw.execute(
        "UPDATE runs SET seed = 'not a seed' WHERE run_id = ?1",
        (run.get(),),
    )
    .await
    .unwrap();
    drop(raw);

    let log = RunLog::open(&path).await.unwrap();
    let refused = log.run(run).await;
    assert!(
        matches!(&refused, Err(LogError::Corrupt(message)) if message.contains("seed")),
        "a malformed seed is a read error naming the column: {refused:?}"
    );
}

#[tokio::test]
async fn a_log_on_disk_keeps_its_rows_across_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("runs.db");

    let run = {
        let mut log = RunLog::open(&path).await.unwrap();
        let run = log.begin_run(meta(), AGENT).await.unwrap();
        log.append(run, record(RecordKind::Event, 0, None))
            .await
            .unwrap();
        log.end_run(run, Err(Error::canceled())).await.unwrap();
        run
    };

    let log = RunLog::open(&path).await.unwrap();
    let row = log.run(run).await.unwrap();
    assert!(matches!(row.result, Some(Err(error)) if error.is_canceled()));
    assert_eq!(log.records(run, ALL).await.unwrap().len(), 1);
}
