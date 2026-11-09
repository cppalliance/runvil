//! A task's inbox at the run's boundary: an owner's `tasks.send` queues a
//! message for a task it spawned, wakes the task's `tasks.receive()`, and
//! reports `InboxMessage` under the task's provenance, right after the
//! task's start for a task still waiting for admission, while a non-owner,
//! an ended task, and a text that is not a string are refused. A task's
//! inbox goes when the task stops being live. A chain that parks on its
//! empty inbox reports `InboxWaiting` and gives back its slot, and a run
//! whose only waits are on inboxes no one can fill ends as `stalled`.

use runvil_types::error::ErrorFact;
use runvil_types::ids::{ChainId, TaskId};
use serde_json::json;

use super::inbox::{answer_store, done, main_task, pending, run_of_sections};
use super::*;
use crate::lua::VfsOutcome;

/// The `index`th task the main chain spawned.
fn spawned(index: u32) -> TaskId {
    TaskId::from(ChainId::root().child(index))
}

/// The coordinates of every `InboxWaiting` event in `events`.
fn inbox_waits(events: &[Event]) -> Vec<(String, TaskId)> {
    events
        .iter()
        .filter(|event| matches!(event, Event::InboxWaiting { .. }))
        .map(|event| (event.section().to_owned(), event.provenance().task.clone()))
        .collect()
}

/// The receiving task and text of every `InboxMessage` event in `events`.
fn inbox_messages(events: &[Event]) -> Vec<(TaskId, String)> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::InboxMessage {
                provenance, text, ..
            } => Some((provenance.task.clone(), text.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn inbox_waiting_reports_the_main_chain_parking_on_an_empty_inbox_and_not_a_read_that_finds_one() {
    let mut run = run_of(
        "local first = tasks.receive()\nlocal second = tasks.receive()\nreturn first .. second",
        run_params(),
    );
    let (_, events, waiting) = pending(&mut run);
    assert!(waiting, "the main chain waits on its inbox");
    assert_eq!(
        inbox_waits(&events),
        [("Only".to_owned(), main_task())],
        "the first read finds the inbox empty"
    );
    let _ = run.deliver("a".to_owned());
    let _ = run.deliver("b".to_owned());
    let Step::Done { result, events, .. } = run.step() else {
        panic!("both messages are read, which ends the run");
    };
    assert_eq!(result.expect("the run returns both messages"), "ab");
    assert!(
        inbox_waits(&events).is_empty(),
        "the second read finds a message waiting, so nothing parks: {events:?}"
    );
}

#[test]
fn an_owners_send_returns_true_resumes_its_tasks_receive_and_reports_under_the_task() {
    let mut run = run_of_sections(&[
        (
            "Main",
            "local t = tasks.spawn('## Worker')\n\
             store.exists('busy')\n\
             local sent = tasks.send(t, 'also cover the tests')\n\
             local _, ok, result = tasks.join_any({ t })\n\
             return tostring(sent) .. ' ' .. tostring(ok) .. ' ' .. result",
        ),
        ("Worker", "return 'got ' .. tasks.receive()"),
    ]);
    let (effects, _, waiting) = pending(&mut run);
    assert!(!waiting, "the main chain waits on its store call");
    answer_store(&mut run, &effects, VfsOutcome::Bool(false));
    let Step::Done { result, events, .. } = run.step() else {
        panic!("the send wakes the worker, whose end ends the run");
    };
    assert_eq!(
        result.expect("the worker returns the message"),
        "true true got also cover the tests"
    );
    assert_eq!(
        inbox_messages(&events),
        [(spawned(0), "also cover the tests".to_owned())],
        "the step that applies the send reports it under the worker"
    );
    let reported = events
        .iter()
        .find(|event| matches!(event, Event::InboxMessage { .. }))
        .expect("the send is reported");
    assert_eq!(reported.section(), "Worker", "the worker reads in `Worker`");
}

#[test]
fn a_non_owner_sending_by_id_string_raises_task_not_owned() {
    let mut run = run_of_sections(&[
        (
            "Main",
            "local a = tasks.spawn('## A')\n\
             local b = tasks.spawn('## B', { input = a.task })\n\
             local _, _, from_b = tasks.join_any({ b })\n\
             local _, own = pcall(tasks.send, sys.taskid, 'x')\n\
             local _, unknown = pcall(tasks.send, '0.9', 'x')\n\
             tasks.cancel(a)\n\
             return from_b .. '|' .. own.kind .. '|' .. unknown.kind",
        ),
        ("A", "return tasks.receive()"),
        (
            "B",
            "local ok, err = pcall(tasks.send, args, 'x')\n\
             return tostring(ok) .. ' ' .. err.kind",
        ),
    ]);
    let (result, _) = done(&mut run);
    assert_eq!(
        result.expect("every refusal is caught"),
        "false task_not_owned|task_not_owned|task_not_owned",
        "B does not own its sibling, no chain owns itself, and no task is 0.9"
    );
}

#[test]
fn a_send_to_a_task_that_returned_returns_false_and_queues_nothing() {
    let mut run = run_of_sections(&[
        (
            "Main",
            "local t = tasks.spawn('## Worker')\n\
             tasks.join({ t })\n\
             return tostring(tasks.send(t, 'too late'))",
        ),
        ("Worker", "return 'done'"),
    ]);
    let Step::Done { result, events, .. } = run.step() else {
        panic!("the run ends in its first step");
    };
    assert_eq!(result.expect("the late send is no failure"), "false");
    assert!(
        inbox_messages(&events).is_empty(),
        "a dropped text enters no inbox: {events:?}"
    );
}

#[test]
fn a_send_to_a_task_waiting_for_admission_is_reported_right_after_its_start() {
    let mut run = run_of_sections(&[
        (
            "Main",
            "local t = tasks.spawn('## Worker')\n\
             tasks.send(t, 'one')\n\
             tasks.send(t, 'two')\n\
             local _, _, result = tasks.join_any({ t })\n\
             return result",
        ),
        ("Worker", "return tasks.receive() .. '|' .. tasks.receive()"),
    ]);
    let Step::Done { result, events, .. } = run.step() else {
        panic!("the worker reads both messages and returns, which ends the run");
    };
    assert_eq!(result.expect("the worker returns both messages"), "one|two");
    let started = events
        .iter()
        .position(|event| matches!(event, Event::TaskStarted { task, .. } if *task == spawned(0)))
        .expect("the worker starts");
    let reported: Vec<usize> = events
        .iter()
        .enumerate()
        .filter(|(_, event)| matches!(event, Event::InboxMessage { .. }))
        .map(|(at, _)| at)
        .collect();
    assert_eq!(
        reported,
        [started + 1, started + 2],
        "both messages are reported right after the worker's start: {events:?}"
    );
    assert_eq!(
        inbox_messages(&events),
        [
            (spawned(0), "one".to_owned()),
            (spawned(0), "two".to_owned())
        ],
        "oldest first, under the worker's provenance"
    );
}

#[test]
fn a_task_canceled_before_admission_reports_neither_its_start_nor_a_message_sent_to_it() {
    let mut run = run_of_sections(&[
        (
            "Main",
            "local t = tasks.spawn('## Worker')\n\
             local sent = tasks.send(t, 'never read')\n\
             tasks.cancel(t)\n\
             return tostring(sent) .. ' ' .. tasks.status(t).state",
        ),
        ("Worker", "return tasks.receive()"),
    ]);
    let Step::Done { result, events, .. } = run.step() else {
        panic!("the run ends in its first step");
    };
    assert_eq!(
        result.expect("the send and the cancel both succeed"),
        "true canceled"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, Event::TaskStarted { .. })),
        "the worker never started: {events:?}"
    );
    assert!(
        inbox_messages(&events).is_empty(),
        "a message to a task that never started is not reported: {events:?}"
    );
}

#[test]
fn a_task_that_stops_being_live_drops_the_messages_left_in_its_inbox() {
    let mut run = run_of_sections(&[
        (
            "Main",
            "local a = tasks.spawn('## Returns')\n\
             local b = tasks.spawn('## Canceled')\n\
             store.exists('main')\n\
             tasks.send(a, 'unread by a')\n\
             tasks.send(b, 'unread by b')\n\
             tasks.cancel(b)\n\
             tasks.join({ a })\n\
             return 'done'",
        ),
        ("Returns", "store.exists('returns')\nreturn 'returned'"),
        ("Canceled", "store.exists('canceled')\nreturn 'never'"),
    ]);
    let (effects, _, _) = pending(&mut run);
    let issued_by = |task: &TaskId| {
        effects
            .iter()
            .find(|issued| issued.provenance.task == *task)
            .unwrap_or_else(|| panic!("{task} waits on its store call: {effects:?}"))
            .id
    };
    let (main, returns, canceled) = (
        issued_by(&main_task()),
        issued_by(&spawned(0)),
        issued_by(&spawned(1)),
    );
    run.resume(main, EffectAnswer::Vfs(Ok(VfsOutcome::Bool(false))));
    let (effects, _, _) = pending(&mut run);
    assert!(effects.is_empty(), "{effects:?}");
    let scheduler = run.scheduler_for_test();
    assert_eq!(
        scheduler.queued_for_test(&spawned(0)),
        1,
        "the message waits while its task runs its store call"
    );
    assert_eq!(
        scheduler.queued_for_test(&spawned(1)),
        0,
        "the cancel dropped the canceled task's inbox"
    );
    run.resume(canceled, EffectAnswer::Dropped);
    run.resume(returns, EffectAnswer::Vfs(Ok(VfsOutcome::Bool(false))));
    let (result, unread) = done(&mut run);
    assert_eq!(result.expect("the returning task ends the wait"), "done");
    assert!(unread.is_empty(), "{unread:?}");
    assert_eq!(
        run.scheduler_for_test().queued_for_test(&spawned(0)),
        0,
        "the task's return dropped its inbox"
    );
}

#[test]
fn a_send_whose_text_is_not_a_string_raises_a_lua_error() {
    let mut run = run_of_sections(&[
        (
            "Main",
            "local t = tasks.spawn('## Worker')\n\
             local ok, err = pcall(tasks.send, t, 42)\n\
             tasks.cancel(t)\n\
             return tostring(ok) .. '|' .. err.kind .. '|' .. tostring(err)",
        ),
        ("Worker", "return tasks.receive()"),
    ]);
    let (result, _) = done(&mut run);
    let text = result.expect("the refusal is caught");
    let parts: Vec<&str> = text.split('|').collect();
    assert_eq!(parts[..2], ["false", "lua"], "{text}");
    assert!(
        parts[2].contains("tasks.send text must be a string, got integer"),
        "{text}"
    );
}

#[test]
fn a_task_waiting_on_its_inbox_reads_blocked_inbox_and_reports_its_wait_under_its_provenance() {
    let mut run = run_of_sections(&[
        (
            "Main",
            "local t = tasks.spawn('## Waiter')\n\
             store.exists('busy')\n\
             local blocked = tasks.status(t).blocked\n\
             tasks.send(t, 'go')\n\
             local _, _, result = tasks.join_any({ t })\n\
             return blocked .. ' ' .. result",
        ),
        ("Waiter", "return tasks.receive()"),
    ]);
    let (effects, events, waiting) = pending(&mut run);
    assert!(
        !waiting,
        "the waiter waits on its own inbox, not the main chain's"
    );
    assert_eq!(
        inbox_waits(&events),
        [("Waiter".to_owned(), spawned(0))],
        "the waiter's park is reported under its own provenance"
    );
    answer_store(&mut run, &effects, VfsOutcome::Bool(false));
    let (result, _) = done(&mut run);
    assert_eq!(
        result.expect("the send wakes the waiter"),
        "inbox go",
        "the waiter read as blocked on its inbox until the send woke it"
    );
}

#[test]
fn a_task_waiting_on_its_inbox_frees_its_slot_so_a_queued_spawn_starts() {
    let mut run = run_of_sections(&[
        (
            "Main",
            "tasks.concurrency(1)\n\
             local waiter = tasks.spawn('## Waiter')\n\
             local worker = tasks.spawn('## Worker')\n\
             local _, _, worked = tasks.join_any({ worker })\n\
             tasks.send(waiter, 'woken')\n\
             local _, _, woken = tasks.join_any({ waiter })\n\
             return worked .. ' ' .. woken",
        ),
        ("Waiter", "return tasks.receive()"),
        ("Worker", "return 'worked'"),
    ]);
    let (result, _) = done(&mut run);
    assert_eq!(
        result.expect("the worker runs in the waiter's slot, then the waiter takes it back"),
        "worked woken"
    );
}

#[test]
fn a_main_chain_joining_a_task_that_waits_on_its_inbox_with_nothing_else_to_run_ends_stalled() {
    let mut run = run_of_sections(&[
        (
            "Main",
            "local t = tasks.spawn('## Worker')\n\
             tasks.join({ t })\n\
             return 'unreachable'",
        ),
        ("Worker", "return tasks.receive()"),
    ]);
    let (result, unread) = done(&mut run);
    let error = result.expect_err("nothing can move, and only the worker's owner could send");
    assert_eq!(error.code, ErrorCode::Stalled, "{error:?}");
    assert!(
        error.message.contains("0.0"),
        "the failure names the waiting task: {}",
        error.message
    );
    assert_eq!(error.facts.get(&ErrorFact::Tasks), Some(&json!(["0.0"])));
    assert!(unread.is_empty(), "{unread:?}");
}
