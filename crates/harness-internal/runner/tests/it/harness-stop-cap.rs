//! A stop under the real performer's default cap of 10: the stop drops
//! the calls waiting for a slot without starting them, and a waiting call
//! that survives stops starts once the stop frees a slot. A task's early
//! end spares no call that survives stops: canceling a task drops its
//! held call that survives stops while the run goes on.

use std::fmt::Write as _;
use std::sync::Arc;

use harness_runner::recorder::MemoryRecorder;
use runvil_engine::model::{Completion, CompletionResult, ToolCall};
use serde_json::json;

use super::{PendingTimer, answers_to, answers_where, calls, launch, run_beside, setup, until};
use crate::scripted::{
    Held, MODEL, ScriptedBroker, hold_host, keeping_hold_host, reply, surviving_hold_host,
};
use crate::support::assert_canceled;

/// The held tool's id.
const HOLD_TOOL: &str = "harness/hold";

/// The id of the held tool that survives stops, under the keep Pack's id.
const KEEP_TOOL: &str = "keep/hold";

/// A model loop under a `pcall` that offers each of `tools` and names
/// each one's Pack, reporting how the loop call ended.
fn loops_offering(tools: &[&str]) -> String {
    let mut packs = String::new();
    let mut offers = String::new();
    for tool in tools {
        let pack = tool.split('/').next().unwrap_or_default();
        writeln!(packs, "  - tests/{pack}").unwrap();
        writeln!(offers, "tools.offer('{tool}')").unwrap();
    }
    format!(
        "---\nname: loops\ndescription: d\nrunvil: 0\nuses:\n{packs}\
         ---\n\n\
         # Loops\n\n\
         ## Only\n\n```lua\n{offers}messages:user('hold')\n\
         local ok, err = pcall(models.loop)\n\
         return tostring(ok) .. ':' .. err.kind\n```\n"
    )
}

/// A broker whose first round calls each of `wire_names` in order, as
/// `c1` onward, and whose every later round replies with text.
fn calls_once(wire_names: Vec<&'static str>) -> ScriptedBroker {
    ScriptedBroker::new(move |round, _messages| {
        let wire_names = wire_names.clone();
        Box::pin(async move {
            if round.id.get() > 0 {
                return reply("done");
            }
            let calls = wire_names
                .into_iter()
                .enumerate()
                .map(|(index, name)| {
                    ToolCall::from_parts(format!("c{}", index + 1), name, json!({}))
                })
                .collect::<Result<Vec<_>, _>>()?;
            Completion::from_result(CompletionResult::ToolCalls(calls), MODEL).map(Box::new)
        })
    })
}

/// Yields enough for the run beside the test to settle, so a check that
/// a call has not started is not vacuous.
async fn settle() {
    for _ in 0..16 {
        tokio::task::yield_now().await;
    }
}

/// Main spawns Child, which makes a held call that survives stops; Main
/// then waits on a `wait` round, cancels Child, and waits on an `after`
/// round.
const CANCELS_A_HOLDER: &str = "---\nname: cancels\ndescription: d\nrunvil: 0\n\
    uses:\n  - tests/harness\n---\n\n\
    # Cancels\n\n\
    ## Main\n\n```lua\n\
    local t = tasks.spawn('../Child')\n\
    models.infer('wait')\n\
    tasks.cancel(t)\n\
    models.infer('after')\n\
    return 'canceled'\n```\n\n\
    ## Child\n\n```lua\nreturn tools.call('harness/hold')\n```\n";

/// A broker that answers the `wait` round once the held call has started,
/// and every other round once it has been torn down. It waits without
/// bound, so a call left in flight fails the test through `run_beside`'s
/// patience.
fn holds_back(held: &Arc<Held>) -> ScriptedBroker {
    let held = Arc::clone(held);
    ScriptedBroker::new(move |_round, messages| {
        let text = messages
            .last()
            .map(|message| message.content().to_owned())
            .unwrap_or_default();
        let held = Arc::clone(&held);
        Box::pin(async move {
            let reached = |held: &Held| {
                if text == "wait" {
                    held.started() > 0
                } else {
                    held.dropped() > 0
                }
            };
            while !reached(&held) {
                tokio::task::yield_now().await;
            }
            reply(format!("re: {text}"))
        })
    })
}

#[tokio::test]
async fn a_stop_drops_waiting_calls_without_starting_them() {
    let held = Arc::new(Held::default());
    let recorder = Arc::new(MemoryRecorder::new());
    let harness = launch(
        hold_host(&held),
        PendingTimer::default(),
        &loops_offering(&[HOLD_TOOL]),
        setup(recorder.clone(), calls_once(vec!["harness_hold"; 11])),
    );
    let watched = Arc::clone(&held);

    let report = run_beside(harness, |control| async move {
        until("ten calls are in flight", || watched.started() >= 10).await;
        settle().await;
        assert_eq!(watched.started(), 10, "the eleventh call waits for a slot");
        control.stop_round();
    })
    .await
    .expect("the run is reported");

    assert_eq!(
        report.result,
        Ok("false:canceled".to_owned()),
        "the loop's pcall caught the dropped batch"
    );
    assert_eq!(held.started(), 10, "the waiting call never started");
    assert_eq!(held.dropped(), 10, "the stop tore down the ten in flight");
    let records = recorder.records(report.run_id);
    assert_eq!(
        answers_to(&records, "ToolCall"),
        vec![json!("Dropped"); 11],
        "the stop answered every call Dropped, the waiting one included"
    );
}

#[tokio::test]
async fn a_stop_starts_a_waiting_call_that_survives_stops() {
    let held = Arc::new(Held::default());
    let kept = Arc::new(Held::default());
    let recorder = Arc::new(MemoryRecorder::new());
    let mut wire_names = vec!["harness_hold"; 10];
    wire_names.push("keep_hold");
    let harness = launch(
        keeping_hold_host(&held, &kept),
        PendingTimer::default(),
        &loops_offering(&[HOLD_TOOL, KEEP_TOOL]),
        setup(recorder.clone(), calls_once(wire_names)),
    );
    let (watched, survivor) = (Arc::clone(&held), Arc::clone(&kept));

    let report = run_beside(harness, |control| async move {
        until("ten holds are in flight", || watched.started() >= 10).await;
        settle().await;
        assert_eq!(
            survivor.started(),
            0,
            "the surviving call waits behind the ten holds"
        );
        control.stop_round();
        until("the stop frees a slot for the surviving call", || {
            survivor.started() == 1
        })
        .await;
        settle().await;
        assert_eq!(survivor.dropped(), 0, "the stop spared the surviving call");
        control.cancel();
    })
    .await
    .expect("the run is reported");

    assert_canceled(&report.result);
    assert_eq!(held.started(), 10);
    assert_eq!(held.dropped(), 10, "the stop tore down the ten holds");
    assert_eq!(kept.dropped(), 1, "the cancel tore down the surviving call");
    let records = recorder.records(report.run_id);
    assert_eq!(
        answers_where(&records, "ToolCall", calls(HOLD_TOOL)),
        vec![json!("Dropped"); 10]
    );
    assert_eq!(
        answers_where(&records, "ToolCall", calls(KEEP_TOOL)),
        vec![json!("Dropped")],
        "the one surviving call, started after the stop, was dropped by the cancel"
    );
}

#[tokio::test]
async fn canceling_a_task_drops_its_call_that_survives_stops() {
    let held = Arc::new(Held::default());
    let recorder = Arc::new(MemoryRecorder::new());
    let harness = launch(
        surviving_hold_host(&held),
        PendingTimer::default(),
        CANCELS_A_HOLDER,
        setup(recorder.clone(), holds_back(&held)),
    );

    let report = run_beside(harness, |_control| async {})
        .await
        .expect("the run is reported");

    assert_eq!(
        report.result,
        Ok("canceled".to_owned()),
        "the run went on past the canceled task"
    );
    assert_eq!(held.dropped(), 1, "canceling the task tore its call down");
    let records = recorder.records(report.run_id);
    assert_eq!(
        answers_where(&records, "ToolCall", calls(HOLD_TOOL)),
        vec![json!("Dropped")],
        "the call that survives stops was answered Dropped"
    );
}
