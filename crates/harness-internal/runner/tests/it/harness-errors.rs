//! How `run` ends a run that never reaches the Engine, and what it
//! returns when the recorder refuses: a model list the broker cannot
//! fetch ends the recorded run with the broker's failure, a list without
//! the selection with a `RequirementsUnmet` failure, and a cancel before
//! `run` or while the broker holds its list with the cancel, each before
//! the prompt is parsed; a recorder that refuses a write is a
//! `RecorderFailed` failure naming the run id it issued; and a broker
//! that panics answers its round `Dropped`.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::task::Poll;

use harness_runner::display_chain;
use harness_runner::environment::HostSnapshot;
use harness_runner::recorder::{MemoryRecorder, RunId};
use harness_runner::{Harness, Packs, RunReport, RunRequest};
use runvil::model::{Completion, phrase};
use runvil::{Error, ErrorCode, ErrorFact};
use runvil_plugin::{HostServices, PackRegistry};

use super::{PendingTimer, answers_to, plain_harness, request, run_beside, until};
use crate::scripted::{Listing, ScriptedBroker};
use crate::support::{FailingRecorder, assert_not_completed};

/// A Pack-free prompt whose one section returns a constant.
const PLAIN: &str = "---\nname: plain\ndescription: d\nrunvil: 0\n---\n\n\
    # Title\n\n## Only\n\n```lua\nreturn 'plain'\n```\n";

/// The run a recorder failure names in its `Run` fact, if any.
fn run_of(error: &Error) -> Option<RunId> {
    error
        .facts
        .get(&ErrorFact::Run)
        .and_then(serde_json::Value::as_i64)
        .map(RunId::from_raw)
}

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
        Arc::new(Packs::new(PackRegistry::new(), HostServices::new())),
        HostServices::new(),
    )
}

/// Asserts that `report` is of a run that ended before its prompt was
/// parsed: the recorder began it, ended it with the reported result, and
/// holds no record between, and the report has no output.
#[track_caller]
fn assert_ended_before_the_parse(recorder: &FailingRecorder, report: &RunReport) {
    assert_eq!(recorder.begun(), [report.run_id], "the run began");
    assert_eq!(
        recorder.inner().result(report.run_id),
        Some(report.result.clone()),
        "the recorder ends the run with the reported result"
    );
    assert!(
        recorder.inner().records(report.run_id).is_empty(),
        "the run ended before its prompt was parsed"
    );
    assert_eq!(recorder.calls(), 2, "the run's begin and its end");
    assert_not_completed(&report.output);
}

#[tokio::test]
async fn a_model_list_the_broker_cannot_fetch_ends_the_recorded_run_with_the_brokers_failure() {
    let recorder = Arc::new(FailingRecorder::never_failing());
    let code = ErrorCode::Transport;
    let broker = ScriptedBroker::replying().listing(Listing::Fails(code));

    let report = harness_over(&recorder, broker)
        .run(request(PLAIN))
        .await
        .expect("a failed listing is the run's result");
    let error = report.result.as_ref().expect_err("the lookup failed");
    assert_eq!(error.code, code, "the code is the broker's");
    assert_eq!(
        error.located_message(),
        format!("the model catalog could not be fetched: {}", phrase(code)),
        "the message is the lookup's, the broker's message included"
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
        .expect("a selection the list lacks is the run's result");
    let error = report.result.as_ref().expect_err("the lookup failed");
    assert_eq!(error.code, ErrorCode::RequirementsUnmet);
    assert_eq!(
        error.located_message(),
        "the selected model `absent` is absent from the fetched catalog"
    );
    assert_ended_before_the_parse(&recorder, &report);
}

#[tokio::test]
async fn a_cancel_while_the_broker_holds_its_list_ends_the_recorded_run_canceled() {
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
    .expect("a cancel during the model lookup is the run's result");
    assert_eq!(report.result, Err(Error::canceled()));
    assert_ended_before_the_parse(&recorder, &report);
}

#[tokio::test]
async fn a_cancel_before_run_ends_the_recorded_run_canceled_against_a_ready_listing() {
    let recorder = Arc::new(FailingRecorder::never_failing());
    let harness = harness_over(&recorder, ScriptedBroker::replying());
    harness.control().cancel();

    let report = harness
        .run(request(PLAIN))
        .await
        .expect("a cancel before the run begins is the run's result");
    assert_eq!(report.result, Err(Error::canceled()));
    assert_ended_before_the_parse(&recorder, &report);
}

#[tokio::test]
async fn a_recorder_failure_returns_the_recorder_error_holding_the_issued_run_id() {
    let refuses_begin = Arc::new(FailingRecorder::failing_on(1));
    let error = harness_over(&refuses_begin, ScriptedBroker::replying())
        .run(request(STORES))
        .await
        .expect_err("a refused begin is the run's error");
    assert_eq!(
        error.code,
        ErrorCode::RecorderFailed,
        "the refusal is the recorder's"
    );
    assert_eq!(run_of(&error), None, "a refused begin issued no run");
    assert!(
        error
            .message
            .starts_with("the run could not be recorded: the run recorder failed: "),
        "{error:?}"
    );

    // A run that ends before its prompt is parsed has one write after its
    // begin: its end.
    let refuses_end = Arc::new(FailingRecorder::failing_on(2));
    let harness = harness_over(&refuses_end, ScriptedBroker::replying());
    harness.control().cancel();
    let error = harness
        .run(request(STORES))
        .await
        .expect_err("a refused end is the run's error");
    assert_eq!(error.code, ErrorCode::RecorderFailed);
    assert_eq!(
        run_of(&error),
        Some(refuses_end.begun()[0]),
        "the error holds the issued run id"
    );
    assert_eq!(refuses_end.inner().result(refuses_end.begun()[0]), None);

    // Every later call names the run the recorder issued: the parse's
    // events in preparation, the loop's records, and the run's end.
    let baseline = Arc::new(FailingRecorder::never_failing());
    harness_over(&baseline, ScriptedBroker::replying())
        .run(request(STORES))
        .await
        .expect("the baseline run is reported");
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
        assert_eq!(
            error.code,
            ErrorCode::RecorderFailed,
            "call {call} of {total}: the refusal is the recorder's"
        );
        let issued = recorder.begun();
        assert_eq!(issued.len(), 1, "call {call} of {total}: one run began");
        assert_eq!(
            run_of(&error),
            Some(issued[0]),
            "call {call} of {total}: the error holds the issued run id"
        );
        assert!(
            display_chain(&error).contains(&format!("refuses call {call}")),
            "call {call} of {total}: {}",
            display_chain(&error)
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
            report.result,
            Ok("false:canceled".to_owned()),
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
