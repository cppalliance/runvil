//! A `Chat`, `ToolCall`, or `Timer` effect the caller drops while the
//! run's cancel flag is clear: the chain resumes with the canceled error,
//! which a `pcall` catches so the run continues, and which ends the run
//! `Cancelled` when nothing catches it. A `Chat` drop is covered for both
//! kinds of round, a nested `models.infer` round and a `models.loop` round.
//! A `Timer` drop is covered for the timed `tasks.join_any` and
//! `tasks.join` waits, and under a run cancel, where the drop wakes no one.

use std::collections::BTreeMap;
use std::sync::Arc;

use runvil_types::cancel::CancelHandle;
use runvil_types::event::{Event, ReplyOrigin};
use runvil_types::ids::RoundId;
use runvil_types::models::{ModelRole, ThinkingMode};
use runvil_types::tools::{ToolCatalog, ToolId, ToolInfo};
use serde_json::json;

use super::run_params;
use crate::execute::RunOutcome;
use crate::execute::run::{
    AnswerRecord, Effect, EffectAnswer, EffectId, EffectRecord, Round, Run, Step,
};
use crate::model::{Completion, CompletionResult};
use crate::prompt::Prompt;
use crate::test_support::scenario::{chat_answer, only_effect, test_model_with};
use crate::test_support::scripted_chat::text;

/// A run over one section whose Lua is `body`, on the `default` role
/// filled by `test-model`, and `tools/echo` in the catalog, under the
/// cancel flag `cancel`.
#[track_caller]
fn bound_run(body: &str, cancel: &CancelHandle) -> Run {
    bound_sections(
        &format!("## Only\n\n```lua\nmodels.use('default')\n{body}\n```\n"),
        cancel,
    )
}

/// A run over `sections`, the prompt's H2 sections, on the `default` role
/// filled by `test-model`, and `tools/echo` in the catalog, under the
/// cancel flag `cancel`.
#[track_caller]
fn bound_sections(sections: &str, cancel: &CancelHandle) -> Run {
    let source = format!("---\nname: t\ndescription: d\nrunvil: 0\n---\n\n# Run\n\n{sections}");
    let prompt = Prompt::parse(&source, "prompt", "run-test")
        .0
        .expect("the drop test prompt parses");
    let model = test_model_with("test-model", 4096, ThinkingMode::Never);
    let echo = ToolInfo::new(
        ToolId::parse("tools/echo").expect("a valid tool id"),
        "Echoes its value.",
        json!({ "type": "object" }),
    );
    let catalog = ToolCatalog::new([echo]).expect("one tool is a valid catalog");
    let ctx = run_params()
        .with_tools(catalog)
        .with_models(BTreeMap::from([(ModelRole::Default, model)]))
        .with_cancel(cancel.clone());
    Run::new(Arc::new(prompt), "", ctx)
}

/// Steps `run` to its next effect and answers it `Dropped`, returning the
/// effect's record and the answer's.
#[track_caller]
fn drop_next(run: &mut Run) -> (EffectRecord, AnswerRecord) {
    let (id, effect) = only_effect(run.step());
    let answer = EffectAnswer::Dropped;
    let records = (effect.to_record(), answer.to_record());
    run.resume(id, answer);
    records
}

#[test]
fn a_dropped_chat_or_tool_call_resumes_a_pcall_with_the_canceled_error_and_the_run_continues() {
    let cancel = CancelHandle::new();
    let mut run = bound_run(
        "local chat_ok, chat_err = pcall(models.infer, 'dropped')\n\
         local tool_ok, tool_err = pcall(tools.call, 'tools/echo', { value = 'dropped' })\n\
         local kept = models.infer('kept')\n\
         return tostring(chat_ok) .. ':' .. chat_err.kind .. '|' \
         .. tostring(tool_ok) .. ':' .. tool_err.kind .. '|' .. kept",
        &cancel,
    );
    let chat = drop_next(&mut run);
    assert!(
        matches!(
            &chat,
            (EffectRecord::Chat { round, .. }, AnswerRecord::Dropped) if *round == RoundId::new(0)
        ),
        "the record holds the round and its dropped answer: {chat:?}"
    );
    let tool = drop_next(&mut run);
    assert!(
        matches!(
            &tool,
            (EffectRecord::ToolCall { alias, .. }, AnswerRecord::Dropped) if alias == "tools/echo"
        ),
        "the record holds the call and its dropped answer: {tool:?}"
    );
    assert!(
        !cancel.is_canceled(),
        "the drops left the run's cancel flag clear"
    );
    let (id, effect) = only_effect(run.step());
    assert!(
        matches!(&effect, Effect::Chat { round, .. } if round.id == RoundId::new(1)),
        "the run went on to its next round: {effect:?}"
    );
    let completion = Completion::from_result(CompletionResult::Text("kept".to_owned()), "m")
        .expect("a text result is accepted");
    run.resume(id, EffectAnswer::Chat(Ok(Box::new(completion))));
    let Step::Done { outcome, events } = run.step() else {
        panic!("the run ends once its last round is answered");
    };
    let RunOutcome::Completed { final_text: text } = outcome else {
        panic!("a caught drop leaves the run to succeed: {outcome:?}");
    };
    assert_eq!(text, "false:canceled|false:canceled|kept");
    let turns: Vec<u32> = events
        .iter()
        .filter_map(|event| match event {
            Event::AssistantReply { turn, .. } => Some(*turn),
            _ => None,
        })
        .collect();
    assert_eq!(
        turns,
        vec![1],
        "the dropped round took no turn, so the answered one is the first"
    );
}

#[test]
fn a_dropped_loop_round_resumes_a_pcall_with_the_canceled_error_and_the_run_continues() {
    let cancel = CancelHandle::new();
    let mut run = bound_run(
        "messages:user('asked')\n\
         local loop_ok, loop_err = pcall(models.loop)\n\
         local after_drop = #messages\n\
         models.loop()\n\
         return tostring(loop_ok) .. ':' .. loop_err.kind .. '|' .. after_drop \
         .. '|' .. #messages .. ':' .. messages[#messages].content",
        &cancel,
    );
    let (id, effect) = only_effect(run.step());
    assert!(
        matches!(
            &effect,
            Effect::Chat { round, .. }
                if *round == Round { id: RoundId::new(0), origin: ReplyOrigin::Chat }
        ),
        "the loop's first round is a chat round numbered 0: {effect:?}"
    );
    let records = (effect.to_record(), EffectAnswer::Dropped.to_record());
    run.resume(id, EffectAnswer::Dropped);
    assert!(
        matches!(
            &records,
            (EffectRecord::Chat { round, .. }, AnswerRecord::Dropped) if *round == RoundId::new(0)
        ),
        "the record holds the loop round and its dropped answer: {records:?}"
    );
    assert!(
        !cancel.is_canceled(),
        "the drop left the run's cancel flag clear"
    );
    let (id, effect) = only_effect(run.step());
    assert!(
        matches!(
            &effect,
            Effect::Chat { round, .. }
                if *round == Round { id: RoundId::new(1), origin: ReplyOrigin::Chat }
        ),
        "the run went on to the next loop's round: {effect:?}"
    );
    let completion = Completion::from_result(CompletionResult::Text("kept".to_owned()), "m")
        .expect("a text result is accepted");
    run.resume(id, EffectAnswer::Chat(Ok(Box::new(completion))));
    let Step::Done { outcome, events } = run.step() else {
        panic!("the run ends once its last round is answered");
    };
    let RunOutcome::Completed { final_text: text } = outcome else {
        panic!("a caught drop leaves the run to succeed: {outcome:?}");
    };
    assert_eq!(
        text, "false:canceled|1|2:kept",
        "the dropped round appended nothing, and the answered one appended its reply"
    );
    let turns: Vec<u32> = events
        .iter()
        .filter_map(|event| match event {
            Event::AssistantReply { turn, .. } => Some(*turn),
            _ => None,
        })
        .collect();
    assert_eq!(
        turns,
        vec![1],
        "the dropped round took no turn, so the answered one is the first"
    );
}

#[test]
fn an_uncaught_dropped_chat_or_tool_call_ends_the_run_canceled() {
    for body in [
        "return models.infer('dropped')",
        "messages:user('dropped')\nreturn models.loop()",
        "return tools.call('tools/echo', { value = 'dropped' })",
    ] {
        let mut run = bound_run(body, &CancelHandle::new());
        let _ = drop_next(&mut run);
        let Step::Done { outcome, .. } = run.step() else {
            panic!("{body}: the uncaught drop ends the run with nothing outstanding");
        };
        assert!(
            matches!(outcome, RunOutcome::Cancelled),
            "{body}: the canceled error ends the run canceled: {outcome:?}"
        );
    }
}

/// A run whose `## Main` runs `main` beside the `## Child` it spawns, whose
/// one round stays out until the test answers it, under the cancel flag
/// `cancel`.
#[track_caller]
fn timed_run(main: &str, cancel: &CancelHandle) -> Run {
    bound_sections(
        &format!(
            "## Main\n\n```lua\nmodels.use('default')\n{main}\n```\n\n\
             ## Child\n\n```lua\nmodels.use('default')\nreturn models.infer('child')\n```\n"
        ),
        cancel,
    )
}

/// A main that catches its timed `wait` over the child, then joins the
/// child untimed and reports both.
fn caught_wait(wait: &str) -> String {
    format!(
        "local t = tasks.spawn('## Child')\n\
         local ok, err = pcall({wait}, {{ t }}, {{ timeout = 30 }})\n\
         local _t, _ok, result = tasks.join_any({{ t }})\n\
         return tostring(ok) .. ':' .. err.kind .. '|' .. result"
    )
}

/// Steps `run` to the main's parked timed wait and returns the wait's
/// timer and the child's round.
#[track_caller]
fn parked_wait(run: &mut Run) -> (EffectId, EffectId) {
    let Step::Pending { effects, .. } = run.step() else {
        panic!("the main parks on its timed wait while the child's round is out");
    };
    let issued = |kind: fn(&Effect) -> bool| {
        let Some(issued) = effects.iter().find(|issued| kind(&issued.effect)) else {
            panic!("the step issued the timer and the round: {effects:?}");
        };
        issued.id
    };
    let timer = issued(|effect| matches!(effect, Effect::Timer { .. }));
    let chat = issued(|effect| matches!(effect, Effect::Chat { .. }));
    assert_eq!(
        effects.len(),
        2,
        "only the timer and the round: {effects:?}"
    );
    (timer, chat)
}

#[test]
fn a_dropped_timer_raises_the_canceled_error_into_a_timed_waits_pcall_and_the_run_continues() {
    for wait in ["tasks.join_any", "tasks.join"] {
        let cancel = CancelHandle::new();
        let mut run = timed_run(&caught_wait(wait), &cancel);
        let (timer, chat) = parked_wait(&mut run);
        run.resume(timer, EffectAnswer::Dropped);
        assert!(
            !cancel.is_canceled(),
            "{wait}: the drop left the run's cancel flag clear"
        );
        let Step::Pending { effects, .. } = run.step() else {
            panic!("{wait}: the main goes on to wait on its child untimed");
        };
        assert!(
            effects.is_empty(),
            "{wait}: the caught drop issued nothing new: {effects:?}"
        );
        run.resume(chat, chat_answer(&text("child")));
        let Step::Done { outcome, .. } = run.step() else {
            panic!("{wait}: the run ends once the child's round is answered");
        };
        let RunOutcome::Completed { final_text: out } = outcome else {
            panic!("{wait}: a caught drop leaves the run to succeed: {outcome:?}");
        };
        assert_eq!(
            out, "false:canceled|child",
            "{wait}: the wait raised the canceled error and the child kept running"
        );
    }
}

#[test]
fn an_uncaught_dropped_timer_ends_the_run_canceled() {
    for wait in ["tasks.join_any", "tasks.join"] {
        let mut run = timed_run(
            &format!(
                "local t = tasks.spawn('## Child')\n\
                 {wait}({{ t }}, {{ timeout = 30 }})\n\
                 return 'waited'"
            ),
            &CancelHandle::new(),
        );
        let (timer, chat) = parked_wait(&mut run);
        run.resume(timer, EffectAnswer::Dropped);
        let Step::Pending {
            effects, withdrawn, ..
        } = run.step()
        else {
            panic!("{wait}: the child's round is still out, so the run is not done");
        };
        assert!(effects.is_empty(), "{wait}: nothing new: {effects:?}");
        assert_eq!(
            withdrawn,
            [chat],
            "{wait}: the uncaught error decided the run and withdrew the child's round"
        );
        run.resume(chat, EffectAnswer::Dropped);
        let Step::Done { outcome, .. } = run.step() else {
            panic!("{wait}: the run ends once the orphan is answered");
        };
        assert!(
            matches!(outcome, RunOutcome::Cancelled),
            "{wait}: the canceled error ends the run canceled: {outcome:?}"
        );
    }
}

#[test]
fn under_a_run_cancel_a_dropped_timer_ends_the_run_canceled_without_resuming_the_wait() {
    let cancel = CancelHandle::new();
    let mut run = timed_run(&caught_wait("tasks.join_any"), &cancel);
    let (timer, chat) = parked_wait(&mut run);
    cancel.cancel();
    run.resume(timer, EffectAnswer::Dropped);
    run.resume(chat, EffectAnswer::Dropped);
    let Step::Done { outcome, .. } = run.step() else {
        panic!("the cancel ends the run once both effects are answered");
    };
    assert!(
        matches!(outcome, RunOutcome::Cancelled),
        "the wait's pcall never resumed, so the run did not go on: {outcome:?}"
    );
}
