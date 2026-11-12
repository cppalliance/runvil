//! A task's replies at the run's boundary: `tasks.reply` hands the task's
//! owner its latest result and leaves the task idle on its inbox, where
//! the owner's next `tasks.send` runs it again. A wait takes each reply
//! once; an idle task reads as running, blocked on its inbox, gives back
//! its slot, is listed as pending, ends canceled when canceled, and is no
//! leak when its owner ends. The main chain has no owner to reply to, and
//! a `call` child replies for the task it runs inside. Past the run's idle
//! cap, a reply closes the task idle longest that owns no live task,
//! counting the tasks its `call` children spawned, or else the one idle
//! longest, as if it had returned its latest reply, so the closed task's
//! live tasks are abandoned as `owner_returned`.

use std::num::NonZeroUsize;

use runvil_prompt::test_support::prompt;
use runvil_types::error::ErrorFact;
use runvil_types::ids::{AbandonReason, ChainId, TaskId};
use serde_json::json;

use super::inbox::{answer_store, done, main_task, pending, run_of_sections};
use super::*;
use crate::execute::RunLimits;
use crate::lua::VfsOutcome;

/// The `index`th task the main chain spawned.
fn spawned(index: u32) -> TaskId {
    TaskId::from(ChainId::root().child(index))
}

/// A run of the Pack-free prompt whose sections are `sections`, the first
/// one walked first, holding at most `cap` idle tasks.
fn run_with_idle_cap(sections: &[(&str, &str)], cap: usize) -> Run {
    let source = prompt("", sections);
    let parsed = Prompt::parse(&|_: &str| Ok(source.clone()), "prompt")
        .expect("the reply test prompt parses");
    let limits = RunLimits {
        max_idle_tasks: NonZeroUsize::new(cap).expect("the cap is non-zero"),
        ..RunLimits::default()
    };
    Run::new(Arc::new(parsed), "", run_params().with_limits(limits))
}

/// Steps `run` to its end and returns its result and the last step's
/// events.
#[track_caller]
fn finish(run: &mut Run) -> (Result<String, Error>, Vec<Event>) {
    match run.step() {
        Step::Done { result, events, .. } => (result, events),
        other @ Step::Pending { .. } => panic!("the run is over: {other:?}"),
    }
}

/// The section and task of every `InboxWaiting` event in `events`.
fn inbox_waits(events: &[Event]) -> Vec<(String, TaskId)> {
    events
        .iter()
        .filter(|event| matches!(event, Event::InboxWaiting { .. }))
        .map(|event| (event.section().to_owned(), event.provenance().task.clone()))
        .collect()
}

#[test]
fn a_wait_on_a_task_that_replied_returns_the_reply_and_the_task_parks_on_its_inbox() {
    let mut run = run_of_sections(&[
        (
            "Main",
            "local t = tasks.spawn('../Worker')\n\
             local _, ok, reply = tasks.join_any({ t })\n\
             tasks.cancel(t)\n\
             local u = tasks.spawn('../Worker')\n\
             local joined = tasks.join({ u })[1]\n\
             tasks.cancel(u)\n\
             return tostring(ok) .. ' ' .. reply .. ' ' .. tostring(joined.ok) .. ' ' .. joined.result",
        ),
        ("Worker", "tasks.reply('summary')\nreturn 'unreachable'"),
    ]);
    let (result, events) = finish(&mut run);
    assert_eq!(
        result.expect("join_any and join both take the reply"),
        "true summary true summary"
    );
    assert_eq!(
        inbox_waits(&events),
        [
            ("Worker".to_owned(), spawned(0)),
            ("Worker".to_owned(), spawned(1)),
        ],
        "each reply parks its worker on its empty inbox, under the worker's provenance"
    );
}

#[test]
fn after_a_send_a_second_wait_returns_the_tasks_next_reply() {
    let mut run = run_of_sections(&[
        (
            "Main",
            "local t = tasks.spawn('../Worker')\n\
             local _, _, first = tasks.join_any({ t })\n\
             tasks.send(t, 'shorten it')\n\
             local _, _, second = tasks.join_any({ t })\n\
             tasks.send(t, 'done')\n\
             local _, _, last = tasks.join_any({ t })\n\
             return first .. '|' .. second .. '|' .. last",
        ),
        (
            "Worker",
            "local text = tasks.reply('long draft')\n\
             text = tasks.reply('short draft after ' .. text)\n\
             return 'returned after ' .. text",
        ),
    ]);
    let (result, _) = done(&mut run);
    assert_eq!(
        result.expect("each wait takes the task's next result"),
        "long draft|short draft after shorten it|returned after done"
    );
}

#[test]
fn a_wait_on_an_idle_task_whose_reply_was_taken_with_no_message_since_raises_task_consumed() {
    let mut run = run_of_sections(&[
        (
            "Main",
            "local t = tasks.spawn('../Worker')\n\
             tasks.join_any({ t })\n\
             local state = tasks.status(t).state\n\
             local ok, err = pcall(tasks.join_any, { t })\n\
             tasks.cancel(t)\n\
             return state .. ' ' .. tostring(ok) .. ' ' .. err.kind",
        ),
        ("Worker", "tasks.reply('once')\nreturn 'unreachable'"),
    ]);
    let (result, _) = done(&mut run);
    assert_eq!(
        result.expect("the refusal is caught"),
        "running false task_consumed",
        "the task is still open, yet its one reply is taken"
    );
}

#[test]
fn a_reply_that_finds_a_message_queued_returns_it_at_once_and_the_next_wait_waits_for_the_task() {
    let mut run = run_of_sections(&[
        (
            "Main",
            "local t = tasks.spawn('../Worker')\n\
             tasks.send(t, 'early')\n\
             local _, _, first = tasks.join_any({ t })\n\
             local _, _, second = tasks.join_any({ t })\n\
             return first .. '|' .. second",
        ),
        ("Worker", "return 'got ' .. tasks.reply('first')"),
    ]);
    let (result, events) = finish(&mut run);
    assert_eq!(
        result.expect("the worker runs on past its reply, so the second wait waits for its end"),
        "first|got early"
    );
    assert!(
        inbox_waits(&events).is_empty(),
        "a reply that finds a message never parks: {events:?}"
    );
}

#[test]
fn an_idle_tasks_status_reads_running_blocked_on_its_inbox_with_no_ok_in_its_section() {
    let mut run = run_of_sections(&[
        (
            "Main",
            "local t = tasks.spawn('../Worker')\n\
             tasks.join_any({ t })\n\
             local status = tasks.status(t)\n\
             tasks.cancel(t)\n\
             return status.state .. ' ' .. tostring(status.blocked) .. ' ' .. tostring(status.ok) \
             .. ' ' .. tostring(status.section)",
        ),
        ("Worker", "tasks.reply('ready')\nreturn 'unreachable'"),
    ]);
    let (result, _) = done(&mut run);
    assert_eq!(
        result.expect("the idle task's status is read"),
        "running inbox nil Worker"
    );
}

#[test]
fn an_idle_task_frees_its_slot_so_a_queued_spawn_starts() {
    let mut run = run_of_sections(&[
        (
            "Main",
            "tasks.concurrency(1)\n\
             local replier = tasks.spawn('../Replier')\n\
             local worker = tasks.spawn('../Worker')\n\
             local _, _, ready = tasks.join_any({ replier })\n\
             local _, _, worked = tasks.join_any({ worker })\n\
             tasks.send(replier, 'more')\n\
             local _, _, last = tasks.join_any({ replier })\n\
             return ready .. ' ' .. worked .. ' ' .. last",
        ),
        ("Replier", "return 'replier got ' .. tasks.reply('ready')"),
        ("Worker", "return 'worked'"),
    ]);
    let (result, _) = done(&mut run);
    assert_eq!(
        result.expect("the worker runs in the idle replier's slot, then the replier takes it back"),
        "ready worked replier got more"
    );
}

#[test]
fn an_owner_that_ends_with_an_idle_task_keeps_its_own_result_and_abandons_the_task() {
    let mut run = run_of_sections(&[
        (
            "Main",
            "local owner = tasks.spawn('../Owner')\n\
             local _, ok, result = tasks.join_any({ owner })\n\
             local t = tasks.spawn('../Worker')\n\
             tasks.join_any({ t })\n\
             return tostring(ok) .. ' ' .. result",
        ),
        (
            "Owner",
            "local t = tasks.spawn('../Worker')\n\
             tasks.join_any({ t })\n\
             return 'owner result'",
        ),
        ("Worker", "tasks.reply('ready')\nreturn 'unreachable'"),
    ]);
    let (result, events) = finish(&mut run);
    assert_eq!(
        result.expect("an idle task is no leak, for a task owner or the main walk"),
        "true owner result"
    );
    let abandoned: Vec<(TaskId, AbandonReason)> = events
        .iter()
        .filter_map(|event| match event {
            Event::TaskAbandoned { task, reason, .. } => Some((task.clone(), *reason)),
            _ => None,
        })
        .collect();
    assert_eq!(
        abandoned,
        [
            (
                TaskId::from(ChainId::root().child(0).child(0)),
                AbandonReason::OwnerReturned
            ),
            (spawned(1), AbandonReason::OwnerReturned),
        ],
        "each idle task ends with its owner"
    );
}

#[test]
fn an_owner_that_ends_with_a_running_task_beside_an_idle_one_names_only_the_running_one() {
    let mut run = run_of_sections(&[
        (
            "Main",
            "local idle = tasks.spawn('../Replier')\n\
             local running = tasks.spawn('../Receiver')\n\
             tasks.join_any({ idle })\n\
             return 'leaked'",
        ),
        ("Replier", "tasks.reply('ready')\nreturn 'unreachable'"),
        ("Receiver", "return tasks.receive()"),
    ]);
    let (result, events) = finish(&mut run);
    let error = result.expect_err("a running task left live is still a leak");
    assert_eq!(error.code, ErrorCode::TasksLive, "{error:?}");
    assert_eq!(
        error.facts.get(&ErrorFact::Tasks),
        Some(&json!(["0.1"])),
        "only the receiver, which never replied, counts as leaked"
    );
    let abandoned: Vec<&TaskId> = events
        .iter()
        .filter_map(|event| match event {
            Event::TaskAbandoned { task, .. } => Some(task),
            _ => None,
        })
        .collect();
    assert_eq!(
        abandoned,
        [&spawned(0), &spawned(1)],
        "the idle replier and the receiver both end with their owner"
    );
}

#[test]
fn canceling_an_idle_task_ends_it_canceled() {
    let mut run = run_of_sections(&[
        (
            "Main",
            "local t = tasks.spawn('../Worker')\n\
             tasks.join_any({ t })\n\
             tasks.cancel(t)\n\
             local status = tasks.status(t)\n\
             local _, ok, err = tasks.join_any({ t })\n\
             local sent = tasks.send(t, 'late')\n\
             return status.state .. ' ' .. tostring(status.ok) .. ' ' .. tostring(ok) .. ' ' \
             .. err.kind .. ' ' .. tostring(sent)",
        ),
        ("Worker", "tasks.reply('ready')\nreturn 'unreachable'"),
    ]);
    let (result, events) = finish(&mut run);
    assert_eq!(
        result.expect("the canceled task is read"),
        "canceled false false canceled false"
    );
    let canceled: Vec<&TaskId> = events
        .iter()
        .filter_map(|event| match event {
            Event::TaskCanceled { task, .. } => Some(task),
            _ => None,
        })
        .collect();
    assert_eq!(canceled, [&spawned(0)]);
}

#[test]
fn a_reply_on_the_main_chain_or_its_call_child_raises_a_lua_error() {
    let mut run = run_of_sections(&[
        (
            "Main",
            "local ok, err = pcall(tasks.reply, 'x')\n\
             return tostring(ok) .. ' ' .. err.kind .. '|' .. call('../Helper') .. '|' .. tostring(err)",
        ),
        (
            "Helper",
            "local ok, err = pcall(tasks.reply, 'x')\n\
             return tostring(ok) .. ' ' .. err.kind",
        ),
    ]);
    let (result, unread) = done(&mut run);
    let text = result.expect("both refusals are caught");
    let parts: Vec<&str> = text.split('|').collect();
    assert_eq!(parts[..2], ["false lua", "false lua"], "{text}");
    assert!(parts[2].contains("main chain"), "{text}");
    assert!(unread.is_empty(), "{unread:?}");
}

#[test]
fn a_reply_in_a_call_child_of_a_task_replies_for_that_task() {
    let mut run = run_of_sections(&[
        (
            "Main",
            "local t = tasks.spawn('../Worker')\n\
             local _, _, reply = tasks.join_any({ t })\n\
             local status = tasks.status(t)\n\
             tasks.send(t, 'next')\n\
             local _, _, result = tasks.join_any({ t })\n\
             return reply .. '|' .. status.state .. ' ' .. tostring(status.blocked) .. ' ' \
             .. tostring(status.ok) .. ' ' .. tostring(status.section) .. '|' .. result",
        ),
        ("Worker", "return 'worker returned ' .. call('../Helper')"),
        (
            "Helper",
            "return 'helper got ' .. tasks.reply('from the helper')",
        ),
    ]);
    let (result, events) = finish(&mut run);
    assert_eq!(
        result.expect("the helper's reply is the worker's"),
        "from the helper|running inbox nil Helper|worker returned helper got next",
        "the idle worker reads as parked on its inbox in the helper's section"
    );
    assert_eq!(
        inbox_waits(&events),
        [("Helper".to_owned(), spawned(0))],
        "the helper parks on the worker's inbox, under the worker's provenance"
    );
}

#[test]
fn past_the_idle_cap_a_reply_closes_the_task_idle_longest_as_done_with_its_reply() {
    let mut run = run_with_idle_cap(
        &[
            (
                "Main",
                "local first = tasks.spawn('../First')\n\
                 local _, _, one = tasks.join_any({ first })\n\
                 local second = tasks.spawn('../Second')\n\
                 local _, _, two = tasks.join_any({ second })\n\
                 local status = tasks.status(first)\n\
                 local sent = tasks.send(first, 'too late')\n\
                 local open = tasks.status(second).state\n\
                 tasks.cancel(second)\n\
                 return one .. ' ' .. two .. ' ' .. status.state .. ' ' .. tostring(status.ok) \
                 .. ' ' .. tostring(sent) .. ' ' .. open",
            ),
            ("First", "tasks.reply('one')\nreturn 'unreachable'"),
            ("Second", "tasks.reply('two')\nreturn 'unreachable'"),
        ],
        1,
    );
    let (result, events) = finish(&mut run);
    assert_eq!(
        result.expect("the cap closes the first task, and the second stays open"),
        "one two done true false running"
    );
    let succeeded: Vec<(&str, &TaskId, &TaskId)> = events
        .iter()
        .filter_map(|event| match event {
            Event::TaskSucceeded {
                section,
                provenance,
                task,
            } => Some((section.as_str(), &provenance.task, task)),
            _ => None,
        })
        .collect();
    assert_eq!(
        succeeded,
        [("First", &spawned(0), &spawned(0))],
        "the close reports the first task as succeeded, under its own provenance"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, Event::TaskAbandoned { .. })),
        "the cap abandons nothing: {events:?}"
    );
}

#[test]
fn a_wait_on_a_task_the_cap_closed_delivers_its_undelivered_reply_once() {
    let mut run = run_with_idle_cap(
        &[
            (
                "Main",
                "local first = tasks.spawn('../First')\n\
                 local second = tasks.spawn('../Second')\n\
                 local _, _, two = tasks.join_any({ second })\n\
                 local closed = tasks.status(first).state\n\
                 local ready = tasks.ready(first)\n\
                 local _, ok, one = tasks.join_any({ first })\n\
                 local again, err = pcall(tasks.join_any, { first })\n\
                 tasks.cancel(second)\n\
                 return two .. ' ' .. closed .. ' ' .. tostring(ready) .. ' ' .. tostring(ok) \
                 .. ' ' .. one .. ' ' .. tostring(again) .. ' ' .. err.kind",
            ),
            ("First", "tasks.reply('one')\nreturn 'unreachable'"),
            ("Second", "tasks.reply('two')\nreturn 'unreachable'"),
        ],
        1,
    );
    let (result, _) = done(&mut run);
    assert_eq!(
        result.expect("the closed task's reply is taken once"),
        "two done true true one false task_consumed",
        "the second reply closes the first task before any wait on it"
    );
}

/// The id of the first task that task `0.index` spawned.
fn spawned_by(index: u32) -> TaskId {
    TaskId::from(ChainId::root().child(index).child(0))
}

/// The task of every `TaskSucceeded` and `TaskAbandoned` event in
/// `events`, in order, each abandonment with its reason.
fn successes_and_abandonments(events: &[Event]) -> Vec<(TaskId, Option<AbandonReason>)> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::TaskSucceeded { task, .. } => Some((task.clone(), None)),
            Event::TaskAbandoned { task, reason, .. } => Some((task.clone(), Some(*reason))),
            _ => None,
        })
        .collect()
}

#[test]
fn past_the_idle_cap_a_reply_passes_over_the_task_idle_longest_when_it_owns_a_live_task() {
    let mut run = run_with_idle_cap(
        &[
            (
                "Main",
                "local owner = tasks.spawn('../Owner')\n\
                 local _, _, one = tasks.join_any({ owner })\n\
                 local idle = tasks.spawn('../Idle')\n\
                 local _, _, two = tasks.join_any({ idle })\n\
                 local third = tasks.spawn('../Third')\n\
                 local _, _, three = tasks.join_any({ third })\n\
                 local states = tasks.status(owner).state .. ' ' .. tasks.status(idle).state \
                 .. ' ' .. tasks.status(third).state\n\
                 tasks.cancel(owner)\n\
                 tasks.cancel(third)\n\
                 return one .. two .. three .. ' ' .. states",
            ),
            (
                "Owner",
                "tasks.spawn('../Worker')\ntasks.reply('a')\nreturn 'unreachable'",
            ),
            ("Worker", "return tasks.receive()"),
            ("Idle", "tasks.reply('b')\nreturn 'unreachable'"),
            ("Third", "tasks.reply('c')\nreturn 'unreachable'"),
        ],
        2,
    );
    let (result, events) = finish(&mut run);
    assert_eq!(
        result.expect("the third reply closes the idle task that owns nothing"),
        "abc running done running"
    );
    let succeeded: Vec<&TaskId> = events
        .iter()
        .filter_map(|event| match event {
            Event::TaskSucceeded { task, .. } => Some(task),
            _ => None,
        })
        .collect();
    assert_eq!(
        succeeded,
        [&spawned(1)],
        "the owner idle longest stays open while its worker runs: {events:?}"
    );
}

#[test]
fn past_the_idle_cap_a_reply_passes_over_a_task_whose_call_child_owns_a_live_task() {
    let mut run = run_with_idle_cap(
        &[
            (
                "Main",
                "local owner = tasks.spawn('../Owner')\n\
                 local _, _, one = tasks.join_any({ owner })\n\
                 local idle = tasks.spawn('../Idle')\n\
                 local _, _, two = tasks.join_any({ idle })\n\
                 local third = tasks.spawn('../Third')\n\
                 local _, _, three = tasks.join_any({ third })\n\
                 local states = tasks.status(owner).state .. ' ' .. tasks.status(idle).state \
                 .. ' ' .. tasks.status(third).state\n\
                 tasks.cancel(owner)\n\
                 tasks.cancel(third)\n\
                 return one .. two .. three .. ' ' .. states",
            ),
            ("Owner", "return call('../Helper')"),
            (
                "Helper",
                "tasks.spawn('../Worker')\ntasks.reply('a')\nreturn 'unreachable'",
            ),
            ("Worker", "return tasks.receive()"),
            ("Idle", "tasks.reply('b')\nreturn 'unreachable'"),
            ("Third", "tasks.reply('c')\nreturn 'unreachable'"),
        ],
        2,
    );
    let (result, events) = finish(&mut run);
    assert_eq!(
        result.expect("the third reply closes the idle task that owns nothing"),
        "abc running done running"
    );
    let succeeded: Vec<&TaskId> = events
        .iter()
        .filter_map(|event| match event {
            Event::TaskSucceeded { task, .. } => Some(task),
            _ => None,
        })
        .collect();
    assert_eq!(
        succeeded,
        [&spawned(1)],
        "the owner's helper spawned the live worker, so the owner stays open: {events:?}"
    );
}

#[test]
fn past_the_idle_cap_closing_the_owner_of_the_replier_abandons_the_replier_as_owner_returned() {
    let mut run = run_with_idle_cap(
        &[
            (
                "Main",
                "local a = tasks.spawn('../A')\n\
                 local _, _, reply = tasks.join_any({ a })\n\
                 store.exists('busy')\n\
                 local status = tasks.status(a)\n\
                 return reply .. ' ' .. status.state .. ' ' .. tostring(status.ok)",
            ),
            (
                "A",
                "tasks.spawn('../B')\ntasks.reply('a')\nreturn 'unreachable'",
            ),
            ("B", "tasks.reply('b')\nreturn 'unreachable'"),
        ],
        1,
    );
    let (effects, events, _) = pending(&mut run);
    answer_store(&mut run, &effects, VfsOutcome::Bool(false));
    let (result, _) = finish(&mut run);
    assert_eq!(
        result.expect("the closed task keeps its reply as its result"),
        "a done true"
    );
    assert_eq!(
        successes_and_abandonments(&events),
        [
            (spawned_by(0), Some(AbandonReason::OwnerReturned)),
            (spawned(0), None),
        ],
        "B's reply closes A, the only other idle task, and B ends first, as A's return \
         would end it: {events:?}"
    );
}

#[test]
fn past_the_idle_cap_closing_a_running_workers_owner_withdraws_its_effect_and_leaks_nothing() {
    let mut run = run_with_idle_cap(
        &[
            (
                "Main",
                "local owner = tasks.spawn('../Owner')\n\
                 local _, _, one = tasks.join_any({ owner })\n\
                 store.exists('main')\n\
                 local replier = tasks.spawn('../Replier')\n\
                 local _, _, two = tasks.join_any({ replier })\n\
                 local status = tasks.status(owner)\n\
                 tasks.cancel(replier)\n\
                 return one .. ' ' .. two .. ' ' .. status.state .. ' ' .. tostring(status.ok)",
            ),
            (
                "Owner",
                "tasks.spawn('../Worker')\ntasks.reply('a')\nreturn 'unreachable'",
            ),
            ("Worker", "store.exists('worker')\nreturn 'never'"),
            ("Replier", "tasks.reply('b')\nreturn 'unreachable'"),
        ],
        1,
    );
    let (effects, _, _) = pending(&mut run);
    let issued_by = |task: &TaskId| {
        effects
            .iter()
            .find(|issued| issued.provenance.task == *task)
            .unwrap_or_else(|| panic!("{task} waits on its store call: {effects:?}"))
            .id
    };
    let (main, worker) = (issued_by(&main_task()), issued_by(&spawned_by(0)));
    run.resume(main, EffectAnswer::Vfs(Ok(VfsOutcome::Bool(false))));
    let Step::Pending {
        effects,
        events,
        withdrawn,
        ..
    } = run.step()
    else {
        panic!("the worker's withdrawn store call still owes its answer");
    };
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(withdrawn, [worker], "the worker's store call is withdrawn");
    assert_eq!(
        successes_and_abandonments(&events),
        [
            (spawned_by(0), Some(AbandonReason::OwnerReturned)),
            (spawned(0), None),
        ],
        "the closed owner's worker ends as the owner's return would end it: {events:?}"
    );
    run.resume(worker, EffectAnswer::Dropped);
    let (result, _) = finish(&mut run);
    assert_eq!(
        result.expect("the closed owner keeps its reply, so no tasks_live arises"),
        "a b done true"
    );
}

#[test]
fn past_the_idle_cap_closing_a_task_abandons_its_call_childs_worker_as_owner_returned() {
    let mut run = run_with_idle_cap(
        &[
            (
                "Main",
                "local owner = tasks.spawn('../Owner')\n\
                 local _, _, one = tasks.join_any({ owner })\n\
                 local replier = tasks.spawn('../Replier')\n\
                 local _, _, two = tasks.join_any({ replier })\n\
                 local status = tasks.status(owner)\n\
                 tasks.cancel(replier)\n\
                 return one .. ' ' .. two .. ' ' .. status.state .. ' ' .. tostring(status.ok)",
            ),
            ("Owner", "return call('../Helper')"),
            (
                "Helper",
                "tasks.spawn('../Worker')\ntasks.reply('a')\nreturn 'unreachable'",
            ),
            ("Worker", "return tasks.receive()"),
            ("Replier", "tasks.reply('b')\nreturn 'unreachable'"),
        ],
        1,
    );
    let (result, events) = finish(&mut run);
    assert_eq!(
        result.expect("the closed owner keeps its reply, so no tasks_live arises"),
        "a b done true"
    );
    let helpers_worker = TaskId::from(ChainId::root().child(0).child(0).child(0));
    assert_eq!(
        successes_and_abandonments(&events),
        [
            (helpers_worker, Some(AbandonReason::OwnerReturned)),
            (spawned(0), None),
        ],
        "the worker the owner's helper spawned ends as the owner's return would end it: \
         {events:?}"
    );
}

#[test]
fn ready_is_true_while_a_reply_waits_undelivered_and_false_once_a_wait_takes_it() {
    let mut run = run_of_sections(&[
        (
            "Main",
            "local t = tasks.spawn('../Worker')\n\
             local before = tasks.ready(t)\n\
             store.exists('busy')\n\
             local waiting = tasks.ready(t)\n\
             tasks.join_any({ t })\n\
             local taken = tasks.ready(t)\n\
             tasks.cancel(t)\n\
             return tostring(before) .. ' ' .. tostring(waiting) .. ' ' .. tostring(taken)",
        ),
        ("Worker", "tasks.reply('ready')\nreturn 'unreachable'"),
    ]);
    let (effects, _, waiting) = pending(&mut run);
    assert!(!waiting, "the main chain waits on its store call");
    answer_store(&mut run, &effects, VfsOutcome::Bool(false));
    let (result, _) = done(&mut run);
    assert_eq!(
        result.expect("ready is read three times"),
        "false true false",
        "not ready before the reply, ready while it waits, not once a wait took it"
    );
}

#[test]
fn pending_lists_an_idle_task() {
    let mut run = run_of_sections(&[
        (
            "Main",
            "local t = tasks.spawn('../Worker')\n\
             tasks.join_any({ t })\n\
             local listed = tasks.pending()\n\
             tasks.cancel(t)\n\
             return #listed .. ' ' .. listed[1].task .. ' ' .. #tasks.pending()",
        ),
        ("Worker", "tasks.reply('ready')\nreturn 'unreachable'"),
    ]);
    let (result, _) = done(&mut run);
    assert_eq!(result.expect("pending is read twice"), "1 0.0 0");
}

#[test]
fn a_reply_converts_its_value_as_a_section_return_does_with_nil_as_empty_text() {
    let mut run = run_of_sections(&[
        (
            "Main",
            "local t = tasks.spawn('../Worker')\n\
             local replies = {}\n\
             for _, word in ipairs({ 'a', 'b', 'c' }) do\n\
               local _, _, reply = tasks.join_any({ t })\n\
               replies[#replies + 1] = '[' .. reply .. ']'\n\
               tasks.send(t, word)\n\
             end\n\
             local _, _, last = tasks.join_any({ t })\n\
             return table.concat(replies, ' ') .. ' ' .. last",
        ),
        (
            "Worker",
            "tasks.reply()\n\
             tasks.reply(nil)\n\
             tasks.reply(42)\n\
             local ok, err = pcall(tasks.reply, { done = true })\n\
             return tostring(ok) .. ' ' .. err.kind .. ' ' .. tostring(err)",
        ),
    ]);
    let (result, _) = done(&mut run);
    let text = result.expect("every reply is taken, and the table's refusal is caught");
    assert!(text.starts_with("[] [] [42] false lua "), "{text}");
    assert!(
        text.contains("cannot return a table as a result"),
        "a table is refused as a section's return is: {text}"
    );
}
