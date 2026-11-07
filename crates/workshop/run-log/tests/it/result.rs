//! The stored form of a run's result: the raw columns `end_run` writes,
//! and the rows `run` refuses because they fit no result.

// clippy.toml's allow-unwrap-in-tests covers #[test] functions only, not
// the helpers they share; failing a test by panicking is what these are for.
#![expect(
    clippy::unwrap_used,
    reason = "test helpers fail by panicking with the failure shown"
)]

use std::num::NonZeroU32;
use std::path::Path;

use harness::record::RunSeed;
use runvil::{Error, ErrorCode, SourceLoc};
use workshop_run_log::{LogError, RunId, RunLog, RunMeta, RunRow};

/// The agent the Host launched.
const AGENT: &str = "chat";

/// A run's opening row.
fn meta() -> RunMeta {
    RunMeta {
        name: "session-1".to_owned(),
        prompt_hash: "sha256:abc".to_owned(),
        seed: RunSeed::from_bytes([5; 32]),
        flags: 0,
        started_at: 1_700_000_000_000,
    }
}

/// A failure that is not a cancel.
fn failure() -> Error {
    let code = ErrorCode::ToolBackend;
    Error::new(code, "expected a reply, got nothing", code.prose())
}

/// Begins a run in the log file at `path`, ends it with `result`, and
/// closes the log.
async fn ended(path: &Path, result: Result<String, Error>) -> RunId {
    let mut log = RunLog::open(path).await.unwrap();
    let run = log.begin_run(meta(), AGENT).await.unwrap();
    log.end_run(run, result).await.unwrap();
    run
}

/// A direct connection to the log file at `path`, past the log's own
/// reads and writes.
async fn connect(path: &Path) -> turso::Connection {
    turso::Builder::new_local(path.to_str().unwrap())
        .build()
        .await
        .unwrap()
        .connect()
        .unwrap()
}

/// Reopens the log file at `path` and reads `run`'s row.
async fn reread(path: &Path, run: RunId) -> Result<RunRow, LogError> {
    let log = RunLog::open(path).await.unwrap();
    log.run(run).await
}

#[tokio::test]
async fn the_status_and_error_code_columns_hold_the_status_and_the_code_tag() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("runs.db");
    let mut located = Error::new(
        ErrorCode::NotFound,
        "expected /notes.md to exist, found nothing",
        ErrorCode::NotFound.prose(),
    );
    located.at = Some(SourceLoc::line("prompt.md", NonZeroU32::new(3).unwrap()));
    let failed = ended(&path, Err(located.clone())).await;
    let canceled = ended(&path, Err(Error::canceled())).await;
    let completed = ended(&path, Ok("done".to_owned())).await;

    let raw = connect(&path).await;
    let expected = [
        (
            failed,
            "failed",
            None,
            Some(ErrorCode::NotFound.tag().to_owned()),
            Some(located.located_message()),
        ),
        (
            canceled,
            "canceled",
            None,
            Some(ErrorCode::Canceled.tag().to_owned()),
            Some(Error::canceled().located_message()),
        ),
        (completed, "completed", Some("done".to_owned()), None, None),
    ];
    for (run, status, final_text, error_code, error_message) in expected {
        let mut rows = raw
            .query(
                "SELECT status, final_text, error_code, error_message FROM runs \
                 WHERE run_id = ?1",
                (run.get(),),
            )
            .await
            .unwrap();
        let row = rows.next().await.unwrap().unwrap();
        assert_eq!(row.get::<String>(0).unwrap(), status, "run {run}'s status");
        assert_eq!(
            row.get::<Option<String>>(1).unwrap(),
            final_text,
            "run {run}'s final_text"
        );
        assert_eq!(
            row.get::<Option<String>>(2).unwrap(),
            error_code,
            "run {run}'s error_code is its code's tag"
        );
        assert_eq!(
            row.get::<Option<String>>(3).unwrap(),
            error_message,
            "run {run}'s error_message is its located message"
        );
    }
}

#[tokio::test]
async fn an_unknown_code_tag_in_error_code_reads_as_corrupt() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("runs.db");
    let run = ended(&path, Err(failure())).await;
    let raw = connect(&path).await;
    raw.execute(
        "UPDATE runs SET error_code = 'exploded' WHERE run_id = ?1",
        (run.get(),),
    )
    .await
    .unwrap();
    drop(raw);

    let refused = reread(&path, run).await;
    assert!(
        matches!(&refused, Err(LogError::Corrupt(message))
            if message.contains("error_code") && message.contains("exploded")),
        "an unknown code tag is a read error naming the column and the tag: {refused:?}"
    );
}

#[tokio::test]
async fn an_unknown_status_reads_as_corrupt() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("runs.db");
    let run = ended(&path, Ok("done".to_owned())).await;
    let raw = connect(&path).await;
    let update = "UPDATE runs SET status = 'exploded' WHERE run_id = ?1";
    assert!(
        raw.execute(update, (run.get(),)).await.is_err(),
        "the schema refuses an unknown status"
    );
    raw.execute("PRAGMA ignore_check_constraints = ON", ())
        .await
        .unwrap();
    raw.execute(update, (run.get(),)).await.unwrap();
    drop(raw);

    let refused = reread(&path, run).await;
    assert!(
        matches!(&refused, Err(LogError::Corrupt(message))
            if message.contains("status") && message.contains("exploded")),
        "an unknown status is a read error naming the status: {refused:?}"
    );
}

#[tokio::test]
async fn a_row_whose_columns_fit_no_result_reads_as_corrupt() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("runs.db");
    let run = ended(&path, Ok("done".to_owned())).await;
    // Each row breaks one rule of the three shapes: a completed run has
    // its final text alone, and a failed or canceled run has a code and a
    // message, the code `canceled` exactly when the run is canceled.
    let shapes = [
        ("completed", None, None, None),
        (
            "completed",
            Some("done"),
            Some("tool_backend"),
            Some("failed"),
        ),
        ("failed", None, Some("tool_backend"), None),
        ("failed", Some("done"), Some("tool_backend"), Some("failed")),
        ("failed", None, Some("canceled"), Some("canceled")),
        ("canceled", None, Some("tool_backend"), Some("failed")),
        ("canceled", None, None, Some("canceled")),
    ];
    for (status, final_text, error_code, error_message) in shapes {
        let raw = connect(&path).await;
        raw.execute(
            "UPDATE runs SET status = ?2, final_text = ?3, error_code = ?4, \
             error_message = ?5 WHERE run_id = ?1",
            (run.get(), status, final_text, error_code, error_message),
        )
        .await
        .unwrap();
        drop(raw);

        let refused = reread(&path, run).await;
        assert!(
            matches!(refused, Err(LogError::Corrupt(_))),
            "status {status} with final_text={final_text:?} error_code={error_code:?} \
             error_message={error_message:?} reads as corrupt: {refused:?}"
        );
    }
}
