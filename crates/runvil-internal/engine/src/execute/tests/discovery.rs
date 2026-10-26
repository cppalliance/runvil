//! Tests for the discovery tools: a section entry whose Dynamic Set holds
//! a tool advertises `GetDynamicTools` and `CallDynamicTool` last, the
//! model lists, searches, and looks up the Dynamic Set with the first and
//! calls its tools with the second, under its own call id, alone or in a
//! batch, and every discovery call counts. The refusals sit in the
//! `refusals` child.

use runvil_parser::test_support::one_section;
use runvil_types::event::Event;

use super::fixtures::{echo_tool, scoped_tool};
use super::models_loop::{LOOP_TO_TEXT, catalog_scenario, loop_model_name};
use super::*;
use crate::execute::run::{EffectRecord, ToolCallOrigin, ToolCaller};
use crate::test_support::rig::{Rig, RigTrace};
use crate::test_support::scenario::{EXECUTION, TEST_SEED};

#[path = "discovery-refusals.rs"]
mod refusals;

/// The section's offers: `tools/echo` in the Static Set, and
/// `tools/lookup` and the adhoc `grab` in the Dynamic Set, in that order.
const OFFERS: &str = "tools.offer('tools/echo')
     tools.offer_dynamic('tools/lookup')
     tools.offer_dynamic(tools.adhoc({ name = 'grab', description = 'Grab a value', parameters = { value = 'string' } }, function(args)
       return 'grabbed ' .. args.value
     end))";

/// The input schema `grab` and `tools/lookup` share, as JSON text.
const VALUE_SCHEMA: &str =
    r#"{"properties":{"value":{"type":"string"}},"required":["value"],"type":"object"}"#;

/// `tools/lookup` as a search result shows it.
const LOOKUP: &str = r#"{"tool":"tools_lookup","description":"Look up a value."}"#;

/// `grab` as a search result shows it.
const GRAB: &str = r#"{"tool":"grab","description":"Grab a value"}"#;

/// What the model sees of a section that makes [`OFFERS`].
const ADVERTISED: [&str; 3] = ["tools_echo", "GetDynamicTools", "CallDynamicTool"];

/// A Lua helper that joins `pairs` over a table as `key=value`, in walk
/// order.
const WALK: &str = "local function walk(t)
       local out = {}
       for key, value in pairs(t) do out[#out + 1] = key .. '=' .. value end
       return table.concat(out, ' ')
     end";

fn nonce() -> GuardNonce {
    GuardNonce::from_seed(&TEST_SEED)
}

/// The run of one section, `section`, whose model answers with each of
/// `requested`, one round each, then `done`, over `tools/echo`,
/// `tools/lookup`, and `extra`.
fn looped(
    section: &str,
    extra: Vec<FnTool>,
    requested: &[&[(&str, &str, &str)]],
) -> (RigTrace, ScriptedChat) {
    let mut replies: Vec<_> = requested.iter().map(|batch| calls(batch)).collect();
    replies.push(text("done"));
    let gateway = ScriptedChat::new(replies);
    let mut scenario = catalog_scenario(one_section(section))
        .tool(echo_tool())
        .tool(scoped_tool("lookup", "Look up a value.").0);
    for tool in extra {
        scenario = scenario.tool(tool);
    }
    let rig = Rig::run(scenario.chat(&gateway));
    (rig, gateway)
}

/// The run of a section that makes [`OFFERS`], then loops to its closing
/// text, answered with each of `requested`.
fn discovering(requested: &[&[(&str, &str, &str)]]) -> (RigTrace, ScriptedChat) {
    let (rig, gateway) = looped(&format!("{OFFERS}\n{LOOP_TO_TEXT}"), Vec::new(), requested);
    assert_eq!(rig.trace().text(), "done", "the loop runs to its close");
    (rig, gateway)
}

/// The content of every `tool` turn the closing round sent, in order.
fn tool_turns(gateway: &ScriptedChat) -> Vec<String> {
    gateway
        .last_request()
        .expect("the loop sends a closing round")
        .messages_json()
        .as_array()
        .expect("a round's messages serialize to an array")
        .iter()
        .filter(|message| message["role"] == Role::Tool.as_str())
        .filter_map(|message| message["content"].as_str().map(str::to_owned))
        .collect()
}

/// Every `ToolResult` as `<call id> <alias>=<tool> <vendor> <trust>`, `-`
/// standing for none, in order.
fn results(trace: &Trace) -> Vec<String> {
    trace
        .events()
        .iter()
        .filter_map(|event| match event {
            Event::ToolResult {
                tool_call_id,
                alias,
                tool,
                vendor,
                trusted,
                ..
            } => Some(format!(
                "{tool_call_id} {alias}={} {} {}",
                tool.as_ref().map_or("-".to_owned(), ToString::to_string),
                vendor.as_deref().unwrap_or("-"),
                if *trusted { "trusted" } else { "untrusted" }
            )),
            _ => None,
        })
        .collect()
}

/// The tool calls the run issued, in order.
fn issued_calls(rig: &RigTrace) -> Vec<EffectRecord> {
    rig.issued()
        .iter()
        .filter(|record| matches!(record, EffectRecord::ToolCall { .. }))
        .cloned()
        .collect()
}

/// The origin of a model call made in the one section's entry.
fn model_origin() -> ToolCallOrigin {
    ToolCallOrigin::new(EXECUTION, "Only", ToolCaller::Model)
        .with_entry("0.1")
        .with_model(loop_model_name("writer"))
}

/// A model call of the catalog tool `id` under `alias` with `value`.
fn model_call(id: &str, alias: &str, value: &str) -> EffectRecord {
    EffectRecord::ToolCall {
        tool: ToolId::parse(id).expect("valid id"),
        alias: alias.to_owned(),
        args: json!({ "value": value }),
        origin: model_origin(),
    }
}

#[test]
fn a_section_that_offers_dynamically_lists_searches_and_calls_through_the_discovery_tools() {
    let (rig, gateway) = discovering(&[
        &[("call_1", "GetDynamicTools", "{}")],
        &[("call_2", "GetDynamicTools", r#"{"pattern":"look"}"#)],
        &[(
            "call_3",
            "CallDynamicTool",
            r#"{"toolName":"tools_lookup","arguments":{"value":"x"}}"#,
        )],
        &[("call_4", "CallDynamicTool", r#"{"toolName":"nope"}"#)],
    ]);
    for body in gateway.requests() {
        assert_eq!(
            body.tool_names(),
            ADVERTISED,
            "every round sends the same tools"
        );
    }
    let search = &gateway.requests()[0].tools[1];
    assert!(
        search.description().ends_with(" (grab, tools_lookup)"),
        "the search tool names every Dynamic Set tool: {}",
        search.description()
    );
    assert_eq!(
        tool_turns(&gateway),
        [
            nonce().wrap(&format!("[{LOOKUP},{GRAB}]")),
            nonce().wrap(&format!("[{LOOKUP}]")),
            "called lookup with x".to_owned(),
            "CallDynamicTool: no dynamic tool is named \"nope\"; call GetDynamicTools with a \
             pattern or no arguments to find the names"
                .to_owned(),
        ]
    );
    assert_eq!(
        issued_calls(&rig),
        [model_call("tools/lookup", "tools_lookup", "x")]
    );
}

#[test]
fn get_dynamic_tools_searches_by_pattern_and_looks_up_by_name_ignoring_namespace() {
    let (_, gateway) = discovering(&[
        &[("call_1", "GetDynamicTools", r#"{"pattern":"(?i)LOOK"}"#)],
        &[(
            "call_2",
            "GetDynamicTools",
            r#"{"toolName":"grab","pattern":"look","namespace":"any"}"#,
        )],
        &[(
            "call_3",
            "GetDynamicTools",
            r#"{"pattern":"^none$","namespace":"any"}"#,
        )],
    ]);
    let found =
        format!(r#"{{"tool":"grab","description":"Grab a value","inputSchema":{VALUE_SCHEMA}}}"#);
    assert_eq!(
        tool_turns(&gateway),
        [
            nonce().wrap(&format!("[{LOOKUP}]")),
            nonce().wrap(&found),
            nonce().wrap("[]"),
        ],
        "a case-insensitive pattern, a lookup that wins over a pattern, and no match"
    );
}

#[test]
fn a_search_cuts_only_a_description_past_two_hundred_characters_and_a_lookup_keeps_it_whole() {
    let exact = "é".repeat(200);
    let long = format!("{exact}Z");
    let described = |id: &str, description: &str| {
        FnTool::new(id, json!({ "type": "object" }), |_| {
            Ok(ToolOutput::trusted("unused"))
        })
        .description(description)
    };
    let (rig, gateway) = looped(
        &format!(
            "tools.offer_dynamic('tools/exact')\n\
             tools.offer_dynamic('tools/long')\n\
             {LOOP_TO_TEXT}"
        ),
        vec![
            described("tools/exact", &exact),
            described("tools/long", &long),
        ],
        &[
            &[("call_1", "GetDynamicTools", "{}")],
            &[("call_2", "GetDynamicTools", r#"{"toolName":"tools_long"}"#)],
        ],
    );
    assert_eq!(rig.trace().text(), "done");
    assert_eq!(
        tool_turns(&gateway),
        [
            nonce().wrap(&format!(
                r#"[{{"tool":"tools_exact","description":"{exact}"}},{{"tool":"tools_long","description":"{exact}... [truncated]"}}]"#
            )),
            nonce().wrap(&format!(
                r#"{{"tool":"tools_long","description":"{long}","inputSchema":{{"type":"object"}}}}"#
            )),
        ]
    );
}

#[test]
fn a_pattern_of_exactly_two_hundred_fifty_six_multibyte_characters_is_searched_not_refused() {
    let args = format!(r#"{{"pattern":"{}"}}"#, "é".repeat(256));
    let (_, gateway) = discovering(&[&[("call_1", "GetDynamicTools", args.as_str())]]);
    assert_eq!(
        tool_turns(&gateway),
        [nonce().wrap("[]")],
        "the limit counts characters and admits a pattern at it"
    );
}

#[test]
fn call_dynamic_tool_runs_a_catalog_or_adhoc_dynamic_tool_under_its_own_call_id() {
    let (rig, gateway) = discovering(&[
        &[(
            "call_1",
            "CallDynamicTool",
            r#"{"toolName":"tools_lookup","arguments":{"value":"x"},"namespace":"any"}"#,
        )],
        &[(
            "call_2",
            "CallDynamicTool",
            r#"{"toolName":"grab","arguments":{"value":"y"}}"#,
        )],
    ]);
    assert_eq!(tool_turns(&gateway), ["called lookup with x", "grabbed y"]);
    assert_eq!(
        issued_calls(&rig),
        [model_call("tools/lookup", "tools_lookup", "x")],
        "only the catalog tool issues a call, under its own id and wire name"
    );
    assert_eq!(
        results(rig.trace()),
        [
            "call_1 tools_lookup=tools/lookup - trusted",
            "call_2 grab=adhoc/grab - trusted",
        ],
        "each tool reports through its own path under the CallDynamicTool call id"
    );
}

#[test]
fn call_dynamic_tool_without_arguments_calls_its_target_with_an_empty_object() {
    let (rig, _) = discovering(&[&[(
        "call_1",
        "CallDynamicTool",
        r#"{"toolName":"tools_lookup"}"#,
    )]]);
    assert_eq!(
        issued_calls(&rig),
        [EffectRecord::ToolCall {
            tool: ToolId::parse("tools/lookup").expect("valid id"),
            alias: "tools_lookup".to_owned(),
            args: json!({}),
            origin: model_origin(),
        }],
        "a missing arguments key reaches the target as an empty object"
    );
}

#[test]
fn a_batch_that_holds_discovery_calls_answers_every_call_in_the_models_order() {
    let (rig, gateway) = discovering(&[&[
        ("call_1", "tools_echo", r#"{"value":"a"}"#),
        ("call_2", "GetDynamicTools", r#"{"pattern":"grab"}"#),
        (
            "call_3",
            "CallDynamicTool",
            r#"{"toolName":"tools_lookup","arguments":{"value":"z"}}"#,
        ),
        ("call_4", "tools_echo", r#"{"value":"b"}"#),
    ]]);
    assert_eq!(
        tool_turns(&gateway),
        [
            "echoed: a".to_owned(),
            nonce().wrap(&format!("[{GRAB}]")),
            "called lookup with z".to_owned(),
            "echoed: b".to_owned(),
        ]
    );
    assert_eq!(
        issued_calls(&rig),
        [
            model_call("tools/echo", "tools_echo", "a"),
            model_call("tools/lookup", "tools_lookup", "z"),
            model_call("tools/echo", "tools_echo", "b"),
        ]
    );
}

#[test]
fn a_listing_is_wrapped_and_untrusted_and_each_discovery_call_reports_the_runvil_vendor() {
    let (rig, _) = discovering(&[&[
        ("call_1", "GetDynamicTools", "{}"),
        ("call_2", "CallDynamicTool", "{}"),
    ]]);
    let requested: Vec<(String, Option<String>, Option<String>)> = rig
        .trace()
        .events()
        .iter()
        .find_map(|event| match event {
            Event::AssistantToolCalls { calls, .. } => Some(
                calls
                    .iter()
                    .map(|call| {
                        (
                            call.name.clone(),
                            call.tool.as_ref().map(ToString::to_string),
                            call.vendor.clone(),
                        )
                    })
                    .collect(),
            ),
            _ => None,
        })
        .expect("the round reports its batch");
    let named = |name: &str, tool: &str| {
        (
            name.to_owned(),
            Some(tool.to_owned()),
            Some("runvil".to_owned()),
        )
    };
    assert_eq!(
        requested,
        [
            named("GetDynamicTools", "dynamic/search"),
            named("CallDynamicTool", "dynamic/call"),
        ]
    );
    assert_eq!(
        results(rig.trace()),
        [
            "call_1 GetDynamicTools=dynamic/search runvil untrusted",
            "call_2 CallDynamicTool=dynamic/call runvil trusted",
        ],
        "the listing is untrusted and the refusal trusted"
    );
}

#[test]
fn a_search_and_every_refusal_count_under_the_dynamic_pack_and_a_reach_under_its_target() {
    let (rig, _) = looped(
        &format!(
            "{OFFERS}\n\
             messages:user('find a tool')\n\
             models.loop()\n\
             {WALK}\n\
             return tools.stats.total .. ' | ' .. walk(tools.stats.packs) .. ' | ' .. walk(tools.stats.tools)"
        ),
        Vec::new(),
        &[
            &[("call_1", "GetDynamicTools", "{}")],
            &[("call_2", "GetDynamicTools", r#"{"pattern":7}"#)],
            &[("call_3", "CallDynamicTool", r#"{"toolName":"nope"}"#)],
            &[("call_4", "tools_lookup", r#"{"value":"x"}"#)],
            &[(
                "call_5",
                "CallDynamicTool",
                r#"{"toolName":"tools_lookup","arguments":{"value":"x"}}"#,
            )],
            &[(
                "call_6",
                "CallDynamicTool",
                r#"{"toolName":"grab","arguments":{"value":"y"}}"#,
            )],
        ],
    );
    assert_eq!(
        rig.trace().text(),
        "6 | adhoc=1 dynamic=4 tools=1 | adhoc/grab=1 tools/echo=0 tools/lookup=1",
        "two searches and two refusals count under packs.dynamic and two reaches under \
         their targets, and tools leaves out both discovery ids"
    );
}

#[test]
fn once_the_discovery_tools_render_packs_starts_with_dynamic_and_tools_with_each_dynamic_tool() {
    let (rig, _) = looped(
        &format!(
            "{OFFERS}\n\
             messages:user('echo')\n\
             models.loop()\n\
             {WALK}\n\
             return walk(tools.stats.packs) .. ' | ' .. walk(tools.stats.tools)"
        ),
        Vec::new(),
        &[&[("call_1", "tools_echo", r#"{"value":"a"}"#)]],
    );
    assert_eq!(
        rig.trace().text(),
        "adhoc=0 dynamic=0 tools=1 | adhoc/grab=0 tools/echo=1 tools/lookup=0"
    );
}

#[test]
fn a_scripts_call_of_a_discovery_tools_id_fails_as_an_unknown_id() {
    let (rig, _) = looped(
        &format!(
            "{OFFERS}\n\
             messages:user('go')\n\
             models.loop()\n\
             return tools.call('dynamic/call', {{ toolName = 'tools_lookup', arguments = {{ value = 'x' }} }})"
        ),
        Vec::new(),
        &[],
    );
    match rig.trace().error().inner() {
        Error::UnboundToolCall { name, .. } => assert_eq!(name, "dynamic/call"),
        other => panic!("expected the unbound-tool error, got {other:?}"),
    }
    assert!(issued_calls(&rig).is_empty(), "nothing was issued");
}
