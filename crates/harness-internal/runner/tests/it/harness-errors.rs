//! How `run` ends a run that never reaches the Engine, and what it
//! returns when the recorder refuses: a model list the broker cannot
//! fetch ends the recorded run as a `Completion` failure, a list without
//! the selection as a `RequirementsUnmet` failure, and a cancel before
//! `run` or while the broker holds its list as cancelled, each before the
//! prompt is parsed; a recorder that refuses a write is
//! `HarnessError::Recorder` holding the run id it issued; and a broker
//! that panics answers its round `Dropped`.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::task::Poll;

use harness_runner::display_chain;
use harness_runner::environment::{CurrentModelError, HostSnapshot};
use harness_runner::recorder::{MemoryRecorder, RunErrorKind};
use harness_runner::{Harness, HarnessError, HostContext, RunReport, RunRequest};
use runvil::error::{Error, ErrorCode};
use runvil::model::{Completion, completion_error, phrase};
use runvil_plugin::{HostServices, PackRegistry};

use super::{
    OutputError, PendingTimer, RunOutcome, answers_to, plain_harness, request, run_beside, until,
};
use crate::scripted::{Listing, ScriptedBroker};
use crate::support::FailingRecorder;

/// A Pack-free prompt whose one section returns a constant.
const PLAIN: &str = "---\nname: plain\ndescription: d\nrunvil: 0\n---\n\n\
    # Title\n\n## Only\n\n```lua\nreturn 'plain'\n```\n";

/// A prompt of one store write and one read.
const STORES: &str = "---\nname: stores\ndescription: d\nrunvil: 0\n---\n\n\
    # Title\n\n## Only\n\n```lua\nstore.write('a.md', '1')\nreturn store.read('a.md')\n```\n";

/// A prompt whose one round sits under a `pcall` that reports how it
/// ended.
const CATCHES_A_ROUND: &str = "---\nname: catches\ndescription: d\nrunvil: 0\n\
    ---\n\n\
    # Title\n\n## Only\n\n```lua\n\
    local ok, err = pcall(models.infer, 'asked')\n\
    return tostring(ok) .. ':' .. err.kind\n```\n";

/// A Harness over `recorder` and `broker` with no Packs and no
/// Host services.
fn harness_over(recorder: &Arc<FailingRecorder>, broker: ScriptedBroker) -> Harness {
    Harness::new(
        recorder.clone(),
        Arc::new(broker),
        Arc::new(PendingTimer::default()),
        Arc::new(HostContext::new(PackRegistry::new(), HostServices::new())),
        HostServices::new(),
    )
}

/// Asserts that `report` is of a run that ended before its prompt was
/// parsed: the recorder began it, ended it with the reported outcome, and
/// holds no record between, and the report has no output.
#[track_caller]
fn assert_ended_before_the_parse(recorder: &FailingRecorder, report: &RunReport) {
    assert_eq!(recorder.begun(), [report.run_id], "the run began");
    assert_eq!(
        recorder.inner().outcome(report.run_id),
        Some(report.outcome.clone()),
        "the recorder ends the run with the reported outcome"
    );
    assert!(
        recorder.inner().records(report.run_id).is_empty(),
        "the run ended before its prompt was parsed"
    );
    assert_eq!(recorder.calls(), 2, "the run's begin and its end");
    assert_eq!(report.output, Err(OutputError::NotCompleted));
}

#[tokio::test]
async fn a_model_list_the_broker_cannot_fetch_ends_the_recorded_run_as_a_completion_failure() {
    let recorder = Arc::new(FailingRecorder::never_failing());
    let code = ErrorCode::Transport;
    let broker = ScriptedBroker::replying().listing(Listing::Fails(code));

    let report = harness_over(&recorder, broker)
        .run(request(PLAIN))
        .await
        .expect("a failed listing is the run's outcome");
    let lookup = CurrentModelError::CatalogFetchFailed(completion_error(code, phrase(code)));
    assert_eq!(
        report.outcome,
        RunOutcome::Failed {
            kind: RunErrorKind::Completion,
            message: display_chain(&lookup),
        },
        "the message is the lookup's cause chain, the broker's message included"
    );
    assert_ended_before_the_parse(&recorder, &report);
}

#[tokio::test]
async fn a_selection_absent_from_the_model_list_ends_the_recorded_run_as_an_unmet_requirement() {
    let recorder = Arc::new(FailingRecorder::never_failing());
    let request = RunRequest {
        host: HostSnapshot {
            selected_model: Some("absent".to_owned()),
            ..HostSnapshot::default()
        },
        ..request(PLAIN)
    };

    let report = harness_over(&recorder, ScriptedBroker::replying())
        .run(request)
        .await
        .expect("a selection the list lacks is the run's outcome");
    assert_eq!(
        report.outcome,
        RunOutcome::Failed {
            kind: RunErrorKind::RequirementsUnmet,
            message: "the selected model `absent` is absent from the fetched catalog".to_owned(),
        }
    );
    assert_ended_before_the_parse(&recorder, &report);
}

#[tokio::test]
async fn a_cancel_while_the_broker_holds_its_list_ends_the_recorded_run_cancelled() {
    let recorder = Arc::new(FailingRecorder::never_failing());
    let broker = ScriptedBroker::replying().listing(Listing::Holds);
    let listings = broker.listings();

    let report = run_beside(
        harness_over(&recorder, broker),
        request(PLAIN),
        |control| async move {
            until("the run asks for the model list", || {
                listings.load(Ordering::SeqCst) == 1
            })
            .await;
            control.cancel();
        },
    )
    .await
    .expect("a cancel during the model lookup is an outcome");
    assert_eq!(report.outcome, RunOutcome::Cancelled);
    assert_ended_before_the_parse(&recorder, &report);
}

#[tokio::test]
async fn a_cancel_before_run_ends_the_recorded_run_cancelled_against_a_ready_listing() {
    let recorder = Arc::new(FailingRecorder::never_failing());
    let harness = harness_over(&recorder, ScriptedBroker::replying());
    harness.control().cancel();

    let report = harness
        .run(request(PLAIN))
        .await
        .expect("a cancel before the run begins is an outcome");
    assert_eq!(report.outcome, RunOutcome::Cancelled);
    assert_ended_before_the_parse(&recorder, &report);
}

#[tokio::test]
async fn a_recorder_failure_returns_the_recorder_error_holding_the_issued_run_id() {
    let refuses_begin = Arc::new(FailingRecorder::failing_on(1));
    let error = harness_over(&refuses_begin, ScriptedBroker::replying())
        .run(request(STORES))
        .await
        .expect_err("a refused begin is the run's error");
    let HarnessError::Recorder { run, .. } = error else {
        panic!("the refusal is the recorder's: {error:?}");
    };
    assert_eq!(run, None, "a refused begin issued no run");

    // A run that ends before its prompt is parsed has one write after its
    // begin: its end.
    let refuses_end = Arc::new(FailingRecorder::failing_on(2));
    let harness = harness_over(&refuses_end, ScriptedBroker::replying());
    harness.control().cancel();
    let error = harness
        .run(request(STORES))
        .await
        .expect_err("a refused end is the run's error");
    let HarnessError::Recorder { run, .. } = error else {
        panic!("the refusal is the recorder's: {error:?}");
    };
    assert_eq!(
        run,
        Some(refuses_end.begun()[0]),
        "the error holds the issued run id"
    );
    assert_eq!(refuses_end.inner().outcome(refuses_end.begun()[0]), None);

    // Every later call names the run the recorder issued: the parse's
    // events in preparation, the loop's records, and the run's end.
    let baseline = Arc::new(FailingRecorder::never_failing());
    harness_over(&baseline, ScriptedBroker::replying())
        .run(request(STORES))
        .await
        .expect("the baseline run reaches an outcome");
    let total = baseline.calls();
    assert!(
        total >= 6,
        "begin, events, two effects, two answers, and the end"
    );
    for call in 2..=total {
        let recorder = Arc::new(FailingRecorder::failing_on(call));
        let error = harness_over(&recorder, ScriptedBroker::replying())
            .run(request(STORES))
            .await
            .expect_err("a refused write is the run's error");
        let HarnessError::Recorder { run, source } = error else {
            panic!("call {call} of {total}: the refusal is the recorder's: {error:?}");
        };
        let issued = recorder.begun();
        assert_eq!(issued.len(), 1, "call {call} of {total}: one run began");
        assert_eq!(
            run,
            Some(issued[0]),
            "call {call} of {total}: the error holds the issued run id"
        );
        assert!(
            display_chain(&source).contains(&format!("refuses call {call}")),
            "call {call} of {total}: {}",
            display_chain(&source)
        );
        assert_eq!(recorder.calls(), call, "nothing is sent after the refusal");
    }
}

#[tokio::test]
async fn a_panicking_broker_answers_its_round_dropped() {
    let in_the_call = ScriptedBroker::new(|_round, _messages| {
        panic!("the broker panics in its call");
    });
    let in_the_round = ScriptedBroker::new(|_round, _messages| {
        Box::pin(std::future::poll_fn(
            |_cx| -> Poll<Result<Box<Completion>, Error>> {
                panic!("the broker panics in its round")
            },
        ))
    });
    for (what, broker) in [("call", in_the_call), ("round", in_the_round)] {
        let recorder = Arc::new(MemoryRecorder::new());
        let report = plain_harness(&recorder, broker)
            .run(request(CATCHES_A_ROUND))
            .await
            .expect("a panicking broker does not end the run with an error");
        assert_eq!(
            report.outcome,
            RunOutcome::Completed {
                final_text: "false:cancelled".to_owned()
            },
            "a panic in the {what}: the pcall caught the dropped round"
        );
        let records = recorder.records(report.run_id);
        assert_eq!(
            answers_to(&records, "Chat"),
            [serde_json::json!("Dropped")],
            "a panic in the {what}: the round is answered Dropped"
        );
    }
}
