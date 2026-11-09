//! What a stop reaches depends on when the loop sees it: a stop
//! drops a timed `tasks.join_any` while the task it joins keeps
//! waiting on its inbox; a stop seen before a step's effects start drops
//! the round in flight and the run steps its chain on; a stop while the
//! main chain waits on its inbox spares the round after its message; and
//! a stop raised with nothing in flight changes nothing.

use std::sync::{Arc, OnceLock};

use harness_runner::recorder::MemoryRecorder;
use harness_runner::{RunControl, RunSetup};
use runvil::vfs::VfsRef;
use serde_json::json;

use super::{
    FIRST_RUN, HookedBackend, PendingTimer, answers_to, inbox_waits, launch, main_waits, no_packs,
    run_beside, setup, until,
};
use crate::scripted::{Held, HeldTimer, ScriptedBroker, held_broker};

/// A main section parked under a `pcall` on a 30-second timed
/// `tasks.join_any` over a child that waits on its inbox. Once the wait
/// ends, the main infers once, sends the child a message, joins the child
/// for it, and returns how the wait ended, the reply, and the message.
const STOPS_TIMED_JOIN: &str = "---\nname: timed\ndescription: d\nrunvil: 0\n\
    ---\n\n\
    # Timed\n\n\
    ## Main\n\n```lua\n\
    local child = tasks.spawn('## Child')\n\
    local ok, err = pcall(tasks.join_any, { child }, { timeout = 30 })\n\
    local after = models.infer('after')\n\
    tasks.send(child, 'kept')\n\
    local _t, _ok, message = tasks.join_any({ child })\n\
    return tostring(ok) .. ':' .. err.kind .. '|' .. after .. '|' .. message\n```\n\n\
    ## Child\n\n```lua\nreturn (tasks.receive())\n```\n";

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

/// A prompt that hands the operator's next message to one model round.
const RECEIVES_THEN_INFERS: &str = "---\nname: receives-infers\ndescription: d\nrunvil: 0\n\
    uses:\n  - user\n---\n\n\
    # ReceivesInfers\n\n\
    ## Only\n\n```lua\nreturn models.infer((tasks.receive()))\n```\n";

/// A prompt that writes a store file, then runs one model round.
const WRITES_THEN_INFERS: &str = "---\nname: writes-infers\ndescription: d\nrunvil: 0\n\
    ---\n\n\
    # WritesInfers\n\n\
    ## Only\n\n```lua\nstore.write('note.md', 'kept')\nreturn models.infer('after')\n```\n";

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
    let timer = Arc::new(Held::default());
    let broker = ScriptedBroker::replying();
    let recorder = Arc::new(MemoryRecorder::new());
    let harness = launch(
        no_packs(),
        HeldTimer(Arc::clone(&timer)),
        STOPS_TIMED_JOIN,
        setup(recorder.clone(), broker),
    );
    let (watched, waits) = (Arc::clone(&timer), Arc::clone(&recorder));

    let report = run_beside(harness, |control| async move {
        until(
            "the child waits on its inbox and the wait's timer is in flight",
            || inbox_waits(&waits.records(FIRST_RUN), "0.0") == 1 && watched.started() == 1,
        )
        .await;
        control.stop_round();
    })
    .await
    .expect("the run is reported");

    assert_eq!(
        report.result,
        Ok("false:canceled|re: after|kept".to_owned()),
        "the wait raised the canceled error, and the child's wait on its inbox outlived the stop"
    );
    assert_eq!(timer.dropped(), 1, "the stopped timer was torn down");
    let records = recorder.records(report.run_id);
    assert_eq!(answers_to(&records, "Timer"), [json!("Dropped")]);
    assert_eq!(
        inbox_waits(&records, "0.0"),
        1,
        "the child waited once, and the stop never woke it"
    );
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
async fn a_stop_while_the_main_chain_waits_on_its_inbox_spares_the_round_after_its_message() {
    let broker = ScriptedBroker::replying();
    let rounds = broker.rounds();
    let recorder = Arc::new(MemoryRecorder::new());
    let harness = launch(
        no_packs(),
        PendingTimer::default(),
        RECEIVES_THEN_INFERS,
        RunSetup {
            has_user: true,
            ..setup(recorder.clone(), broker)
        },
    );
    let waits = Arc::clone(&recorder);

    let report = run_beside(harness, |control| async move {
        until("the main chain waits on its inbox", || {
            main_waits(&waits.records(FIRST_RUN)) == 1
        })
        .await;
        control.stop_round();
        for _ in 0..16 {
            tokio::task::yield_now().await;
        }
        assert!(control.send("kept".to_owned()));
    })
    .await
    .expect("the run is reported");

    assert_eq!(
        report.result,
        Ok("re: kept".to_owned()),
        "the round after the message completed"
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
