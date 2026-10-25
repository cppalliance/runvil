//! The loop's scope gate: a model call naming anything outside its
//! section's frozen sets fails with `OutOfScopeToolCall`, which hints
//! offered but unscoped only when the name is the wire name of a tool the
//! run offers. A Dynamic Set name passes the gate and is answered with
//! text.

use super::*;
use crate::execute::tests::fixtures::{echo_tool, scoped_tool};
use crate::test_support::rig::Rig;

/// A run of the loop section that offers `tools/scoped` (wire name
/// `scoped`) and runs `lua` before its loop. The run catalog also holds
/// `tools/global_tool` (wire name `global_tool`), which only `lua` may
/// offer.
fn scoped_loop(lua: &str) -> Scenario {
    let source = one_section(&format!("tools.offer('tools/scoped')\n{lua}{LOOP_TO_TEXT}"));
    loop_scenario(source)
        .offer("scoped", scoped_tool("scoped", "A scoped tool.").0)
        .offer(
            "global_tool",
            scoped_tool("global_tool", "A global tool.").0,
        )
}

/// The dynamic offers of `tools/global_tool` and the adhoc `grab`, whose
/// handler calls `tools/scoped`.
const DYNAMIC_OFFERS: &str = "tools.offer_dynamic('tools/global_tool')\n\
                              tools.offer_dynamic(tools.adhoc({ name = 'grab', description = 'Grab' }, function()\n\
                                return tools.call('tools/scoped', { value = 'grab' })\n\
                              end))\n";

#[test]
fn model_calling_an_offered_but_unscoped_tool_is_a_hard_error() {
    // The loop's scope gate: a model call naming a tool the run offers but
    // the section never offered fails with OutOfScopeToolCall holding the
    // offered-but-unscoped hint.
    let gateway = ScriptedChat::new(vec![calls(&[(
        "call_1",
        "global_tool",
        "{\"value\":\"x\"}",
    )])]);
    let trace = scoped_loop("").chat(&gateway).run();
    let error = trace.error().inner();
    match error {
        Error::OutOfScopeToolCall {
            name,
            global_exists,
            in_scope,
        } => {
            assert_eq!(name, "global_tool");
            assert!(*global_exists, "the name is an offered tool's wire name");
            assert_eq!(in_scope, &["scoped".to_owned()]);
        }
        other => panic!("expected OutOfScopeToolCall, got {other:?}"),
    }
    assert!(
        error
            .to_string()
            .ends_with("[\"scoped\"] (a catalog tool that was not offered in this section)"),
        "error message must hint offered-but-unscoped: {error}"
    );
    assert_eq!(gateway.call_count(), 1, "the rejected round is the last");
}

#[test]
fn model_calling_an_advertised_tool_by_canonical_id_gets_no_not_offered_suffix() {
    // A model reaches a tool only by the wire name its round advertised.
    // The canonical id of the advertised `scoped` is out of scope, but it
    // names no tool the section left unoffered, so the error drops the
    // offered-but-unscoped hint.
    let gateway = ScriptedChat::new(vec![calls(&[(
        "call_1",
        "tools/scoped",
        "{\"value\":\"x\"}",
    )])]);
    let trace = scoped_loop("").chat(&gateway).run();
    let error = trace.error().inner();
    match error {
        Error::OutOfScopeToolCall {
            name,
            global_exists,
            in_scope,
        } => {
            assert_eq!(name, "tools/scoped");
            assert!(!*global_exists, "an id is no offered tool's wire name");
            assert_eq!(in_scope, &["scoped".to_owned()]);
        }
        other => panic!("expected OutOfScopeToolCall, got {other:?}"),
    }
    let message = error.to_string();
    assert!(
        message.ends_with("[\"scoped\"]"),
        "error message must end with the in-scope list: {message}"
    );
    assert!(
        !message.contains("not offered in this section"),
        "an advertised tool's id must not hint offered-but-unscoped: {message}"
    );
}

#[test]
fn model_calling_a_defined_but_unoffered_adhoc_tool_gets_no_not_offered_suffix() {
    // An adhoc tool the section defined but never offered is in no
    // catalog, so the error drops the hint that names a catalog tool.
    let gateway = ScriptedChat::new(vec![calls(&[("call_1", "grab", "{}")])]);
    let trace = scoped_loop(
        "tools.adhoc({ name = 'grab', description = 'Grab' }, function() return '' end)\n",
    )
    .chat(&gateway)
    .run();
    let error = trace.error().inner();
    match error {
        Error::OutOfScopeToolCall {
            name,
            global_exists,
            in_scope,
        } => {
            assert_eq!(name, "grab");
            assert!(
                !*global_exists,
                "an adhoc tool's name is no catalog tool's wire name"
            );
            assert_eq!(in_scope, &["scoped".to_owned()]);
        }
        other => panic!("expected OutOfScopeToolCall, got {other:?}"),
    }
    assert!(
        !error.to_string().contains("not offered in this section"),
        "an unoffered adhoc tool must not hint a catalog tool: {error}"
    );
}

#[test]
fn model_calling_a_dynamic_set_tool_by_its_wire_name_gets_text_and_issues_no_call() {
    // The gate lets a Dynamic Set name through, alone or in a batch, and
    // the call is answered with text pointing to CallDynamicTool, so the
    // loop goes on. Reaching the adhoc tool would run its handler, whose
    // script call would issue a tool call too.
    for (name, args) in [("global_tool", r#"{"value":"x"}"#), ("grab", "{}")] {
        let refusal = format!(
            "{name:?} is a dynamic tool; call it through CallDynamicTool with toolName {name:?}"
        );
        let alone = vec![("call_1", name, args)];
        let batched = vec![
            ("call_1", "scoped", r#"{"value":"a"}"#),
            ("call_2", name, args),
        ];
        for batch in [alone, batched] {
            let gateway = ScriptedChat::new(vec![calls(&batch), text("done")]);
            let rig = Rig::run(scoped_loop(DYNAMIC_OFFERS).chat(&gateway));
            assert_eq!(rig.trace().text(), "done", "{name}: the loop goes on");
            let last = gateway
                .last_request()
                .expect("the loop sends a closing round")
                .messages_json();
            let turns: Vec<&str> = last
                .as_array()
                .expect("a round's messages serialize to an array")
                .iter()
                .filter(|message| message["role"] == "tool")
                .filter_map(|message| message["content"].as_str())
                .collect();
            assert_eq!(turns.last(), Some(&refusal.as_str()), "{name}: {turns:?}");
            let issued: Vec<String> = rig
                .issued()
                .iter()
                .filter_map(|record| match record {
                    EffectRecord::ToolCall { tool, .. } => Some(tool.to_string()),
                    _ => None,
                })
                .collect();
            let expected: &[&str] = if batch.len() == 1 {
                &[]
            } else {
                &["tools/scoped"]
            };
            assert_eq!(issued, expected, "a direct call to {name} issues no call");
        }
    }
}

#[test]
fn with_dynamic_offers_a_name_outside_both_sets_fails_listing_only_the_static_set() {
    // The in-scope list names the Static Set, the two discovery tools
    // included, and none of the Dynamic Set's tools.
    let gateway = ScriptedChat::new(vec![calls(&[("call_1", "rogue", "{}")])]);
    let trace = scoped_loop(DYNAMIC_OFFERS).chat(&gateway).run();
    match trace.error().inner() {
        Error::OutOfScopeToolCall {
            name,
            global_exists,
            in_scope,
        } => {
            assert_eq!(name, "rogue");
            assert!(!*global_exists, "the name is no offered tool");
            assert_eq!(in_scope, &["scoped", "GetDynamicTools", "CallDynamicTool"]);
        }
        other => panic!("expected OutOfScopeToolCall, got {other:?}"),
    }
    assert_eq!(gateway.call_count(), 1, "the rejected round is the last");
}

#[test]
fn model_calling_pure_unknown_tool_is_a_hard_error() {
    let gateway = ScriptedChat::new(vec![calls(&[(
        "call_1",
        "nonexistent",
        "{\"value\":\"x\"}",
    )])]);
    let source = one_section(&format!("tools.offer('tools/echo')\n{LOOP_TO_TEXT}"));
    let trace = loop_scenario(source)
        .offer("echo", echo_tool())
        .chat(&gateway)
        .run();
    let error = trace.error().inner();
    match error {
        Error::OutOfScopeToolCall {
            name,
            global_exists,
            in_scope,
        } => {
            assert_eq!(name, "nonexistent");
            assert!(!*global_exists, "the name is no offered tool");
            assert_eq!(in_scope, &["echo".to_owned()]);
        }
        other => panic!("expected OutOfScopeToolCall, got {other:?}"),
    }
    assert!(
        !error.to_string().contains("not offered in this section"),
        "pure unknown must not hint offered-but-unscoped: {error}"
    );
}

#[test]
fn the_gate_reads_the_sections_frozen_sets_after_a_handler_runs_model_rounds_of_its_own() {
    let gateway = ScriptedChat::new(vec![
        calls(&[("call_1", "grab", "{}")]),
        text("aside"),
        text("helped"),
        calls(&[("call_2", "echo", r#"{"value":"after"}"#)]),
        text("done"),
    ]);
    let only = "tools.offer('tools/echo')\n\
                tools.offer(tools.adhoc({ name = 'grab', description = 'Grab' }, function()\n\
                  return models.infer('aside') .. '+' .. call('## Helper')\n\
                end))\n\
                messages:user('go')\n\
                models.loop()\n\
                return messages[3].content .. '|' .. messages[5].content";
    let helper = "messages:user('help')\nmodels.loop()\nreturn messages[2].content";
    let trace = loop_scenario(prompt("", &[("Only", only), ("Helper", helper)]))
        .offer("echo", echo_tool())
        .chat(&gateway)
        .run();
    assert_eq!(
        trace.text(),
        "aside+helped|echoed: after",
        "the handler's infer and its called section's round run, then the echo call passes"
    );
    let batches: Vec<Vec<(String, Option<String>)>> = trace
        .events()
        .iter()
        .filter_map(|event| match event {
            Event::AssistantToolCalls { calls, .. } => Some(
                calls
                    .iter()
                    .map(|call| {
                        (
                            call.name.clone(),
                            call.tool.as_ref().map(ToString::to_string),
                        )
                    })
                    .collect(),
            ),
            _ => None,
        })
        .collect();
    assert_eq!(
        batches,
        [
            vec![("grab".to_owned(), Some("adhoc/grab".to_owned()))],
            vec![("echo".to_owned(), Some("tools/echo".to_owned()))],
        ],
        "each batch's event names the tool the section's frozen sets hold"
    );
}
