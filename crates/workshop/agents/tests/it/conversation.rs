//! A conversation over its one run: the run's report becomes the
//! conversation's failure report, an agent that fails to parse is a
//! run-failed report with no run recorded, a stop keeps a question open, a
//! close cancels the run, and the conversation ends with its run.

// clippy.toml's allow-expect-in-tests covers #[test] functions only, not
// the helpers they share; failing a test by panicking with the invariant
// named is exactly what these are for.
#![expect(
    clippy::expect_used,
    reason = "test helpers fail by panicking with the invariant named"
)]

use std::sync::Arc;
use std::time::Duration;

use harness::record::{MemoryRecorder, RunId};
use tokio::sync::broadcast;
use workshop_agents::{
    Conversation, Conversations, FailureKind, SessionFailure, SessionState, WaitFrame,
};

use crate::support::{AGENT, prompt, run_for};

const DEADLINE: Duration = Duration::from_secs(10);

/// Runs the conversation's one run over `source` on its own task.
fn start(
    conversation: &Conversation,
    recorder: &Arc<MemoryRecorder>,
    source: String,
) -> tokio::task::JoinHandle<()> {
    let (host, setup) = run_for(conversation, Arc::clone(recorder), source);
    let conversation = conversation.clone();
    tokio::spawn(async move { conversation.run(host, AGENT, setup).await })
}

async fn next_wait(waits: &mut broadcast::Receiver<WaitFrame>) -> WaitFrame {
    tokio::time::timeout(DEADLINE, waits.recv())
        .await
        .expect("a wait frame arrives in time")
        .expect("the wait channel is open")
}

async fn required(waits: &mut broadcast::Receiver<WaitFrame>) -> String {
    match next_wait(waits).await {
        WaitFrame::Required { token } => token,
        frame @ WaitFrame::Canceled { .. } => panic!("expected a required frame, got {frame:?}"),
    }
}

async fn ended(run: tokio::task::JoinHandle<()>) {
    tokio::time::timeout(DEADLINE, run)
        .await
        .expect("the run ends in time")
        .expect("the run's task joins");
}

/// Every failure report still queued.
fn reports(errors: &mut broadcast::Receiver<SessionFailure>) -> Vec<SessionFailure> {
    std::iter::from_fn(|| errors.try_recv().ok()).collect()
}

#[tokio::test]
async fn a_failed_report_becomes_a_run_failed_report_and_ends_the_conversation() {
    let table = Conversations::new();
    let conversation = table.open("boom");
    let recorder = Arc::new(MemoryRecorder::new());
    let mut errors = conversation.subscribe_errors();
    let mut events = conversation.subscribe_events();

    ended(start(&conversation, &recorder, prompt("error('kaboom')"))).await;

    let reports = reports(&mut errors);
    assert_eq!(reports.len(), 1, "one report: {reports:?}");
    assert_eq!(reports[0].kind, FailureKind::RunFailed);
    assert!(
        reports[0].message.contains("kaboom"),
        "the run's own failure is the message: {reports:?}"
    );
    assert_eq!(conversation.state(), SessionState::Closed);
    let run = conversation.run_id().expect("the recorder began the run");
    let Some(Err(error)) = recorder.result(run) else {
        panic!("the run is recorded as failed");
    };
    assert!(!error.is_canceled(), "a failure is not a cancel: {error:?}");
    assert_eq!(
        reports[0].message,
        error.located_message(),
        "the report is the recorded failure's located message"
    );
    assert!(
        table.get(conversation.id()).is_none(),
        "an ended conversation leaves the table"
    );
    while let Ok(_entry) = events.try_recv() {}
    assert!(
        matches!(
            events.try_recv(),
            Err(broadcast::error::TryRecvError::Closed)
        ),
        "an ended conversation's channels close"
    );
}

#[tokio::test]
async fn an_agent_that_fails_to_parse_reports_a_run_failure_and_records_no_run() {
    let table = Conversations::new();
    let conversation = table.open("unclosed");
    let recorder = Arc::new(MemoryRecorder::new());
    let mut errors = conversation.subscribe_errors();
    let unclosed = "---\nname: unclosed\ndescription: d\nrunvil: 0\n\n# Title\n".to_owned();
    let (host, _setup) = run_for(&conversation, Arc::clone(&recorder), unclosed.clone());
    let parse_failure = host
        .parse(AGENT)
        .expect_err("a frontmatter that is never closed does not parse");

    ended(start(&conversation, &recorder, unclosed)).await;

    let reports = reports(&mut errors);
    assert_eq!(
        reports,
        [SessionFailure {
            kind: FailureKind::RunFailed,
            message: parse_failure.located_message(),
        }],
        "the parse failure's located message is the one run-failed report"
    );
    assert_eq!(conversation.run_id(), None, "no run began");
    assert_eq!(
        recorder.meta(RunId::from_raw(1)),
        None,
        "the recorder holds no run"
    );
    assert_eq!(conversation.state(), SessionState::Closed);
    assert!(
        table.get(conversation.id()).is_none(),
        "the conversation ends with its parse failure"
    );
}

#[tokio::test]
async fn a_completed_report_reports_nothing_and_ends_the_conversation() {
    let table = Conversations::new();
    let conversation = table.open("done");
    let recorder = Arc::new(MemoryRecorder::new());
    let mut errors = conversation.subscribe_errors();

    ended(start(&conversation, &recorder, prompt("return 'done'"))).await;

    assert!(
        reports(&mut errors).is_empty(),
        "a completed run reports nothing"
    );
    assert_eq!(conversation.state(), SessionState::Closed);
    let run = conversation.run_id().expect("the recorder began the run");
    assert_eq!(recorder.result(run), Some(Ok("done".to_owned())));
    assert!(table.get(conversation.id()).is_none());
}

#[tokio::test]
async fn a_close_with_a_question_open_cancels_it_and_reports_the_cancel() {
    let table = Conversations::new();
    let conversation = table.open("ask");
    let recorder = Arc::new(MemoryRecorder::new());
    let mut errors = conversation.subscribe_errors();
    let mut waits = conversation.subscribe_waits();
    let run = start(&conversation, &recorder, prompt("return (input.ask())"));
    let token = required(&mut waits).await;

    assert!(
        table.close(conversation.id()),
        "the open conversation closes"
    );
    assert_eq!(
        conversation.state(),
        SessionState::Closing,
        "a close is Closing until the run ends"
    );
    assert_eq!(
        next_wait(&mut waits).await,
        WaitFrame::Canceled { token },
        "the open question dies as an outcome"
    );
    ended(run).await;

    let reports = reports(&mut errors);
    assert_eq!(
        reports.iter().map(|report| report.kind).collect::<Vec<_>>(),
        [FailureKind::Canceled],
        "a canceled report is the cancel's one report"
    );
    assert_eq!(conversation.state(), SessionState::Closed);
    assert!(conversation.unresolved_waits().is_empty());
    let run = conversation.run_id().expect("the recorder began the run");
    assert!(matches!(
        recorder.result(run),
        Some(Err(error)) if error.is_canceled()
    ));
    assert!(!table.close(conversation.id()), "a second close is a no-op");
}

#[tokio::test]
async fn a_close_before_the_run_starts_ends_its_recorded_run_canceled() {
    let table = Conversations::new();
    let conversation = table.open("early");
    let recorder = Arc::new(MemoryRecorder::new());
    let mut errors = conversation.subscribe_errors();

    conversation.close();
    ended(start(&conversation, &recorder, prompt("return 'never'"))).await;

    let run = conversation.run_id().expect("the recorder began the run");
    assert!(matches!(
        recorder.result(run),
        Some(Err(error)) if error.is_canceled()
    ));
    assert!(
        recorder.records(run).is_empty(),
        "the run ended before it started"
    );
    assert_eq!(
        reports(&mut errors)
            .iter()
            .map(|report| report.kind)
            .collect::<Vec<_>>(),
        [FailureKind::Canceled]
    );
    assert_eq!(conversation.state(), SessionState::Closed);
}

#[tokio::test]
async fn a_stop_with_only_a_question_open_leaves_it_open_and_its_token_answers() {
    let table = Conversations::new();
    let conversation = table.open("ask");
    let recorder = Arc::new(MemoryRecorder::new());
    let mut errors = conversation.subscribe_errors();
    let mut waits = conversation.subscribe_waits();
    let run = start(&conversation, &recorder, prompt("return (input.ask())"));
    let token = required(&mut waits).await;

    conversation.stop_round();
    // The stop is taken by the run's loop; the question it spared stays
    // open, so the original token is still the one to answer.
    tokio::task::yield_now().await;
    assert_eq!(
        conversation.unresolved_waits(),
        std::slice::from_ref(&token)
    );
    conversation
        .send_input(&token, "still here".to_owned(), || {})
        .expect("the original token answers the open question");
    ended(run).await;

    assert!(
        matches!(
            waits.try_recv(),
            Err(broadcast::error::TryRecvError::Empty | broadcast::error::TryRecvError::Closed)
        ),
        "no canceled frame: the stop never touched the question"
    );
    assert!(reports(&mut errors).is_empty(), "a stop is never an error");
    let run = conversation.run_id().expect("the recorder began the run");
    assert_eq!(
        recorder.result(run),
        Some(Ok("still here".to_owned())),
        "the run went on past the stop to the operator's answer"
    );
}
