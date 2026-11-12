//! The main chain's inbox at the run's boundary: `Run::deliver` queues a
//! message and returns the events to record before the next step, the
//! `InboxMessage` event that reports the message last, the next step
//! applies it, and `tasks.receive()` takes the oldest message or waits
//! for one, in the main chain, the H1 pass, and their `call` children.
//! `waiting_for_input` reports that wait, and `Step::Done` hands back every
//! message the run never read.

use runvil_prompt::test_support::{PromptSource, prompt};
use runvil_types::ids::{ChainId, TaskId};

use super::*;
use crate::lua::VfsOutcome;

/// The main chain's task, `0`.
pub(super) fn main_task() -> TaskId {
    TaskId::from(ChainId::root())
}

/// A run of the Pack-free prompt `source`.
fn run_of_source(source: &str) -> Run {
    let parsed = Prompt::parse(&|_: &str| Ok(source.to_owned()), "prompt")
        .expect("the inbox test prompt parses");
    Run::new(Arc::new(parsed), "", run_params())
}

/// A run of the Pack-free prompt whose sections are `sections`, the first
/// one walked first.
pub(super) fn run_of_sections(sections: &[(&str, &str)]) -> Run {
    run_of_source(&prompt("", sections))
}

/// Steps `run` and returns the pending step's effects, events, and
/// `waiting_for_input`.
#[track_caller]
pub(super) fn pending(run: &mut Run) -> (Vec<IssuedEffect>, Vec<Event>, bool) {
    match run.step() {
        Step::Pending {
            effects,
            events,
            waiting_for_input,
            ..
        } => (effects, events, waiting_for_input),
        other @ Step::Done { .. } => panic!("the run goes on: {other:?}"),
    }
}

/// Steps `run` and returns the finished run's result and unread messages.
#[track_caller]
pub(super) fn done(run: &mut Run) -> (Result<String, Error>, Vec<String>) {
    match run.step() {
        Step::Done { result, unread, .. } => (result, unread),
        other @ Step::Pending { .. } => panic!("the run is over: {other:?}"),
    }
}

/// Answers the one store effect in `effects` with `outcome`.
#[track_caller]
pub(super) fn answer_store(run: &mut Run, effects: &[IssuedEffect], outcome: VfsOutcome) {
    let id = only_effect_id(effects);
    assert!(
        matches!(effects[0].effect, Effect::Vfs { .. }),
        "the one effect is a store call: {effects:?}"
    );
    run.resume(id, EffectAnswer::Vfs(Ok(outcome)));
}

/// The one event `delivered` holds: a delivery with no answer resumed
/// since the last step returns its message's event alone.
#[track_caller]
fn only_event(delivered: &[Event]) -> &Event {
    let [event] = delivered else {
        panic!("deliver returns the message's event alone: {delivered:?}");
    };
    event
}

/// The sequence numbers of the events in `events` stamped under task `0`.
fn main_seqs(events: &[Event]) -> Vec<u32> {
    events
        .iter()
        .filter(|event| event.provenance().task == main_task())
        .map(|event| event.provenance().seq)
        .collect()
}

#[test]
fn a_delivered_message_resumes_a_waiting_receive_with_its_text() {
    let mut run = run_of("return 'got ' .. tasks.receive()", run_params());
    let (effects, _, waiting) = pending(&mut run);
    assert!(
        effects.is_empty(),
        "a wait on the inbox issues nothing: {effects:?}"
    );
    assert!(waiting, "the main chain waits on its inbox");
    let _ = run.deliver("hello".to_owned());
    let (result, unread) = done(&mut run);
    assert_eq!(result.expect("the read message ends the run"), "got hello");
    assert!(unread.is_empty(), "the one message was read: {unread:?}");
}

#[test]
fn messages_delivered_before_the_call_return_at_once_oldest_first() {
    let mut run = run_of(
        "store.exists('busy')\n\
         local first = tasks.receive()\n\
         local second = tasks.receive()\n\
         return first .. '|' .. second",
        run_params(),
    );
    let (effects, _, waiting) = pending(&mut run);
    assert!(!waiting, "the main chain waits on its store call");
    let _ = run.deliver("one".to_owned());
    let _ = run.deliver("two".to_owned());
    answer_store(&mut run, &effects, VfsOutcome::Bool(false));
    let (result, unread) = done(&mut run);
    assert_eq!(
        result.expect("both reads return at once, with no wait between"),
        "one|two"
    );
    assert!(unread.is_empty(), "{unread:?}");
}

#[test]
fn messages_delivered_before_the_first_step_wait_in_the_inbox() {
    let mut run = run_of(
        "return tasks.receive() .. '|' .. tasks.receive()",
        run_params(),
    );
    let _ = run.deliver("early".to_owned());
    let _ = run.deliver("earlier still".to_owned());
    let (result, _) = done(&mut run);
    assert_eq!(
        result.expect("both reads return at once"),
        "early|earlier still"
    );
}

#[test]
fn deliver_returns_the_inbox_message_event_and_the_applying_step_reports_no_second_one() {
    let mut run = run_of(
        "local text = tasks.receive()\nlog('read ' .. text)\nreturn text",
        run_params(),
    );
    let (_, before, waiting) = pending(&mut run);
    assert!(waiting, "the main chain waits on its inbox");
    let delivered = run.deliver("hello".to_owned());
    let event = only_event(&delivered);
    let Event::InboxMessage {
        section,
        provenance,
        text,
    } = event
    else {
        panic!("deliver reports the message: {event:?}");
    };
    assert_eq!(text, "hello");
    assert_eq!(provenance.task, main_task(), "the main chain is task 0");
    assert_eq!(section, "Only", "the main chain waits in section `Only`");
    let Step::Done { result, events, .. } = run.step() else {
        panic!("the read message ends the run");
    };
    assert_eq!(result.expect("the run returns the message"), "hello");
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Lua { message, .. } if message == "read hello")),
        "the step that applies the message runs the reader: {events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, Event::InboxMessage { .. })),
        "the step that applies the message reports no second event: {events:?}"
    );
    assert!(
        main_seqs(&before).iter().all(|seq| *seq < provenance.seq),
        "the event follows every main-chain event before it: {before:?} {event:?}"
    );
    assert!(
        main_seqs(&events).iter().all(|seq| *seq > provenance.seq),
        "the event precedes every main-chain event of the step that applies it: {events:?} {event:?}"
    );
}

#[test]
fn deliver_before_the_run_starts_reports_under_the_prompts_title() {
    let mut run = run_of("return tasks.receive()", run_params());
    let delivered = run.deliver("early".to_owned());
    let event = only_event(&delivered);
    assert!(
        matches!(event, Event::InboxMessage { section, text, .. } if section == "Run" && text == "early"),
        "no section runs yet, so the event reports under the title: {event:?}"
    );
    assert_eq!(event.provenance().task, main_task());
    let (result, _) = done(&mut run);
    assert_eq!(result.expect("the queued message is read"), "early");
}

#[test]
fn waiting_for_input_is_true_exactly_while_the_main_chain_waits_on_its_inbox() {
    let mut run = run_of_sections(&[
        (
            "Main",
            "local t = tasks.spawn('../Worker')\n\
             store.exists('busy')\n\
             local text = tasks.receive()\n\
             tasks.cancel(t)\n\
             store.exists(text)\n\
             return text",
        ),
        ("Worker", "return tasks.receive()"),
    ]);
    let (effects, _, waiting) = pending(&mut run);
    assert!(
        !waiting,
        "a task waits on its own inbox, but the main chain waits on its store call"
    );
    answer_store(&mut run, &effects, VfsOutcome::Bool(false));
    let (effects, _, waiting) = pending(&mut run);
    assert!(effects.is_empty(), "{effects:?}");
    assert!(waiting, "the main chain waits on its inbox");
    let (effects, _, waiting) = pending(&mut run);
    assert!(
        effects.is_empty() && waiting,
        "a step with no message leaves the main chain waiting, and is no stall"
    );
    let _ = run.deliver("later".to_owned());
    let (effects, _, waiting) = pending(&mut run);
    assert!(
        !waiting,
        "the message resumed the main chain, which waits on its store call"
    );
    answer_store(&mut run, &effects, VfsOutcome::Bool(true));
    let (result, _) = done(&mut run);
    assert_eq!(result.expect("the run returns the message"), "later");
}

#[test]
fn done_lists_the_messages_left_in_the_main_chains_inbox_oldest_first() {
    let mut run = run_of("return tasks.receive()", run_params());
    let (_, _, waiting) = pending(&mut run);
    assert!(waiting);
    for text in ["one", "two", "three"] {
        let _ = run.deliver(text.to_owned());
    }
    let (result, unread) = done(&mut run);
    assert_eq!(result.expect("the first message ends the run"), "one");
    assert_eq!(unread, ["two", "three"]);
}

#[test]
fn a_failed_run_hands_back_its_unread_messages() {
    let mut run = run_of("store.exists('busy')\nerror('boom')", run_params());
    let (effects, _, _) = pending(&mut run);
    let _ = run.deliver("kept".to_owned());
    answer_store(&mut run, &effects, VfsOutcome::Bool(false));
    let (result, unread) = done(&mut run);
    assert!(result.is_err(), "the raise fails the run: {result:?}");
    assert_eq!(unread, ["kept"]);
}

#[test]
fn a_canceled_run_hands_back_the_message_its_woken_reader_never_resumed_with() {
    let cancel = CancelHandle::new();
    let mut run = run_of(
        "return tasks.receive()",
        run_params().with_cancel(cancel.clone()),
    );
    let (_, _, waiting) = pending(&mut run);
    assert!(waiting);
    let _ = run.deliver("one".to_owned());
    let _ = run.deliver("two".to_owned());
    cancel.cancel();
    let (result, unread) = done(&mut run);
    assert!(
        matches!(&result, Err(error) if error.is_canceled()),
        "{result:?}"
    );
    assert_eq!(
        unread,
        ["one", "two"],
        "the reader never resumed with its message, so it comes back first"
    );
}

#[test]
fn a_run_that_cannot_start_hands_back_the_messages_delivered_before_it() {
    let source = "---\nname: t\ndescription: d\nrunvil: 7\n---\n\n# Run\n\n## Only\n\ndone\n";
    let mut run = run_of_source(source);
    let _ = run.deliver("early".to_owned());
    let (result, unread) = done(&mut run);
    assert_eq!(
        result.expect_err("the version gate refuses the run").code,
        ErrorCode::UnsupportedVersion
    );
    assert_eq!(unread, ["early"]);
}

#[test]
fn a_receive_in_a_call_child_reads_the_main_chains_inbox() {
    let mut run = run_of_sections(&[
        ("Main", "return 'main got ' .. call('../Reader')"),
        ("Reader", "return tasks.receive()"),
    ]);
    let (effects, _, waiting) = pending(&mut run);
    assert!(effects.is_empty(), "{effects:?}");
    assert!(waiting, "the call child waits on the main chain's inbox");
    let delivered = run.deliver("hello".to_owned());
    let event = only_event(&delivered);
    assert_eq!(event.provenance().task, main_task());
    assert_eq!(
        event.section(),
        "Reader",
        "the main chain waits in its call child's section"
    );
    let (result, unread) = done(&mut run);
    assert_eq!(
        result.expect("the child returns the message"),
        "main got hello"
    );
    assert!(unread.is_empty(), "{unread:?}");
}

#[test]
fn the_h1_pass_and_the_walk_after_it_read_one_inbox() {
    let source = PromptSource::new()
        .h1("var.first = tasks.receive()")
        .section("Only", "return var.first .. '|' .. tasks.receive()")
        .build();
    let mut run = run_of_source(&source);
    let (_, _, waiting) = pending(&mut run);
    assert!(
        waiting,
        "the H1 pass is the main chain, waiting on its inbox"
    );
    let _ = run.deliver("one".to_owned());
    let _ = run.deliver("two".to_owned());
    let (result, unread) = done(&mut run);
    assert_eq!(
        result.expect("the walk reads the second message"),
        "one|two"
    );
    assert!(unread.is_empty(), "{unread:?}");
}

#[test]
fn receive_refuses_any_argument_and_leaves_the_inbox_alone() {
    let mut run = run_of(
        "local ok, err = pcall(tasks.receive, 'x')\n\
         local ok_nil, err_nil = pcall(tasks.receive, nil)\n\
         assert(not ok and err.kind == 'lua', tostring(err))\n\
         assert(not ok_nil and err_nil.kind == 'lua', tostring(err_nil))\n\
         return tostring(err) .. '|' .. tostring(err_nil) .. '|' .. tasks.receive()",
        run_params(),
    );
    let _ = run.deliver("kept".to_owned());
    let (result, unread) = done(&mut run);
    let text = result.expect("both refusals are caught");
    let parts: Vec<&str> = text.split('|').collect();
    assert_eq!(parts.len(), 3, "{text}");
    assert!(
        parts[0].contains("tasks.receive takes no arguments, got string"),
        "{text}"
    );
    assert!(
        parts[1].contains("tasks.receive takes no arguments, got nil"),
        "an explicit nil is an argument too: {text}"
    );
    assert_eq!(parts[2], "kept", "a refused call takes no message");
    assert!(unread.is_empty(), "{unread:?}");
}

#[test]
fn a_task_waiting_on_its_inbox_reads_blocked_inbox_and_gives_back_its_slot() {
    let mut run = run_of_sections(&[
        (
            "Main",
            "tasks.concurrency(1)\n\
             local waiter = tasks.spawn('../Waiter')\n\
             local worker = tasks.spawn('../Worker')\n\
             local _, ok, result = tasks.join_any({ worker })\n\
             local status = tasks.status(waiter)\n\
             tasks.cancel(waiter)\n\
             return tostring(ok) .. ' ' .. result .. ' ' .. status.state .. ' ' .. tostring(status.blocked)",
        ),
        ("Waiter", "return tasks.receive()"),
        ("Worker", "return 'worked'"),
    ]);
    let (result, _) = done(&mut run);
    assert_eq!(
        result.expect("the worker takes the slot the waiter gave back"),
        "true worked running inbox"
    );
}

#[test]
fn a_task_reads_its_own_inbox_and_leaves_the_main_chains_message_queued() {
    let mut run = run_of_sections(&[
        (
            "Main",
            "local waiter = tasks.spawn('../Waiter')\n\
             store.exists('busy')\n\
             local status = tasks.status(waiter)\n\
             local text = tasks.receive()\n\
             tasks.cancel(waiter)\n\
             return status.state .. ' ' .. tostring(status.blocked) .. ' ' .. text",
        ),
        ("Waiter", "return tasks.receive()"),
    ]);
    let _ = run.deliver("for main".to_owned());
    let (effects, _, waiting) = pending(&mut run);
    assert!(
        !waiting,
        "the main chain waits on its store call, and the waiter on its own inbox"
    );
    answer_store(&mut run, &effects, VfsOutcome::Bool(false));
    let (result, unread) = done(&mut run);
    assert_eq!(
        result.expect("the main chain reads the message the waiter left queued"),
        "running inbox for main"
    );
    assert!(unread.is_empty(), "{unread:?}");
}
