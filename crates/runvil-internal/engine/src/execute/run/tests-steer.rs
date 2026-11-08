//! Messages steering a running `models.loop`, at the run's boundary. A
//! message queued before a request is appended before it, under its
//! chain's arrival line; one that arrives while a request has no answer
//! withdraws that request, which the loop sends again holding it, counted
//! as neither a round nor a turn; one that arrives while tool calls run,
//! or while the render before the first request has no answer, waits for
//! them and is appended before the next request; and one that lands as
//! the final request answers keeps the loop going. A stop still raises
//! `canceled`, and leaves the message queued for the next reader, and a
//! canceled run hands back the messages its loop took and never read.

use std::collections::BTreeMap;

use runvil_prompt::test_support::prompt;
use runvil_types::ids::{ChainId, TaskId};
use runvil_types::models::{ModelRole, ThinkingMode};
use runvil_types::tools::{ToolCatalog, ToolId, ToolInfo, ToolOutput};
use serde_json::json;

use super::inbox::done;
use super::*;
use crate::lua::VfsOutcome;
use crate::model::{Completion, CompletionResult, ToolCall};
use crate::test_support::scenario::{chat_answer, pass_through, test_model_with};
use crate::test_support::scripted_chat::text;

/// The arrival line of a message appended on the main chain.
const MAIN_LINE: &str = "This message arrived while you were working:";

/// The arrival line of a message appended on a task.
const TASK_LINE: &str = "The task that started you sent this message while you were working:";

/// Lua that returns the section's records as `role:content` lines joined
/// by `|`.
const RECORDS: &str = "local out = {}\n\
     for i = 1, #messages do out[i] = messages[i].role .. ':' .. messages[i].content end\n\
     return table.concat(out, '|')";

/// A message as the loop appends it under `line`.
fn arrived(line: &str, text: &str) -> String {
    format!("{line}\n\n{text}")
}

/// A run of the prompt whose frontmatter adds `front` and whose sections
/// are `sections`, on the `default` role filled by `test-model`, with
/// `tools/echo` in the catalog, under the cancel flag `cancel`.
fn steered_run(front: &str, sections: &[(&str, &str)], cancel: &CancelHandle) -> Run {
    let source = prompt(front, sections);
    let parsed = Prompt::parse(&|_: &str| Ok(source.clone()), "prompt")
        .0
        .expect("the steering test prompt parses");
    let model = test_model_with("test-model", 4096, ThinkingMode::Never);
    let echo = ToolInfo::new(
        ToolId::parse("tools/echo").expect("a valid tool id"),
        "Echoes its value.",
        json!({ "type": "object" }),
    );
    let catalog = ToolCatalog::new([echo]).expect("one tool is a valid catalog");
    let params = run_params()
        .with_tools(catalog)
        .with_models(BTreeMap::from([(ModelRole::Default, model)]))
        .with_cancel(cancel.clone());
    Run::new(Arc::new(parsed), "", params)
}

/// A run of one section, `Only`, whose Lua is `body` on the `default`
/// model.
fn loop_run(front: &str, body: &str) -> Run {
    steered_run(
        front,
        &[("Only", &format!("models.use('default')\n{body}"))],
        &CancelHandle::new(),
    )
}

/// One pending step past its renders: the effects it issued, renders
/// excluded, the effects it withdrew, and the events it reported.
struct Pending {
    effects: Vec<(EffectId, Effect)>,
    withdrawn: Vec<EffectId>,
    events: Vec<Event>,
}

/// Steps `run`, answering each render a step issues with its tools
/// unchanged and stepping again, and returns what those steps issued,
/// withdrew, and reported. Panics when the run is over.
#[track_caller]
fn step(run: &mut Run) -> Pending {
    let mut pending = Pending {
        effects: Vec::new(),
        withdrawn: Vec::new(),
        events: Vec::new(),
    };
    loop {
        let Step::Pending {
            effects,
            withdrawn,
            events,
            ..
        } = run.step()
        else {
            panic!("the run is still going");
        };
        pending.withdrawn.extend(withdrawn);
        pending.events.extend(events);
        let mut rendered = false;
        for IssuedEffect { id, effect, .. } in effects {
            if matches!(effect, Effect::Render { .. }) {
                run.resume(id, pass_through(&effect));
                rendered = true;
            } else {
                pending.effects.push((id, effect));
            }
        }
        if !rendered {
            return pending;
        }
    }
}

/// Steps `run` to its end and returns its result and events.
#[track_caller]
fn finish(run: &mut Run) -> (Result<String, Error>, Vec<Event>) {
    match run.step() {
        Step::Done { result, events, .. } => (result, events),
        other @ Step::Pending { .. } => panic!("the run is over: {other:?}"),
    }
}

/// The one model round among `effects`.
#[track_caller]
fn only_round(effects: &[(EffectId, Effect)]) -> (EffectId, &Effect) {
    let rounds: Vec<_> = effects
        .iter()
        .filter(|(_, effect)| matches!(effect, Effect::Chat { .. }))
        .collect();
    let [(id, effect)] = rounds.as_slice() else {
        panic!("exactly one model round: {effects:?}");
    };
    (*id, effect)
}

/// The text of the last message a model round sends.
fn last_sent(effect: &Effect) -> &str {
    let Effect::Chat { messages, .. } = effect else {
        panic!("a model round: {effect:?}");
    };
    messages.last().expect("a round sends a message").content()
}

/// A round answer requesting one `tools/echo` call, `c1`.
fn one_call() -> EffectAnswer {
    let call = ToolCall::from_parts("c1", "tools_echo", json!({ "value": "v" }))
        .expect("a scripted call is whole");
    let completion = Completion::from_result(CompletionResult::ToolCalls(vec![call]), "test-model")
        .expect("a scripted batch is accepted");
    EffectAnswer::Chat(Ok(Box::new(completion)))
}

/// The turns of the assistant replies among `events`.
fn reply_turns(events: &[Event]) -> Vec<u32> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::AssistantReply { turn, .. } => Some(*turn),
            _ => None,
        })
        .collect()
}

#[test]
fn a_message_queued_before_the_first_request_is_appended_before_it_under_the_main_chains_line() {
    let mut run = loop_run(
        "",
        &format!("messages:user('hi')\nmodels.loop()\n{RECORDS}"),
    );
    let _ = run.deliver("queued early".to_owned());
    let pending = step(&mut run);
    let (round, effect) = only_round(&pending.effects);
    assert!(
        last_sent(effect).ends_with(&arrived(MAIN_LINE, "queued early")),
        "the first request holds the message: {effect:?}"
    );
    run.resume(round, chat_answer(&text("ok")));
    let (result, _) = finish(&mut run);
    assert_eq!(
        result.expect("the loop returns"),
        format!(
            "user:hi|user:{}|assistant:ok",
            arrived(MAIN_LINE, "queued early")
        )
    );
}

#[test]
fn two_messages_queued_before_one_request_become_two_user_records_each_with_its_arrival_line() {
    let mut run = loop_run(
        "",
        &format!("messages:user('hi')\nmodels.loop()\n{RECORDS}"),
    );
    let _ = run.deliver("one".to_owned());
    let _ = run.deliver("two".to_owned());
    let pending = step(&mut run);
    let (round, effect) = only_round(&pending.effects);
    assert!(
        last_sent(effect).ends_with(&format!(
            "{}\n\n{}",
            arrived(MAIN_LINE, "one"),
            arrived(MAIN_LINE, "two")
        )),
        "the projection joins the records, each keeping its line: {effect:?}"
    );
    run.resume(round, chat_answer(&text("ok")));
    let (result, _) = finish(&mut run);
    assert_eq!(
        result.expect("the loop returns"),
        format!(
            "user:hi|user:{}|user:{}|assistant:ok",
            arrived(MAIN_LINE, "one"),
            arrived(MAIN_LINE, "two")
        )
    );
}

#[test]
fn a_message_while_a_request_has_no_answer_withdraws_it_and_the_loop_sends_it_again_holding_it() {
    let mut run = loop_run(
        "max_tool_iterations: 1",
        &format!("messages:user('hi')\nmodels.loop()\n{RECORDS}"),
    );
    let first = step(&mut run);
    let (withdrawn, _) = only_round(&first.effects);
    let _ = run.deliver("steer".to_owned());
    let second = step(&mut run);
    assert_eq!(
        second.withdrawn,
        [withdrawn],
        "the message withdraws the request"
    );
    let (reissued, effect) = only_round(&second.effects);
    assert!(
        last_sent(effect).ends_with(&arrived(MAIN_LINE, "steer")),
        "the request goes out again holding the message: {effect:?}"
    );
    run.resume(withdrawn, EffectAnswer::Dropped);
    run.resume(reissued, chat_answer(&text("done")));
    let (result, events) = finish(&mut run);
    assert_eq!(
        result.expect("nothing raises, and a cap of 1 is not spent"),
        format!(
            "user:hi|user:{}|assistant:done",
            arrived(MAIN_LINE, "steer")
        ),
        "nothing of the withdrawn request is in `messages`"
    );
    assert_eq!(
        reply_turns(&events),
        [1],
        "the withdrawn request took no turn"
    );
}

#[test]
fn a_message_while_the_final_request_has_no_answer_keeps_the_loop_going() {
    let mut run = loop_run(
        "",
        &format!("messages:user('hi')\nmodels.loop()\n{RECORDS}"),
    );
    let first = step(&mut run);
    let (stale, _) = only_round(&first.effects);
    let _ = run.deliver("one more thing".to_owned());
    let second = step(&mut run);
    let (fresh, _) = only_round(&second.effects);
    run.resume(stale, chat_answer(&text("stale reply")));
    run.resume(fresh, chat_answer(&text("fresh reply")));
    let (result, events) = finish(&mut run);
    assert_eq!(
        result.expect("the loop returns after the request it sent again"),
        format!(
            "user:hi|user:{}|assistant:fresh reply",
            arrived(MAIN_LINE, "one more thing")
        ),
        "the reply to the withdrawn request is discarded"
    );
    assert_eq!(reply_turns(&events), [1]);
}

#[test]
fn a_message_that_lands_as_the_final_request_answers_makes_the_loop_go_on() {
    let mut run = loop_run(
        "",
        &format!("messages:user('hi')\nmodels.loop()\n{RECORDS}"),
    );
    let first = step(&mut run);
    let (round, _) = only_round(&first.effects);
    run.resume(round, chat_answer(&text("first")));
    let _ = run.deliver("late".to_owned());
    let second = step(&mut run);
    assert!(
        second.withdrawn.is_empty(),
        "the answer applied first, so nothing was withdrawn: {:?}",
        second.withdrawn
    );
    let (next, effect) = only_round(&second.effects);
    assert!(
        last_sent(effect).ends_with(&arrived(MAIN_LINE, "late")),
        "the loop reads its inbox before it returns and goes on: {effect:?}"
    );
    run.resume(next, chat_answer(&text("second")));
    let (result, events) = finish(&mut run);
    assert_eq!(
        result.expect("the loop returns"),
        format!(
            "user:hi|assistant:first|user:{}|assistant:second",
            arrived(MAIN_LINE, "late")
        )
    );
    assert_eq!(
        [reply_turns(&second.events), reply_turns(&events)].concat(),
        [1, 2],
        "both rounds were answered, so each took a turn"
    );
}

#[test]
fn a_message_while_tool_calls_have_no_answer_withdraws_nothing_and_is_appended_before_the_next_request()
 {
    let mut run = loop_run(
        "",
        &format!("tools.offer('tools/echo')\nmessages:user('hi')\nmodels.loop()\n{RECORDS}"),
    );
    let first = step(&mut run);
    let (round, _) = only_round(&first.effects);
    run.resume(round, one_call());
    let calls = step(&mut run);
    let [(call, Effect::ToolCall { .. })] = calls.effects.as_slice() else {
        panic!("the round's one call is in flight: {:?}", calls.effects);
    };
    let call = *call;
    let _ = run.deliver("mid-tool".to_owned());
    let waiting = step(&mut run);
    assert!(
        waiting.effects.is_empty() && waiting.withdrawn.is_empty(),
        "the running call is left alone: {:?} {:?}",
        waiting.effects,
        waiting.withdrawn
    );
    run.resume(
        call,
        EffectAnswer::ToolCall(Ok(ToolOutput::trusted("echoed"))),
    );
    let next = step(&mut run);
    assert!(next.withdrawn.is_empty(), "{:?}", next.withdrawn);
    let (round, effect) = only_round(&next.effects);
    assert!(
        last_sent(effect).ends_with(&arrived(MAIN_LINE, "mid-tool")),
        "the next request holds the message after the call's result: {effect:?}"
    );
    run.resume(round, chat_answer(&text("done")));
    let (result, _) = finish(&mut run);
    assert_eq!(
        result.expect("the loop returns"),
        format!(
            "user:hi|assistant:|tool:echoed|user:{}|assistant:done",
            arrived(MAIN_LINE, "mid-tool")
        ),
        "the call kept its real result"
    );
}

#[test]
fn a_message_while_the_render_has_no_answer_withdraws_nothing_and_the_first_request_holds_it() {
    let mut run = loop_run(
        "",
        &format!("tools.offer('tools/echo')\nmessages:user('hi')\nmodels.loop()\n{RECORDS}"),
    );
    let (render, effect) = only_effect(run.step());
    assert!(
        matches!(&effect, Effect::Render { .. }),
        "the first round renders its offers before it goes out: {effect:?}"
    );
    let _ = run.deliver("mid-render".to_owned());
    let waiting = step(&mut run);
    assert!(
        waiting.effects.is_empty() && waiting.withdrawn.is_empty(),
        "a render is no model request, so the message waits in the inbox: {:?} {:?}",
        waiting.effects,
        waiting.withdrawn
    );
    run.resume(render, pass_through(&effect));
    let next = step(&mut run);
    let (round, effect) = only_round(&next.effects);
    assert!(
        last_sent(effect).ends_with(&arrived(MAIN_LINE, "mid-render")),
        "the round the render held goes out holding the message: {effect:?}"
    );
    run.resume(round, chat_answer(&text("done")));
    let (result, _) = finish(&mut run);
    assert_eq!(
        result.expect("the loop returns"),
        format!(
            "user:hi|user:{}|assistant:done",
            arrived(MAIN_LINE, "mid-render")
        )
    );
}

/// Main spawns Worker, whose loop's first request is out when Main sends
/// to it, and returns the record Worker appended.
const STEERED_TASK: &[(&str, &str)] = &[
    (
        "Main",
        "models.use('default')\n\
         local t = tasks.spawn('## Worker')\n\
         store.exists('busy')\n\
         tasks.send(t, 'shorter please')\n\
         local _, _, result = tasks.join_any({ t })\n\
         return result",
    ),
    (
        "Worker",
        "messages:user('write')\nmodels.loop()\nreturn messages[2].content",
    ),
];

#[test]
fn on_a_task_the_arrival_line_is_the_tasks() {
    let mut run = steered_run("", STEERED_TASK, &CancelHandle::new());
    let first = step(&mut run);
    let (round, _) = only_round(&first.effects);
    let (store, _) = first
        .effects
        .iter()
        .find(|(_, effect)| matches!(effect, Effect::Vfs { .. }))
        .expect("Main waits on its store call");
    run.resume(*store, EffectAnswer::Vfs(Ok(VfsOutcome::Bool(false))));
    let second = step(&mut run);
    assert_eq!(
        second.withdrawn,
        [round],
        "the send withdraws Worker's request"
    );
    let (reissued, effect) = only_round(&second.effects);
    assert!(last_sent(effect).ends_with(&arrived(TASK_LINE, "shorter please")));
    let worker = TaskId::from(ChainId::root().child(0));
    assert!(
        second.events.iter().any(|event| matches!(
            event,
            Event::InboxMessage { provenance, text, .. }
                if provenance.task == worker && text == "shorter please"
        )),
        "the send is reported under Worker: {:?}",
        second.events
    );
    run.resume(round, EffectAnswer::Dropped);
    run.resume(reissued, chat_answer(&text("done")));
    let (result, _) = finish(&mut run);
    assert_eq!(
        result.expect("Worker returns the record it appended"),
        arrived(TASK_LINE, "shorter please")
    );
}

#[test]
fn a_message_to_one_task_does_not_withdraw_another_chains_request() {
    let mut run = steered_run(
        "",
        &[
            (
                "Main",
                "models.use('default')\n\
                 local a = tasks.spawn('## A')\n\
                 local b = tasks.spawn('## B')\n\
                 store.exists('busy')\n\
                 tasks.send(a, 'for a')\n\
                 local results = tasks.join({ a, b })\n\
                 return results[1].result .. '|' .. results[2].result",
            ),
            (
                "A",
                "messages:user('a')\nmodels.loop()\nreturn #messages .. ''",
            ),
            (
                "B",
                "messages:user('b')\nmodels.loop()\nreturn #messages .. ''",
            ),
        ],
        &CancelHandle::new(),
    );
    let first = step(&mut run);
    let asking = |effects: &[(EffectId, Effect)], user: &str| {
        effects
            .iter()
            .find_map(|(id, effect)| {
                (matches!(effect, Effect::Chat { .. }) && last_sent(effect) == user).then_some(*id)
            })
            .unwrap_or_else(|| panic!("a round asking {user:?}: {effects:?}"))
    };
    let (a, b) = (asking(&first.effects, "a"), asking(&first.effects, "b"));
    let (store, _) = first
        .effects
        .iter()
        .find(|(_, effect)| matches!(effect, Effect::Vfs { .. }))
        .expect("Main waits on its store call");
    run.resume(*store, EffectAnswer::Vfs(Ok(VfsOutcome::Bool(false))));
    let second = step(&mut run);
    assert_eq!(second.withdrawn, [a], "only A's request is withdrawn");
    let (again, _) = only_round(&second.effects);
    run.resume(a, EffectAnswer::Dropped);
    run.resume(again, chat_answer(&text("a done")));
    run.resume(b, chat_answer(&text("b done")));
    let (result, _) = finish(&mut run);
    assert_eq!(
        result.expect("both tasks return"),
        "3|2",
        "A appended the message, and B's request ran untouched"
    );
}

#[test]
fn a_tool_call_answered_dropped_raises_canceled_and_leaves_the_queued_message_in_the_inbox() {
    let mut run = loop_run(
        "",
        "tools.offer('tools/echo')\n\
         messages:user('hi')\n\
         local ok, err = pcall(models.loop)\n\
         return tostring(ok) .. ':' .. err.kind .. '|' .. tasks.receive()",
    );
    let first = step(&mut run);
    let (round, _) = only_round(&first.effects);
    run.resume(round, one_call());
    let calls = step(&mut run);
    let [(call, Effect::ToolCall { .. })] = calls.effects.as_slice() else {
        panic!("the round's one call is in flight: {:?}", calls.effects);
    };
    let _ = run.deliver("kept".to_owned());
    run.resume(*call, EffectAnswer::Dropped);
    let (result, _) = finish(&mut run);
    assert_eq!(
        result.expect("the pcall catches the stop"),
        "false:canceled|kept",
        "the stop raised at the call site, and the next reader took the message"
    );
}

#[test]
fn a_dropped_request_and_a_message_applied_in_the_same_step_raise_canceled_and_leave_it_queued() {
    for deliver_first in [true, false] {
        let mut run = loop_run(
            "",
            "messages:user('hi')\n\
             local ok, err = pcall(models.loop)\n\
             return tostring(ok) .. ':' .. err.kind .. '|' .. #messages .. '|' .. tasks.receive()",
        );
        let first = step(&mut run);
        let (round, _) = only_round(&first.effects);
        if deliver_first {
            let _ = run.deliver("kept".to_owned());
            run.resume(round, EffectAnswer::Dropped);
        } else {
            run.resume(round, EffectAnswer::Dropped);
            let _ = run.deliver("kept".to_owned());
        }
        let (result, _) = finish(&mut run);
        assert_eq!(
            result.expect("the pcall catches the stop"),
            "false:canceled|1|kept",
            "deliver first: {deliver_first}: the stop wins, and the message waits unappended"
        );
    }
}

#[test]
fn a_canceled_run_hands_back_the_messages_its_steered_loop_never_resumed_with() {
    let cancel = CancelHandle::new();
    let mut run = steered_run(
        "",
        &[(
            "Only",
            "models.use('default')\nmessages:user('hi')\nmodels.loop()",
        )],
        &cancel,
    );
    let first = step(&mut run);
    let (round, _) = only_round(&first.effects);
    let _ = run.deliver("one".to_owned());
    let _ = run.deliver("two".to_owned());
    cancel.cancel();
    let ending = step(&mut run);
    assert_eq!(
        ending.withdrawn,
        [round],
        "the round is withdrawn, and the run waits for its answer"
    );
    run.resume(round, EffectAnswer::Dropped);
    let (result, unread) = done(&mut run);
    assert!(
        matches!(&result, Err(error) if error.is_canceled()),
        "{result:?}"
    );
    assert_eq!(
        unread,
        ["one", "two"],
        "the loop never resumed with the message that withdrew its round, so it comes back first"
    );
}
