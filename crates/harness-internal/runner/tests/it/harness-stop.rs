//! A stop and a cancel reaching the effects in flight: `stop_round` drops
//! a chat round, a tool call, and a timer and leaves the run's cancel
//! flag clear, so a `pcall` keeps the run alive while an uncaught drop
//! ends it canceled; two calls one round requested are in flight at
//! once and one stop drops both; a prompt shaped like `chat` returns to
//! its inbox after a stop during a tool call; a stop leaves the main
//! chain's wait on its inbox, so a message sent after it is read; and a
//! stop leaves any call whose tool info survives stops in flight, while a
//! cancel drops it.

use std::sync::Arc;

use harness_runner::RunSetup;
use harness_runner::recorder::MemoryRecorder;
use runvil_engine::Error;
use runvil_engine::model::{Completion, CompletionResult, ToolCall};
use serde_json::json;

use super::{
    FIRST_RUN, PendingTimer, answers_to, launch, main_waits, no_packs, run_beside, setup, until,
};
use crate::scripted::{
    Held, HeldTimer, MODEL, ScriptedBroker, calls_hold_once, held_broker, hold_host, reply,
    surviving_hold_host,
};
use crate::support::assert_canceled;

/// A main section parked under a `pcall` on a 30-second timed wait over
/// two children: one parked on a model round, the other on a held tool
/// call, each under a `pcall` that reports how the call ended. The run
/// then joins both children and returns how its own wait ended beside
/// both children's reports.
const STOPS_ALL: &str = "---\nname: stops\ndescription: d\nrunvil: 0\n\
    uses:\n  - tests/harness\n---\n\n\
    # Stops\n\n\
    ## Main\n\n```lua\n\
    local chat = tasks.spawn('## Chat')\n\
    local tool = tasks.spawn('## Tool')\n\
    local ok, err = pcall(tasks.join_any, { chat, tool }, { timeout = 30 })\n\
    local results = tasks.join({ chat, tool })\n\
    return tostring(ok) .. ':' .. err.kind .. '|' .. results[1].result .. '|' .. results[2].result\n```\n\n\
    ## Chat\n\n```lua\n\
    local ok, err = pcall(models.infer, 'held')\n\
    return 'chat:' .. tostring(ok) .. ':' .. err.kind\n```\n\n\
    ## Tool\n\n```lua\n\
    local ok, err = pcall(tools.call, 'harness/hold')\n\
    return 'tool:' .. tostring(ok) .. ':' .. err.kind\n```\n";

/// A prompt whose one round sits under nothing that catches.
const STOPS_UNCAUGHT: &str = "---\nname: uncaught\ndescription: d\nrunvil: 0\n\
    ---\n\n\
    # Uncaught\n\n\
    ## Only\n\n```lua\nreturn models.infer('held')\n```\n";

/// The built-in chat's shape: read the operator's next message, run the
/// model loop under a `pcall`, and read again, with the held tool
/// advertised to the model.
const CHATS: &str = "---\nname: chats\ndescription: d\nrunvil: 0\n\
    uses:\n  - tests/harness\n  - user\n\
    ---\n\n\
    # Chats\n\n\
    ## Conversation\n\n```lua\n\
    tools.offer('harness/hold')\n\
    while true do\n\
    \x20   messages:user(tasks.receive())\n\
    \x20   pcall(function() return models.loop() end)\n\
    end\n```\n";

/// A model loop under a `pcall` with the held tool advertised, reporting
/// how the loop call ended.
const LOOPS_ONCE: &str = "---\nname: loops\ndescription: d\nrunvil: 0\n\
    uses:\n  - tests/harness\n\
    ---\n\n\
    # Loops\n\n\
    ## Only\n\n```lua\n\
    tools.offer('harness/hold')\n\
    messages:user('hold twice')\n\
    local ok, err = pcall(models.loop)\n\
    return tostring(ok) .. ':' .. err.kind\n```\n";

/// A prompt that returns the operator's next message.
const RECEIVES: &str = "---\nname: receives\ndescription: d\nrunvil: 0\n\
    uses:\n  - user\n---\n\n\
    # Receives\n\n## Only\n\n```lua\nreturn (tasks.receive())\n```\n";

/// A prompt that returns what the held tool answers.
const HOLDS: &str = "---\nname: holds\ndescription: d\nrunvil: 0\n\
    uses:\n  - tests/harness\n---\n\n\
    # Holds\n\n## Only\n\n```lua\nreturn tools.call('harness/hold')\n```\n";

#[tokio::test]
async fn a_stop_drops_a_chat_round_a_tool_call_and_a_timer_and_a_pcall_keeps_the_run_alive() {
    let chat = Arc::new(Held::default());
    let tool = Arc::new(Held::default());
    let timer = Arc::new(Held::default());
    let recorder = Arc::new(MemoryRecorder::new());
    let harness = launch(
        hold_host(&tool),
        HeldTimer(Arc::clone(&timer)),
        STOPS_ALL,
        setup(recorder.clone(), held_broker(&chat)),
    );
    let in_flight = [Arc::clone(&chat), Arc::clone(&tool), Arc::clone(&timer)];

    let report = run_beside(harness, |control| async move {
        until(
            "a chat round, a tool call, and a timer are all in flight",
            || in_flight.iter().all(|held| held.started() == 1),
        )
        .await;
        control.stop_round();
    })
    .await
    .expect("the run is reported");

    assert_eq!(
        report.result,
        Ok("false:canceled|chat:false:canceled|tool:false:canceled".to_owned()),
        "the main's pcall caught its dropped wait, each child's pcall caught its dropped \
         call, and the run went on to its end"
    );
    for (held, what) in [(&chat, "round"), (&tool, "tool call"), (&timer, "timer")] {
        assert_eq!(held.started(), 1, "the {what} started once");
        assert_eq!(held.dropped(), 1, "the stopped {what} was torn down");
    }
    let records = recorder.records(report.run_id);
    for kind in ["Chat", "ToolCall", "Timer"] {
        assert_eq!(
            answers_to(&records, kind),
            [json!("Dropped")],
            "the stop answered the {kind} effect Dropped"
        );
    }
}

#[tokio::test]
async fn an_uncaught_stopped_round_ends_the_run_canceled() {
    let chat = Arc::new(Held::default());
    let recorder = Arc::new(MemoryRecorder::new());
    let harness = launch(
        hold_host(&Arc::default()),
        PendingTimer::default(),
        STOPS_UNCAUGHT,
        setup(recorder.clone(), held_broker(&chat)),
    );
    let watched = Arc::clone(&chat);

    let report = run_beside(harness, |control| async move {
        until("the round is in flight", || watched.started() == 1).await;
        control.stop_round();
    })
    .await
    .expect("the run is reported");

    assert_canceled(&report.result);
    let run_id = report.run_id;
    assert_eq!(recorder.result(run_id), Some(report.result));
    assert_eq!(
        answers_to(&recorder.records(run_id), "Chat"),
        [json!("Dropped")]
    );
}

/// The round that asks for the held tool twice: the model's first round
/// calls its wire name `harness_hold` as `c1` and `c2`, and every later
/// round replies with text.
fn calls_hold_twice() -> ScriptedBroker {
    ScriptedBroker::new(|round, _messages| {
        Box::pin(async move {
            if round.id.get() > 0 {
                return reply("done");
            }
            let calls = vec![
                ToolCall::from_parts("c1", "harness_hold", json!({}))?,
                ToolCall::from_parts("c2", "harness_hold", json!({}))?,
            ];
            Completion::from_result(CompletionResult::ToolCalls(calls), MODEL).map(Box::new)
        })
    })
}

#[tokio::test]
async fn two_calls_in_one_round_are_in_flight_at_once_and_a_stop_drops_both() {
    let held = Arc::new(Held::default());
    let recorder = Arc::new(MemoryRecorder::new());
    let harness = launch(
        hold_host(&held),
        PendingTimer::default(),
        LOOPS_ONCE,
        setup(recorder.clone(), calls_hold_twice()),
    );
    let watched = Arc::clone(&held);

    let report = run_beside(harness, |control| async move {
        until("both calls are in flight", || watched.started() == 2).await;
        control.stop_round();
    })
    .await
    .expect("the run is reported");

    assert_eq!(
        report.result,
        Ok("false:canceled".to_owned()),
        "the loop's pcall caught the dropped batch"
    );
    assert_eq!(held.dropped(), 2, "the stop tore both calls down");
    let records = recorder.records(report.run_id);
    assert_eq!(
        answers_to(&records, "ToolCall"),
        [json!("Dropped"), json!("Dropped")],
        "the stop answered both calls Dropped"
    );
}

#[tokio::test]
async fn a_chat_shaped_prompt_returns_to_its_inbox_after_a_stop_during_a_tool_call() {
    let tool = Arc::new(Held::default());
    let recorder = Arc::new(MemoryRecorder::new());
    let harness = launch(
        hold_host(&tool),
        PendingTimer::default(),
        CHATS,
        RunSetup {
            has_user: true,
            ..setup(recorder.clone(), calls_hold_once())
        },
    );
    let (watched, waits) = (Arc::clone(&tool), Arc::clone(&recorder));

    let report = run_beside(harness, |control| async move {
        until("the prompt waits on its inbox", || {
            main_waits(&waits.records(FIRST_RUN)) == 1
        })
        .await;
        assert!(control.send("hello".to_owned()));
        until("the model's tool call is in flight", || {
            watched.started() == 1
        })
        .await;
        control.stop_round();
        until("the prompt returns to its inbox", || {
            main_waits(&waits.records(FIRST_RUN)) == 2
        })
        .await;
        control.cancel();
    })
    .await
    .expect("the run is reported");

    assert!(
        report.result.as_ref().is_err_and(Error::is_canceled),
        "the cancel ended the run waiting on its inbox: {:?}",
        report.result
    );
    assert_eq!(tool.dropped(), 1, "the stopped tool call was torn down");
    assert!(report.unread.is_empty(), "{:?}", report.unread);
    let records = recorder.records(report.run_id);
    assert_eq!(
        answers_to(&records, "ToolCall"),
        [json!("Dropped")],
        "the stop answered the model's tool call Dropped, and reading the inbox issued none"
    );
}

#[tokio::test]
async fn a_stop_while_the_main_chain_waits_on_its_inbox_leaves_the_wait_and_a_later_message_reaches_it()
 {
    let recorder = Arc::new(MemoryRecorder::new());
    let harness = launch(
        no_packs(),
        PendingTimer::default(),
        RECEIVES,
        RunSetup {
            has_user: true,
            ..setup(recorder.clone(), ScriptedBroker::replying())
        },
    );
    let watched = Arc::clone(&recorder);

    let report = run_beside(harness, |control| async move {
        until("the main chain waits on its inbox", || {
            main_waits(&watched.records(FIRST_RUN)) == 1
        })
        .await;
        control.stop_round();
        for _ in 0..16 {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            watched.result(FIRST_RUN),
            None,
            "the stop left the run waiting on its inbox"
        );
        assert_eq!(
            main_waits(&watched.records(FIRST_RUN)),
            1,
            "the stop neither woke the wait nor began another"
        );
        assert!(control.send("kept".to_owned()));
    })
    .await
    .expect("the run is reported");

    assert_eq!(
        report.result,
        Ok("kept".to_owned()),
        "the wait outlived the stop, and the message sent after it ended the run"
    );
    let records = recorder.records(report.run_id);
    assert!(
        answers_to(&records, "ToolCall").is_empty() && answers_to(&records, "Chat").is_empty(),
        "reading the inbox issued no effect for the stop to drop: {records:?}"
    );
}

#[tokio::test]
async fn a_stop_leaves_any_tool_call_whose_info_survives_stops_in_flight() {
    let tool = Arc::new(Held::default());
    let recorder = Arc::new(MemoryRecorder::new());
    let harness = launch(
        surviving_hold_host(&tool),
        PendingTimer::default(),
        HOLDS,
        setup(recorder.clone(), ScriptedBroker::replying()),
    );
    let watched = Arc::clone(&tool);

    let report = run_beside(harness, |control| async move {
        until("the held call is in flight", || watched.started() == 1).await;
        control.stop_round();
        for _ in 0..16 {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            watched.dropped(),
            0,
            "the stop left the surviving call in flight"
        );
        control.cancel();
    })
    .await
    .expect("the run is reported");

    assert_canceled(&report.result);
    assert_eq!(tool.dropped(), 1, "the cancel tore the surviving call down");
    assert_eq!(
        answers_to(&recorder.records(report.run_id), "ToolCall"),
        [json!("Dropped")],
        "the cancel answered the surviving call Dropped"
    );
}
