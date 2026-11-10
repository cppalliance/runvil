//! Agent status tests: which conversation events push a status frame and reset the backoff.

use std::fmt;
use std::time::Duration;

use runvil::ids::{ChainId, Provenance, RoundId, TaskId};
use workshop_protocol::Severity;
use workshop_registry::Registry;
use workshop_status::StatusBus;

use super::*;

/// A push facade wired to a real status bus through the registry, with
/// the bus's receiver and the registration guards a test reads through.
fn wired_push() -> (
    Push,
    broadcast::Receiver<workshop_protocol::StatusBarUpdate>,
    impl fmt::Debug + Send + Sync + 'static + use<>,
) {
    let status = StatusBus::new();
    let status_rx = status.subscribe();
    let registry = Registry::new();
    let guards = workshop_status::register(&registry, &status);
    (registry.push(), status_rx, guards)
}

/// A session event holding one Engine event under the fixed test
/// coordinates, in the persisted shape the reporter reads.
fn session_event(event: &Event) -> SessionEvent {
    SessionEvent {
        index: 0,
        reply: None,
        event: serde_json::to_value(event).expect("an engine event serializes"),
    }
}

fn reply_event() -> Event {
    Event::AssistantReply {
        section: "chat".to_owned(),
        provenance: Provenance::new(TaskId::from(ChainId::root()), 0),
        turn: 1,
        round: RoundId::new(0),
        text: "hello".to_owned(),
        finish_reason: None,
        model: "m".to_owned(),
        metrics: None,
        origin: runvil::event::ReplyOrigin::Chat,
    }
}

/// `task` parked on its empty inbox.
fn waiting_event(task: TaskId) -> Event {
    Event::InboxWaiting {
        section: "chat".to_owned(),
        provenance: Provenance::new(task, 0),
    }
}

/// The main chain's program returning, which ends the run.
fn run_succeeded() -> Event {
    Event::RunSucceeded {
        section: "chat".to_owned(),
        provenance: Provenance::new(TaskId::from(ChainId::root()), 0),
    }
}

/// The four failure kinds the session reports on its error channel: a
/// failed model turn and a failed tool call the program survived, a run
/// that ended in error, and the synthetic terminal of a cancel. Each
/// must release the turn-dispatch Thinking push with a terminal,
/// non-thinking status; the survived turns keep the boundary as their
/// label because the agent is still running, and only a run that ended
/// reads `Agent failed`. The label comes from the kind alone: the message
/// is deliberately unlike the label, so a reporter that read the sentence
/// would mislabel every row.
#[test]
fn every_error_report_pushes_a_terminal_failure_status() {
    let (push, mut status_rx, _guards) = wired_push();
    let reports = [
        (
            FailureKind::ModelTurnFailed,
            "round 3 in agent `chat`",
            "Model turn failed",
        ),
        (
            FailureKind::ToolCallFailed,
            "call 7 in agent `chat`",
            "Tool call failed",
        ),
        (
            FailureKind::RunFailed,
            "agent run failed: kaboom",
            "Agent failed",
        ),
        (FailureKind::Canceled, "run canceled", "Agent failed"),
    ];

    for (kind, message, label) in reports {
        let failure = SessionFailure {
            kind,
            message: message.to_owned(),
        };
        on_error(&failure, &push);
        let update = status_rx
            .try_recv()
            .expect("the report pushes a terminal status");
        assert_eq!(update.severity, Severity::Error, "kind: {kind:?}");
        assert_eq!(
            update.activity,
            Activity::General,
            "a non-thinking activity releases the status bar's sustained amber LED"
        );
        assert_eq!(
            update.label, label,
            "the label is chosen by the kind, never by the sentence: {kind:?}"
        );
        assert_eq!(
            update.description, message,
            "the message passes through unchanged as the status description"
        );
    }
}

#[test]
fn the_status_stays_busy_across_a_reply_and_goes_idle_at_the_main_chains_wait() {
    let (push, mut status_rx, _guards) = wired_push();
    let backoff = ReconnectBackoff::with_schedule(
        Duration::from_millis(1),
        Duration::from_millis(8),
        Duration::from_secs(1),
    );
    let _ = backoff.next_delay();
    assert!(
        backoff.is_escalated_for_test(),
        "a handed-out delay escalates the schedule"
    );

    on_event(&session_event(&reply_event()), &push, &backoff);
    assert!(
        status_rx.try_recv().is_err(),
        "a reply does not end the turn: a message that lands as it does makes the loop go on"
    );
    assert!(
        !backoff.is_escalated_for_test(),
        "an agent reply is useful gateway work and resets the backoff"
    );

    let task = TaskId::from(ChainId::root().child(0));
    on_event(&session_event(&waiting_event(task)), &push, &backoff);
    assert!(
        status_rx.try_recv().is_err(),
        "a task waiting on its inbox ends no turn of the operator's"
    );

    let main = TaskId::from(ChainId::root());
    on_event(&session_event(&waiting_event(main)), &push, &backoff);
    let update = status_rx
        .try_recv()
        .expect("the main chain's wait on its inbox pushes the idle status");
    assert_eq!(update.severity, Severity::Info);
    assert_eq!(update.activity, Activity::General);
    assert_eq!(update.label, "Ready");
}

#[test]
fn the_main_chains_run_success_pushes_the_idle_status() {
    let (push, mut status_rx, _guards) = wired_push();
    let backoff = ReconnectBackoff::new();
    let failed = Event::RunFailed {
        section: "chat".to_owned(),
        provenance: Provenance::new(TaskId::from(ChainId::root()), 0),
    };

    on_event(&session_event(&failed), &push, &backoff);
    assert!(
        status_rx.try_recv().is_err(),
        "a failed run's failure report sets its status, so its end pushes nothing"
    );

    on_event(&session_event(&run_succeeded()), &push, &backoff);
    let update = status_rx
        .try_recv()
        .expect("a run that returned waits on no inbox, so its success pushes the idle status");
    assert_eq!(update.severity, Severity::Info);
    assert_eq!(update.activity, Activity::General);
    assert_eq!(update.label, "Ready");
}

/// The conversation closes its three channels together as its run ends,
/// when the run's last reports can still be queued: the final round's
/// pulse, the run's success, a failure report. Each still reaches the
/// status bar, deltas first, and the reporter returns once all three have
/// closed.
#[tokio::test]
async fn the_reporter_drains_every_channel_after_they_close() {
    let (push, mut status_rx, _guards) = wired_push();
    let (events_tx, events) = broadcast::channel::<SessionEvent>(1);
    let (deltas_tx, deltas) = broadcast::channel::<Delta>(1);
    let (errors_tx, errors) = broadcast::channel::<SessionFailure>(1);
    let delta: Delta = serde_json::from_str(r#"{"kind":"text","content":"po","reply":1}"#)
        .expect("the delta fixture parses");
    deltas_tx
        .send(delta)
        .expect("the reporter's receiver is subscribed");
    events_tx
        .send(session_event(&run_succeeded()))
        .expect("the reporter's receiver is subscribed");
    errors_tx
        .send(SessionFailure {
            kind: FailureKind::RunFailed,
            message: "the run could not be recorded: disk full".to_owned(),
        })
        .expect("the reporter's receiver is subscribed");
    drop((events_tx, deltas_tx, errors_tx));

    report(events, deltas, errors, push, ReconnectBackoff::new()).await;

    let labels: Vec<String> = std::iter::from_fn(|| status_rx.try_recv().ok())
        .map(|update| update.label)
        .collect();
    assert_eq!(
        labels,
        ["Streaming response...", "Ready", "Agent failed"],
        "every report queued at the close reaches the status bar, deltas first"
    );
}

#[test]
fn each_delta_pulses_the_activity_of_its_channel() {
    let (push, mut status_rx, _guards) = wired_push();
    // The conversation's `Delta` is `#[non_exhaustive]`, so each
    // fixture is deserialized rather than written as a struct literal.
    let cases = [
        (
            r#"{"kind":"text","content":"po","reply":1}"#,
            Activity::Generating,
        ),
        (
            r#"{"kind":"reasoning","content":"hmm","reply":1}"#,
            Activity::Thinking,
        ),
    ];

    for (json, activity) in cases {
        let delta: Delta = serde_json::from_str(json).expect("the delta fixture parses");
        on_delta(&delta, &push);
        let update = status_rx
            .try_recv()
            .expect("every delta pushes an activity pulse");
        assert_eq!(
            update.activity, activity,
            "the channel picks the LED: {json}"
        );
        assert_eq!(
            update.severity,
            Severity::Debug,
            "a pulse drives the LED and is never displayed as text"
        );
        assert_eq!(update.label, "Streaming response...");
    }
}

#[test]
fn a_lifecycle_event_pushes_nothing() {
    let (push, mut status_rx, _guards) = wired_push();
    let backoff = ReconnectBackoff::new();
    let started = Event::SectionStarted {
        section: "chat".to_owned(),
        provenance: Provenance::new(TaskId::from(ChainId::root()), 0),
    };

    on_event(&session_event(&started), &push, &backoff);

    assert!(
        status_rx.try_recv().is_err(),
        "the status bar has nothing to say about a section starting"
    );
}
