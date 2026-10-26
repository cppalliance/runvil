//! Tests for what a tool call's origin names beyond its section: the
//! section entry that made the call, the value that entry reads as
//! `sys.id`, and for a model call the model of the round that requested
//! it, which the section's message list recorded when the round was sent.
//! A script call names no model.

use runvil_parser::test_support::{one_section, prompt};

use super::fixtures::echo_tool;
use super::models_loop::{loop_model_name, loop_scenario};
use super::*;
use crate::execute::run::{EffectRecord, ToolCallOrigin, ToolCaller};
use crate::test_support::rig::{Rig, RigTrace};
use crate::test_support::scenario::EXECUTION;

/// Runs `source` over the loop models with `echo` in the catalog, its
/// model rounds answered by `replies` in order.
#[track_caller]
fn run_echo(source: &str, replies: Vec<ScriptedReply>) -> (RigTrace, ScriptedChat) {
    let gateway = ScriptedChat::new(replies);
    let rig = Rig::run(
        loop_scenario(source)
            .offer("echo", echo_tool())
            .chat(&gateway),
    );
    (rig, gateway)
}

/// Runs `source`, which makes no model round, with `echo` in the catalog.
#[track_caller]
fn run_scripted(source: &str) -> RigTrace {
    Rig::run(loop_scenario(source).offer("echo", echo_tool()))
}

/// The origin of every tool call the run issued, in issue order.
fn origins(rig: &RigTrace) -> Vec<ToolCallOrigin> {
    rig.issued()
        .iter()
        .filter_map(|record| match record {
            EffectRecord::ToolCall { origin, .. } => Some(origin.clone()),
            _ => None,
        })
        .collect()
}

/// The model each tool call's origin names, in issue order.
fn models(rig: &RigTrace) -> Vec<Option<String>> {
    origins(rig)
        .into_iter()
        .map(|origin| origin.model)
        .collect()
}

/// The origin of a script call made by the entry `entry` of `section`.
fn script_call(section: &str, entry: &str) -> ToolCallOrigin {
    ToolCallOrigin::new(EXECUTION, section, ToolCaller::Script).with_entry(entry)
}

/// The model each round asked for, in the order the rounds left.
fn request_models(gateway: &ScriptedChat) -> Vec<String> {
    gateway
        .requests()
        .iter()
        .map(|body| body.options.model().to_owned())
        .collect()
}

/// A section that offers `echo` and an adhoc `grab` whose handler runs
/// `handler`, then loops once and returns the final record's text.
fn grab_then_loop(handler: &str) -> String {
    one_section(&format!(
        "tools.offer('tools/echo')\n\
         tools.offer(tools.adhoc({{ name = 'grab', description = 'Grab a value', parameters = {{ value = 'string' }} }}, function()\n\
           {handler}\n\
         end))\n\
         messages:user('Use the tools.')\n\
         models.loop()\n\
         return messages[#messages].content"
    ))
}

/// The batch a [`grab_then_loop`] section's first round requests: `grab`,
/// then `echo`.
fn grab_then_echo() -> ScriptedReply {
    calls(&[
        ("c1", "grab", r#"{"value":"a"}"#),
        ("c2", "echo", r#"{"value":"b"}"#),
    ])
}

#[test]
fn a_model_calls_origin_names_its_rounds_model_and_a_script_calls_names_none() {
    let source = one_section(
        "tools.offer('tools/echo')\n\
         messages:user('hello')\n\
         models.loop()\n\
         return tools.call('tools/echo', { value = 'script' })",
    );
    let (rig, _) = run_echo(
        &source,
        vec![
            calls(&[("call_1", "echo", r#"{"value":"model"}"#)]),
            text("done"),
        ],
    );
    assert_eq!(rig.trace().text(), "echoed: script");
    assert_eq!(
        origins(&rig),
        vec![
            ToolCallOrigin::new(EXECUTION, "Only", ToolCaller::Model)
                .with_entry("0.1")
                .with_model(loop_model_name("writer")),
            script_call("Only", "0.1"),
        ],
        "the model call names its round's model, and the script call after that round names none"
    );
}

#[test]
fn a_section_that_switches_models_between_rounds_reports_each_rounds_model() {
    let source = one_section(
        "tools.offer('tools/echo')\n\
         messages:user('first')\n\
         models.loop()\n\
         models.use('other')\n\
         messages:user('second')\n\
         models.loop()\n\
         return messages[#messages].content",
    );
    let (rig, _) = run_echo(
        &source,
        vec![
            calls(&[("call_1", "echo", r#"{"value":"a"}"#)]),
            text("one"),
            calls(&[("call_2", "echo", r#"{"value":"b"}"#)]),
            text("two"),
        ],
    );
    assert_eq!(rig.trace().text(), "two");
    assert_eq!(
        models(&rig),
        vec![
            Some(loop_model_name("writer")),
            Some(loop_model_name("other"))
        ],
        "each call names the model of the round that requested it"
    );
}

#[test]
fn an_adhoc_handlers_infer_on_another_model_leaves_the_batchs_later_call_on_its_rounds_model() {
    let (rig, gateway) = run_echo(
        &grab_then_loop("return models.get('other'):infer('aside')"),
        vec![grab_then_echo(), text("aside reply"), text("final answer")],
    );
    assert_eq!(rig.trace().text(), "final answer");
    assert_eq!(
        request_models(&gateway),
        ["test-model", "other-model", "test-model"],
        "the handler's round goes to another model between the loop's rounds"
    );
    assert_eq!(
        models(&rig),
        vec![Some(loop_model_name("writer"))],
        "the call after the handler names the batch round's model"
    );
}

#[test]
fn an_adhoc_handlers_model_switch_leaves_the_batchs_later_call_on_its_rounds_model() {
    let (rig, gateway) = run_echo(
        &grab_then_loop("models.use('other')\nreturn 'switched'"),
        vec![grab_then_echo(), text("final answer")],
    );
    assert_eq!(rig.trace().text(), "final answer");
    assert_eq!(
        request_models(&gateway),
        ["test-model", "other-model"],
        "the switch steers the loop's next round"
    );
    assert_eq!(
        models(&rig),
        vec![Some(loop_model_name("writer"))],
        "the call after the handler names the batch round's model"
    );
}

#[test]
fn re_entering_a_section_gives_each_entrys_call_its_own_entry_id() {
    let source = prompt(
        "",
        &[
            (
                "Loop",
                "tools.call('tools/echo', { value = 'x' })\n\
                 if var.again then return 'done' end\n\
                 var.again = true",
            ),
            ("Back", "jump('## Loop')"),
        ],
    );
    let rig = run_scripted(&source);
    assert_eq!(rig.trace().text(), "done");
    assert_eq!(
        origins(&rig),
        vec![script_call("Loop", "0.1"), script_call("Loop", "0.3")],
        "both entries share the section's name, and each names its own entry"
    );
}

#[test]
fn two_fanout_arms_of_one_section_name_different_entries() {
    let source = prompt(
        "",
        &[
            (
                "Main",
                "local r = tasks.fanout('## Worker', { 'a', 'b' })\n\
                 return r[1].text .. '|' .. r[2].text",
            ),
            (
                "Worker",
                "return tools.call('tools/echo', { value = item })",
            ),
        ],
    );
    let rig = run_scripted(&source);
    assert_eq!(rig.trace().text(), "echoed: a|echoed: b");
    let mut seen = origins(&rig);
    seen.sort_by(|left, right| left.entry.cmp(&right.entry));
    assert_eq!(
        seen,
        vec![
            script_call("Worker", "0.0.0"),
            script_call("Worker", "0.1.0")
        ],
        "each arm is its own entry of the one section"
    );
}
