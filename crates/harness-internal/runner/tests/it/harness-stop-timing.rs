//! What a stop reaches depends on when the loop sees it: a stop
//! drops a timed `tasks.join_any` while the task it joins keeps
//! waiting; a stop seen before a step's effects start drops the round in
//! flight and the run steps its chain on; a stop with only a question to
//! the operator open spares the round after its answer; and a stop raised
//! with nothing in flight changes nothing.

use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};

use harness_runner::recorder::MemoryRecorder;
use harness_runner::{Packs, RunControl, RunSetup};
use runvil::vfs::VfsRef;
use serde_json::json;

use super::{
    HookedBackend, PendingTimer, answers_to, answers_where, calls, launch, no_packs, run_beside,
    setup, until,
};
use crate::asker::ASKER;
use crate::scripted::{Held, HeldTimer, Operator, ScriptedBroker, held_broker};
use crate::support::registry;

/// A main section parked under a `pcall` on a 30-second timed
/// `tasks.join_any` over a child that asks the operator. Once the wait
/// ends, the main infers once, joins the child for the operator's answer,
/// and returns how the wait ended, the reply, and the answer.
const STOPS_TIMED_JOIN: &str = "---\nname: timed\ndescription: d\nrunvil: 0\n\
    uses:\n  - tests/user-input\n---\n\n\
    # Timed\n\n\
    ## Main\n\n```lua\n\
    local asker = tasks.spawn('## Asker')\n\
    local ok, err = pcall(tasks.join_any, { asker }, { timeout = 30 })\n\
    local after = models.infer('after')\n\
    local _t, _ok, answer = tasks.join_any({ asker })\n\
    return tostring(ok) .. ':' .. err.kind .. '|' .. after .. '|' .. answer\n```\n\n\
    ## Asker\n\n```lua\nreturn (input.ask())\n```\n";

/// A main section that spawns a child parked under a `pcall` on a model
/// round, writes a store file, and then only waits on the child, returning
/// the child's report of how its round ended.
const STOPS_BEFORE_A_STEP: &str = "---\nname: before-step\ndescription: d\nrunvil: 0\n\
    ---\n\n\
    # BeforeStep\n\n\
    ## Main\n\n```lua\n\
    local child = tasks.spawn('## Child')\n\
    store.write('note.md', 'kept')\n\
    local _t, _ok, report = tasks.join_any({ child })\n\
    return report\n```\n\n\
    ## Child\n\n```lua\n\
    local ok, err = pcall(models.infer, 'held')\n\
    return 'chat:' .. tostring(ok) .. ':' .. err.kind\n```\n";

/// A prompt that hands the operator's answer to one model round.
const ASKS_THEN_INFERS: &str = "---\nname: asks-infers\ndescription: d\nrunvil: 0\n\
    uses:\n  - tests/user-input\n---\n\n\
    # AsksInfers\n\n\
    ## Only\n\n```lua\nreturn models.infer((input.ask()))\n```\n";

/// A prompt that writes a store file, then runs one model round.
const WRITES_THEN_INFERS: &str = "---\nname: writes-infers\ndescription: d\nrunvil: 0\n\
    ---\n\n\
    # WritesInfers\n\n\
    ## Only\n\n```lua\nstore.write('note.md', 'kept')\nreturn models.infer('after')\n```\n";

/// A Host that built only the fixture ask Pack, as `user-input`, beside
/// the built-in.
fn asker_host() -> Packs {
    Packs::new(registry(&[ASKER]))
}

/// A store whose every write raises a stop through the control the
/// returned slot holds, once the test puts the run's control there.
fn stopping_store() -> (VfsRef, Arc<OnceLock<RunControl>>) {
    let slot = Arc::new(OnceLock::<RunControl>::new());
    let control = Arc::clone(&slot);
    let vfs = VfsRef::builder()
        .store(
            "/",
            HookedBackend::new(move || {
                if let Some(control) = control.get() {
                    control.stop_round();
                }
            }),
        )
        .build();
    (vfs, slot)
}

#[tokio::test]
async fn a_stop_drops_a_timed_join_any_and_its_pcall_resumes_while_the_joined_task_waits() {
    let (operator, answers) = Operator::new();
    let asked = Arc::clone(&operator.asked);
    let timer = Arc::new(Held::default());
    let broker = ScriptedBroker::replying();
    let rounds = broker.rounds();
    let recorder = Arc::new(MemoryRecorder::new());
    let harness = launch(
        asker_host(),
        HeldTimer(Arc::clone(&timer)),
        STOPS_TIMED_JOIN,
        RunSetup {
            services: operator.services(),
            ..setup(recorder.clone(), broker)
        },
    );
    let watched = Arc::clone(&timer);

    let report = run_beside(harness, |control| async move {
        until(
            "the child's question and the wait's timer are in flight",
            || asked.load(Ordering::SeqCst) == 1 && watched.started() == 1,
        )
        .await;
        control.stop_round();
        until(
            "the dropped wait's pcall resumes and the main infers",
            || rounds.lock().unwrap().len() == 1,
        )
        .await;
        answers.send("kept".to_owned()).unwrap();
    })
    .await
    .expect("the run is reported");

    assert_eq!(
        report.result,
        Ok("false:canceled|re: after|kept".to_owned()),
        "the wait raised the canceled error, and the child's question outlived the stop"
    );
    assert_eq!(timer.dropped(), 1, "the stopped timer was torn down");
    assert_eq!(
        operator.abandoned.load(Ordering::SeqCst),
        0,
        "the stop left the child's question open"
    );
    let records = recorder.records(report.run_id);
    assert_eq!(answers_to(&records, "Timer"), [json!("Dropped")]);
    let asks = answers_where(&records, "ToolCall", calls("user-input/ask"));
    assert_eq!(asks.len(), 1, "one question: {asks:?}");
    assert_eq!(asks[0]["ToolCall"]["Ok"]["text"], "kept");
}

#[tokio::test]
async fn a_stop_seen_before_a_steps_effects_start_drops_the_round_in_flight_and_its_pcall_resumes()
{
    let chat = Arc::new(Held::default());
    let recorder = Arc::new(MemoryRecorder::new());
    let (vfs, stops) = stopping_store();
    let harness = launch(
        no_packs(),
        PendingTimer::default(),
        STOPS_BEFORE_A_STEP,
        RunSetup {
            vfs,
            ..setup(recorder.clone(), held_broker(&chat))
        },
    );
    stops.set(harness.control()).unwrap();

    // The write and the child's round are issued in one step, and the
    // write runs inline and raises the stop. The loop sees the stop before
    // the next step's effects start, and that step issues none: the main
    // only waits on the child, whose dropped round is all that can wake it.
    let report = run_beside(harness, |_control| async {})
        .await
        .expect("the run is reported");

    assert_eq!(
        report.result,
        Ok("chat:false:canceled".to_owned()),
        "the child's pcall caught its dropped round, and the main joined the child"
    );
    let records = recorder.records(report.run_id);
    assert_eq!(
        answers_to(&records, "Chat"),
        [json!("Dropped")],
        "the stop answered the round in flight Dropped"
    );
}

#[tokio::test]
async fn a_stop_with_only_a_question_open_spares_the_round_after_its_answer() {
    let (operator, answers) = Operator::new();
    let asked = Arc::clone(&operator.asked);
    let broker = ScriptedBroker::replying();
    let rounds = broker.rounds();
    let recorder = Arc::new(MemoryRecorder::new());
    let harness = launch(
        asker_host(),
        PendingTimer::default(),
        ASKS_THEN_INFERS,
        RunSetup {
            services: operator.services(),
            ..setup(recorder.clone(), broker)
        },
    );

    let report = run_beside(harness, |control| async move {
        until("the prompt asks its question", || {
            asked.load(Ordering::SeqCst) == 1
        })
        .await;
        control.stop_round();
        for _ in 0..16 {
            tokio::task::yield_now().await;
        }
        answers.send("kept".to_owned()).unwrap();
    })
    .await
    .expect("the run is reported");

    assert_eq!(
        report.result,
        Ok("re: kept".to_owned()),
        "the round after the answer completed"
    );
    assert_eq!(
        rounds.lock().unwrap().len(),
        1,
        "the round reached the broker"
    );
    assert_eq!(operator.abandoned.load(Ordering::SeqCst), 0);
    let records = recorder.records(report.run_id);
    let chats = answers_to(&records, "Chat");
    assert_eq!(chats.len(), 1, "one round: {chats:?}");
    assert_ne!(
        chats[0],
        json!("Dropped"),
        "the stop never reached the round"
    );
}

#[tokio::test]
async fn a_stop_raised_with_nothing_in_flight_changes_nothing_for_the_next_effect() {
    let broker = ScriptedBroker::replying();
    let rounds = broker.rounds();
    let recorder = Arc::new(MemoryRecorder::new());
    // The write runs inline, so nothing is in flight when it raises the stop.
    let (vfs, stops) = stopping_store();
    let harness = launch(
        no_packs(),
        PendingTimer::default(),
        WRITES_THEN_INFERS,
        RunSetup {
            vfs,
            ..setup(recorder.clone(), broker)
        },
    );
    stops.set(harness.control()).unwrap();

    let report = harness.run().await.expect("the run is reported");

    assert_eq!(
        report.result,
        Ok("re: after".to_owned()),
        "the round after the stop completed"
    );
    assert_eq!(
        rounds.lock().unwrap().len(),
        1,
        "the round reached the broker"
    );
    let records = recorder.records(report.run_id);
    let chats = answers_to(&records, "Chat");
    assert_eq!(chats.len(), 1, "one round: {chats:?}");
    assert_ne!(
        chats[0],
        json!("Dropped"),
        "the stop never reached the round"
    );
}
