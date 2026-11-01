//! The step's `withdrawn` list: a step that cancels a task parked on a
//! batch lists the calls it orphaned, the calls the same step issued
//! included, in effect-id order however many tasks it cancels, and the
//! step that decides the run lists every effect still outstanding, of
//! every kind, the ones it issued included.

use runvil_types::cancel::CancelHandle;

use super::*;
use crate::execute::tests::fixtures::echo_tool;
use crate::execute::tests::models_loop::loop_scenario;

/// The effects one step issued, in issue order, and the tool calls it
/// withdrew. Panics when the run is over.
fn withdrawals(step: Step) -> (Vec<(EffectId, Effect)>, Vec<EffectId>) {
    let (effects, withdrawn) = match step {
        Step::Pending {
            effects, withdrawn, ..
        } => (effects, withdrawn),
        other @ Step::Done { .. } => panic!("the run is still going: {other:?}"),
    };
    let effects = effects
        .into_iter()
        .map(|issued| (issued.id, issued.effect))
        .collect();
    (effects, withdrawn)
}

/// Main spawns Child and waits on one model round while Child parks on a
/// tool-call batch, then cancels Child and lingers on a second round.
const CANCELLED_CHILD: &str = "---\nname: loop\ndescription: d\nrunvil: 0\n---\n\n# Loop\n\n\
     ## Main\n\n```lua\n\
     local t = tasks.spawn('## Child')\n\
     models.infer('wait')\n\
     tasks.cancel(t)\n\
     models.infer('linger')\n\
     return 'cancelled'\n```\n\n\
     ## Child\n\n```lua\n\
     tools.offer('tools/echo')\n\
     messages:user('go')\n\
     models.loop()\n\
     return 'child done'\n```\n";

/// A run of `source` over the loop models with `echo` offered, under the
/// cancel flag `cancel`, stepped by hand.
#[track_caller]
fn echo_run(source: &str, cancel: &CancelHandle) -> Run {
    let cancel = cancel.clone();
    loop_scenario(source)
        .offer("echo", echo_tool())
        .config(move |ctx| ctx.with_cancel(cancel))
        .into_run()
}

/// A run of [`CANCELLED_CHILD`] under the cancel flag `cancel`, past its
/// first steps and Child's render, with the ids of Main's `wait` round and
/// Child's loop round.
fn cancelled_child(cancel: &CancelHandle) -> (Run, EffectId, EffectId) {
    let mut run = echo_run(CANCELLED_CHILD, cancel);
    let (rounds, withdrawn) = step_past_renders(&mut run);
    assert!(
        withdrawn.is_empty(),
        "nothing is withdrawn yet: {withdrawn:?}"
    );
    let origin = |wanted: ReplyOrigin| {
        rounds
            .iter()
            .find_map(|(id, effect)| match effect {
                Effect::Chat { round, .. } if round.origin == wanted => Some(*id),
                _ => None,
            })
            .unwrap_or_else(|| panic!("a {wanted:?} round: {rounds:?}"))
    };
    let (infer, round) = (origin(ReplyOrigin::Infer), origin(ReplyOrigin::Chat));
    (run, infer, round)
}

/// Child's round answer: two echo calls, `c1` then `c2`.
fn two_echoes() -> EffectAnswer {
    calls_reply(&[
        ("c1", "echo", json!({ "value": "one" })),
        ("c2", "echo", json!({ "value": "two" })),
    ])
}

/// The ids of the tool calls among `effects`, in issue order.
fn tool_call_ids(effects: &[(EffectId, Effect)]) -> Vec<EffectId> {
    effects
        .iter()
        .filter(|(_, effect)| matches!(effect, Effect::ToolCall { .. }))
        .map(|(id, _)| *id)
        .collect()
}

#[test]
fn cancelling_a_task_withdraws_its_unanswered_tool_calls() {
    let (mut run, infer, round) = cancelled_child(&CancelHandle::new());
    run.resume(round, two_echoes());
    let (issued, withdrawn) = withdrawals(run.step());
    let calls = tool_call_ids(&issued);
    assert_eq!(calls.len(), 2, "both calls are in flight: {issued:?}");
    assert!(
        withdrawn.is_empty(),
        "issuing withdraws nothing: {withdrawn:?}"
    );
    run.resume(infer, chat_answer(&text("go")));
    let (linger, effect) = {
        let (mut issued, withdrawn) = withdrawals(run.step());
        assert_eq!(
            withdrawn, calls,
            "the cancel withdraws both calls, c1 first"
        );
        assert_eq!(issued.len(), 1, "only Main's next round: {issued:?}");
        issued.remove(0)
    };
    assert!(
        matches!(&effect, Effect::Chat { round, .. } if round.origin == ReplyOrigin::Infer),
        "the cancel issues only Main's next round: {effect:?}"
    );
    for id in calls {
        run.resume(id, EffectAnswer::Dropped);
    }
    run.resume(linger, chat_answer(&text("done")));
    let result = match run.step() {
        Step::Done { outcome, .. } => outcome,
        other @ Step::Pending { .. } => panic!("Main's second round ends the run: {other:?}"),
    };
    assert_eq!(text_of(result), "cancelled");
}

#[test]
fn a_call_issued_and_withdrawn_in_one_step_is_listed_twice() {
    let (mut run, infer, round) = cancelled_child(&CancelHandle::new());
    run.resume(round, two_echoes());
    run.resume(infer, chat_answer(&text("go")));
    let (issued, withdrawn) = withdrawals(run.step());
    let calls = tool_call_ids(&issued);
    assert_eq!(
        calls.len(),
        2,
        "Child issues both calls before Main cancels it: {issued:?}"
    );
    assert_eq!(withdrawn, calls, "the same step withdraws both calls");
    for (id, effect) in &issued {
        let answer = match effect {
            Effect::ToolCall { .. } => EffectAnswer::Dropped,
            _ => chat_answer(&text("done")),
        };
        run.resume(*id, answer);
    }
    let result = match run.step() {
        Step::Done { outcome, .. } => outcome,
        other @ Step::Pending { .. } => panic!("Main's second round ends the run: {other:?}"),
    };
    assert_eq!(text_of(result), "cancelled");
}

#[test]
fn the_step_that_decides_the_run_withdraws_every_outstanding_effect() {
    let cancel = CancelHandle::new();
    let (mut run, infer, round) = cancelled_child(&cancel);
    run.resume(round, two_echoes());
    let (issued, _) = withdrawals(run.step());
    let calls = tool_call_ids(&issued);
    assert_eq!(calls.len(), 2, "both calls are in flight: {issued:?}");
    cancel.cancel();
    let (issued, withdrawn) = withdrawals(run.step());
    assert!(
        issued.is_empty(),
        "a torn-down run issues nothing: {issued:?}"
    );
    assert_eq!(
        withdrawn,
        [infer, calls[0], calls[1]],
        "the cancel withdraws Main's round and both of Child's calls, in effect-id order"
    );
    for id in withdrawn {
        run.resume(id, EffectAnswer::Dropped);
    }
    let result = match run.step() {
        Step::Done { outcome, .. } => outcome,
        other @ Step::Pending { .. } => {
            panic!("every orphan is answered, so the run is done: {other:?}")
        }
    };
    assert!(
        matches!(result, RunOutcome::Cancelled),
        "the run ends cancelled: {result:?}"
    );
}

/// Main spawns A and then B, each parked on a one-call batch, waits on a
/// model round, then cancels B before A and lingers on a second round.
const CANCELLED_PAIR: &str = "---\nname: loop\ndescription: d\nrunvil: 0\n---\n\n# Loop\n\n\
     ## Main\n\n```lua\n\
     local a = tasks.spawn('## A')\n\
     local b = tasks.spawn('## B')\n\
     models.infer('wait')\n\
     tasks.cancel(b)\n\
     tasks.cancel(a)\n\
     models.infer('linger')\n\
     return 'cancelled'\n```\n\n\
     ## A\n\n```lua\n\
     tools.offer('tools/echo')\n\
     messages:user('a')\n\
     models.loop()\n\
     return 'a done'\n```\n\n\
     ## B\n\n```lua\n\
     tools.offer('tools/echo')\n\
     messages:user('b')\n\
     models.loop()\n\
     return 'b done'\n```\n";

#[test]
fn one_step_lists_every_tasks_withdrawn_calls_in_effect_id_order() {
    let mut run = echo_run(CANCELLED_PAIR, &CancelHandle::new());
    let (rounds, _) = step_past_renders(&mut run);
    let asking = |user: &str| {
        rounds
            .iter()
            .find_map(|(id, effect)| match effect {
                Effect::Chat { messages, .. }
                    if messages.iter().any(|message| message.content() == user) =>
                {
                    Some(*id)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("a round asking {user:?}: {rounds:?}"))
    };
    for task in ["a", "b"] {
        let call = (task, "echo", json!({ "value": task }));
        run.resume(asking(task), calls_reply(&[call]));
    }
    let (issued, _) = withdrawals(run.step());
    let values: Vec<&str> = issued
        .iter()
        .map(|(_, effect)| call_value(effect))
        .collect();
    assert_eq!(values, ["a", "b"], "A's call issues before B's");
    run.resume(asking("wait"), chat_answer(&text("go")));
    let (linger, withdrawn) = withdrawals(run.step());
    assert_eq!(
        withdrawn,
        tool_call_ids(&issued),
        "Main cancels B first, yet A's call is listed first"
    );
    for (id, _) in &issued {
        run.resume(*id, EffectAnswer::Dropped);
    }
    let linger = match linger.as_slice() {
        [(linger, _)] => *linger,
        other => panic!("the cancels issue only Main's next round: {other:?}"),
    };
    run.resume(linger, chat_answer(&text("done")));
    let result = match run.step() {
        Step::Done { outcome, .. } => outcome,
        other @ Step::Pending { .. } => panic!("Main's second round ends the run: {other:?}"),
    };
    assert_eq!(text_of(result), "cancelled");
}

/// Main spawns Store, Call, and Fast, waits on Fast under a timeout, then
/// cancels the other two and returns Fast's result, all in the run's first
/// step.
const DECIDED_IN_ONE_STEP: &str = "---\nname: loop\ndescription: d\nrunvil: 0\n---\n\n# Loop\n\n\
     ## Main\n\n```lua\n\
     local stored = tasks.spawn('## Store')\n\
     local called = tasks.spawn('## Call')\n\
     local fast = tasks.spawn('## Fast')\n\
     local _t, _ok, result = tasks.join_any({ fast }, { timeout = 30 })\n\
     tasks.cancel(stored)\n\
     tasks.cancel(called)\n\
     return result\n```\n\n\
     ## Store\n\n```lua\n\
     store.write('slow.md', 'never')\n\
     return 'stored'\n```\n\n\
     ## Call\n\n```lua\n\
     return tools.call('tools/echo', { value = 'never' })\n```\n\n\
     ## Fast\n\n```lua\n\
     return 'fast'\n```\n";

#[test]
fn the_step_that_decides_the_run_withdraws_the_effects_it_issued_of_every_kind() {
    let mut run = echo_run(DECIDED_IN_ONE_STEP, &CancelHandle::new());
    let (issued, withdrawn) = withdrawals(run.step());
    let kinds: Vec<&str> = issued
        .iter()
        .map(|(_, effect)| match effect {
            Effect::Timer { .. } => "timer",
            Effect::Vfs { .. } => "store",
            Effect::ToolCall { .. } => "tool call",
            other => panic!("only the timer, the write, and the call: {other:?}"),
        })
        .collect();
    assert_eq!(
        kinds,
        ["timer", "store", "tool call"],
        "Main's timeout timer, then Store's write and Call's call"
    );
    let ids: Vec<EffectId> = issued.iter().map(|(id, _)| *id).collect();
    assert_eq!(
        withdrawn, ids,
        "the step that decides the run withdraws every effect it issued, in effect-id order"
    );
    for id in withdrawn {
        run.resume(id, EffectAnswer::Dropped);
    }
    let result = match run.step() {
        Step::Done { outcome, .. } => outcome,
        other @ Step::Pending { .. } => panic!("every orphan is answered: {other:?}"),
    };
    assert_eq!(text_of(result), "fast");
}
