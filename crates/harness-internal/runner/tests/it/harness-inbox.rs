//! The main chain's inbox through `RunControl::send`: each send while the
//! run waits on its inbox resumes it, a send before the run starts is
//! delivered before its first step, and a send after the run ends returns
//! false; the recorder holds a sent message's `InboxMessage` event ahead
//! of the records of the step that applies it; a cancel ends a run that
//! waits for input; a stop never reaches a message, whether it was sent
//! before the stop or after it, so the next `tasks.receive()` returns it;
//! a message sent once a round's answer has landed leaves that answer
//! standing, and the loop asks again with the message; and every message
//! a run never read comes back on its report, the ones the loop never
//! delivered last.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use harness_runner::recorder::{MemoryRecorder, Record, RecordKind, RunId};
use harness_runner::{RunControl, RunSetup};
use runvil::vfs::VfsRef;
use serde_json::json;
use tokio::sync::Notify;

use super::{
    HookedBackend, PendingTimer, answers_to, launch, no_packs, plain_harness, run_beside, setup,
    until,
};
use crate::scripted::{Held, ScriptedBroker, calls_hold_once, held_broker, hold_host, reply};
use crate::support::assert_canceled;

/// The run every test here drives: the first one a fresh recorder begins.
const FIRST_RUN: RunId = RunId::from_raw(1);

/// A prompt that returns the first message its main chain reads.
const RECEIVES: &str = "---\nname: receives\ndescription: d\nrunvil: 0\n---\n\n\
    # Receives\n\n## Only\n\n```lua\nreturn 'got ' .. tasks.receive()\n```\n";

/// A prompt that returns the first two messages its main chain reads.
const RECEIVES_TWICE: &str = "---\nname: receives-twice\ndescription: d\nrunvil: 0\n---\n\n\
    # ReceivesTwice\n\n## Only\n\n```lua\n\
    local first = tasks.receive()\n\
    local second = tasks.receive()\n\
    return first .. '|' .. second\n```\n";

/// A prompt that reads one message, logs that it did, writes the message
/// to the store, and returns it.
const RECEIVES_THEN_WRITES: &str = "---\nname: receives-writes\ndescription: d\nrunvil: 0\n\
    ---\n\n\
    # ReceivesWrites\n\n## Only\n\n```lua\n\
    local text = tasks.receive()\n\
    log('read')\n\
    store.write('note.md', text)\n\
    return text\n```\n";

/// A model loop under a `pcall` with the held tool advertised, then the
/// next message the main chain reads, beside how the loop ended.
const LOOPS_THEN_RECEIVES: &str = "---\nname: loops-receives\ndescription: d\nrunvil: 0\n\
    uses:\n  - tests/harness\n---\n\n\
    # LoopsReceives\n\n## Only\n\n```lua\n\
    tools.offer('harness/hold')\n\
    messages:user('hold')\n\
    local ok, err = pcall(models.loop)\n\
    return tostring(ok) .. ':' .. err.kind .. '|' .. tasks.receive()\n```\n";

/// A model loop over one user message that returns the loop's final
/// reply.
const LOOPS: &str = "---\nname: loops\ndescription: d\nrunvil: 0\n---\n\n\
    # Loops\n\n## Only\n\n```lua\n\
    messages:user('hi')\n\
    models.loop()\n\
    return messages[#messages].content\n```\n";

/// Whether `record` is an event of `kind`.
fn is_event(record: &Record, kind: &str) -> bool {
    record.kind == RecordKind::Event && record.payload["kind"] == kind
}

/// How many times `records` show the main chain parked on its empty
/// inbox.
fn main_waits(records: &[Record]) -> usize {
    records
        .iter()
        .filter(|record| record.task_id == "0" && is_event(record, "inbox_waiting"))
        .count()
}

/// The text of each message whose `InboxMessage` event `records` hold, in
/// record order.
fn messages(records: &[Record]) -> Vec<String> {
    records
        .iter()
        .filter(|record| is_event(record, "inbox_message"))
        .filter_map(|record| record.payload["text"].as_str().map(str::to_owned))
        .collect()
}

/// Where the first record of `records` that `wanted` takes sits, failing
/// naming `what` when there is none.
#[track_caller]
fn position(records: &[Record], what: &str, wanted: impl Fn(&Record) -> bool) -> usize {
    records
        .iter()
        .position(wanted)
        .unwrap_or_else(|| panic!("the record holds {what}: {records:?}"))
}

/// A store whose every write sends `text` through the control the
/// returned slot holds, once the test puts the run's control there.
fn sending_store(text: &'static str) -> (VfsRef, Arc<OnceLock<RunControl>>) {
    let slot = Arc::new(OnceLock::<RunControl>::new());
    let control = Arc::clone(&slot);
    let vfs = VfsRef::builder()
        .store(
            "/",
            HookedBackend::new(move || {
                if let Some(control) = control.get() {
                    assert!(control.send(text.to_owned()), "the live run accepts {text}");
                }
            }),
        )
        .build();
    (vfs, slot)
}

/// A broker whose first round replies `first` once `gate` opens, and whose
/// every later round replies `re: <last message>` at once.
fn gated_broker(gate: &Arc<Notify>) -> ScriptedBroker {
    let gate = Arc::clone(gate);
    let calls = AtomicUsize::new(0);
    ScriptedBroker::new(move |_round, messages| {
        if calls.fetch_add(1, Ordering::SeqCst) > 0 {
            let asked = messages
                .last()
                .map(|message| message.content().to_owned())
                .unwrap_or_default();
            return Box::pin(async move { reply(format!("re: {asked}")) });
        }
        let gate = Arc::clone(&gate);
        Box::pin(async move {
            gate.notified().await;
            reply("first")
        })
    })
}

#[tokio::test]
async fn each_send_while_the_run_waits_on_its_inbox_resumes_it() {
    let recorder = Arc::new(MemoryRecorder::new());
    let harness = plain_harness(&recorder, ScriptedBroker::replying(), RECEIVES_TWICE);
    let watched = Arc::clone(&recorder);

    let report = run_beside(harness, |control| async move {
        until("the main chain waits on its inbox", || {
            main_waits(&watched.records(FIRST_RUN)) == 1
        })
        .await;
        assert!(
            control.send("one".to_owned()),
            "the live run accepts the message"
        );
        until("the main chain waits on its inbox again", || {
            main_waits(&watched.records(FIRST_RUN)) == 2
        })
        .await;
        assert!(
            control.send("two".to_owned()),
            "the live run accepts the message"
        );
    })
    .await
    .expect("the run is reported");

    assert_eq!(report.result, Ok("one|two".to_owned()));
    assert!(
        report.unread.is_empty(),
        "both messages were read: {:?}",
        report.unread
    );
}

#[tokio::test]
async fn a_send_before_the_run_starts_is_delivered_before_its_first_step() {
    let recorder = Arc::new(MemoryRecorder::new());
    let harness = plain_harness(&recorder, ScriptedBroker::replying(), RECEIVES);
    assert!(
        harness.control().send("early".to_owned()),
        "a run that has not started accepts the message"
    );

    let report = harness.run().await.expect("the run is reported");

    assert_eq!(report.result, Ok("got early".to_owned()));
    let records = recorder.records(report.run_id);
    assert!(
        is_event(&records[0], "inbox_message") && records[0].payload["text"] == "early",
        "the message leads the record: {records:?}"
    );
    assert!(
        is_event(&records[1], "run_started"),
        "the run's first step comes after the message: {records:?}"
    );
    assert_eq!(
        main_waits(&records),
        0,
        "the first read found the message and never waited"
    );
}

#[tokio::test]
async fn a_send_after_the_run_ends_returns_false_and_drops_the_text() {
    let recorder = Arc::new(MemoryRecorder::new());
    let harness = plain_harness(&recorder, ScriptedBroker::replying(), RECEIVES);
    let control = harness.control();
    assert!(control.send("only".to_owned()));

    let report = harness.run().await.expect("the run is reported");

    assert_eq!(report.result, Ok("got only".to_owned()));
    assert!(
        !control.send("late".to_owned()),
        "the ended run refuses the message"
    );
    assert!(report.unread.is_empty(), "{:?}", report.unread);
    assert_eq!(
        messages(&recorder.records(report.run_id)),
        ["only"],
        "the refused text reached no record"
    );
}

#[tokio::test]
async fn a_run_that_ends_before_it_starts_reports_every_message_sent_to_it_unread() {
    let recorder = Arc::new(MemoryRecorder::new());
    let harness = plain_harness(&recorder, ScriptedBroker::replying(), RECEIVES);
    let control = harness.control();
    control.cancel();
    for text in ["one", "two"] {
        assert!(control.send(text.to_owned()));
    }

    let report = harness.run().await.expect("the run is reported");

    assert_canceled(&report.result);
    assert_eq!(
        report.unread,
        ["one", "two"],
        "the run never started, so it read neither, oldest first"
    );
    assert!(
        recorder.records(report.run_id).is_empty(),
        "nothing was delivered, so nothing was recorded"
    );
    assert!(
        !control.send("late".to_owned()),
        "the ended run refuses the message"
    );
}

#[tokio::test]
async fn the_recorder_holds_a_sent_messages_event_ahead_of_the_records_of_the_step_that_applies_it()
{
    let recorder = Arc::new(MemoryRecorder::new());
    let harness = plain_harness(&recorder, ScriptedBroker::replying(), RECEIVES_THEN_WRITES);
    let watched = Arc::clone(&recorder);

    let report = run_beside(harness, |control| async move {
        until("the main chain waits on its inbox", || {
            main_waits(&watched.records(FIRST_RUN)) == 1
        })
        .await;
        assert!(control.send("hello".to_owned()));
    })
    .await
    .expect("the run is reported");

    assert_eq!(report.result, Ok("hello".to_owned()));
    let records = recorder.records(report.run_id);
    let waited = position(&records, "the wait", |record| {
        is_event(record, "inbox_waiting")
    });
    let sent = position(&records, "the message", |record| {
        is_event(record, "inbox_message")
    });
    let logged = position(&records, "the read's log", |record| is_event(record, "lua"));
    let written = position(&records, "the write", |record| {
        record.kind == RecordKind::Effect
    });
    assert_eq!(
        sent,
        waited + 1,
        "the loop recorded the message as it delivered it: {records:?}"
    );
    assert!(
        sent < logged && logged < written,
        "the step that applied the message reported its log and issued its write after it: \
         {records:?}"
    );
    let message = &records[sent];
    assert_eq!(message.task_id, "0", "the message is the main chain's");
    assert_eq!(message.payload["text"], json!("hello"));
    assert!(
        message.task_seq < records[logged].task_seq,
        "the message's stamp comes before the step's own: {records:?}"
    );
}

#[tokio::test]
async fn a_cancel_while_the_run_waits_for_input_ends_it_canceled() {
    let recorder = Arc::new(MemoryRecorder::new());
    let harness = plain_harness(&recorder, ScriptedBroker::replying(), RECEIVES);
    let watched = Arc::clone(&recorder);

    let report = run_beside(harness, |control| async move {
        until("the main chain waits on its inbox", || {
            main_waits(&watched.records(FIRST_RUN)) == 1
        })
        .await;
        for _ in 0..16 {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            watched.result(FIRST_RUN),
            None,
            "the run waits for input with nothing in flight and is still open"
        );
        control.cancel();
    })
    .await
    .expect("the run is reported");

    assert_canceled(&report.result);
    assert_eq!(recorder.result(report.run_id), Some(report.result.clone()));
    assert!(report.unread.is_empty(), "{:?}", report.unread);
}

#[tokio::test]
async fn a_stop_during_a_tool_call_leaves_a_message_sent_before_it_queued_for_the_next_receive() {
    let tool = Arc::new(Held::default());
    let broker = calls_hold_once();
    let rounds = broker.rounds();
    let recorder = Arc::new(MemoryRecorder::new());
    let harness = launch(
        hold_host(&tool),
        PendingTimer::default(),
        LOOPS_THEN_RECEIVES,
        setup(recorder.clone(), broker),
    );
    let (watched, delivered) = (Arc::clone(&tool), Arc::clone(&recorder));

    let report = run_beside(harness, |control| async move {
        until("the model's tool call is in flight", || {
            watched.started() == 1
        })
        .await;
        assert!(control.send("queued".to_owned()));
        until("the loop delivers the message", || {
            messages(&delivered.records(FIRST_RUN)) == ["queued"]
        })
        .await;
        control.stop_round();
    })
    .await
    .expect("the run is reported");

    assert_eq!(
        report.result,
        Ok("false:canceled|queued".to_owned()),
        "the stop raised canceled at the loop, and the message waited for the next read"
    );
    assert_eq!(tool.dropped(), 1, "the stop tore the tool call down");
    assert_eq!(
        rounds.lock().unwrap().len(),
        1,
        "the message reached no model round"
    );
    assert_eq!(
        answers_to(&recorder.records(report.run_id), "ToolCall"),
        [json!("Dropped")]
    );
}

#[tokio::test]
async fn a_stop_then_a_send_while_a_round_is_in_flight_raise_canceled_and_the_next_receive_returns_the_message()
 {
    let chat = Arc::new(Held::default());
    let recorder = Arc::new(MemoryRecorder::new());
    let harness = launch(
        hold_host(&Arc::default()),
        PendingTimer::default(),
        LOOPS_THEN_RECEIVES,
        setup(recorder.clone(), held_broker(&chat)),
    );
    let watched = Arc::clone(&chat);

    let report = run_beside(harness, |control| async move {
        until("the model round is in flight", || watched.started() == 1).await;
        control.stop_round();
        assert!(control.send("after".to_owned()));
    })
    .await
    .expect("the run is reported");

    assert_eq!(
        report.result,
        Ok("false:canceled|after".to_owned()),
        "the stop raised canceled at the loop, and the message waited for the next read"
    );
    assert_eq!(
        chat.started(),
        1,
        "the message withdrew no round and asked for none"
    );
    assert_eq!(chat.dropped(), 1, "the stop tore the round down");
    assert_eq!(
        answers_to(&recorder.records(report.run_id), "Chat"),
        [json!("Dropped")],
        "one round, answered Dropped by the stop"
    );
}

#[tokio::test]
async fn a_send_once_a_rounds_answer_has_landed_keeps_that_answer_and_asks_again_with_the_message()
{
    let gate = Arc::new(Notify::new());
    let broker = gated_broker(&gate);
    let rounds = broker.rounds();
    let recorder = Arc::new(MemoryRecorder::new());
    let harness = plain_harness(&recorder, broker, LOOPS);

    let report = run_beside(harness, |control| async move {
        until("the model round is in flight", || {
            rounds.lock().unwrap().len() == 1
        })
        .await;
        // The reply lands and the message arrives with no poll of the run
        // between them, so the loop wakes for the message with the reply
        // already landed.
        gate.notify_one();
        assert!(control.send("more".to_owned()));
    })
    .await
    .expect("the run is reported");

    let chats = answers_to(&recorder.records(report.run_id), "Chat");
    assert_eq!(chats.len(), 2, "the landed round, then one more: {chats:?}");
    assert_eq!(
        chats[0]["Chat"]["Ok"]["reply"], "first",
        "the round whose answer landed was answered, not withdrawn: {chats:?}"
    );
    assert_eq!(
        report.result,
        Ok("re: This message arrived while you were working:\n\nmore".to_owned()),
        "the loop asked again with the message after the first reply"
    );
    assert!(report.unread.is_empty(), "{:?}", report.unread);
}

#[tokio::test]
async fn a_run_that_ends_with_messages_it_never_read_reports_them_unread_oldest_first() {
    let recorder = Arc::new(MemoryRecorder::new());
    let (vfs, slot) = sending_store("three");
    let harness = launch(
        no_packs(),
        PendingTimer::default(),
        RECEIVES_THEN_WRITES,
        RunSetup {
            vfs,
            ..setup(recorder.clone(), ScriptedBroker::replying())
        },
    );
    let control = harness.control();
    slot.set(control.clone()).unwrap();
    for text in ["one", "two"] {
        assert!(control.send(text.to_owned()));
    }

    // The two messages are delivered before the first step, which reads
    // the first. The write runs inline and sends the third, and the run
    // ends before the loop waits again, so it never delivers the third.
    let report = harness.run().await.expect("the run is reported");

    assert_eq!(report.result, Ok("one".to_owned()));
    assert_eq!(
        report.unread,
        ["two", "three"],
        "the message left in the inbox, then the one the loop never delivered"
    );
    assert_eq!(
        messages(&recorder.records(report.run_id)),
        ["one", "two"],
        "only the delivered messages are recorded"
    );
}
