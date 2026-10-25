//! The discovery tools' refusals: each malformed discovery call, and a
//! direct call to a Dynamic Set tool, is answered with trusted text that
//! says what was wrong and what to do next, reports a failed tool call,
//! and leaves the loop running.

use super::super::context::Recorder;
use super::super::models_loop::loop_events;
use super::*;

/// Each refused call as its wire name, its arguments, and the text it is
/// answered with, `overlong` being arguments whose pattern is too long.
fn refusals(overlong: &str) -> [(&str, &str, &str); 11] {
    [
        (
            "GetDynamicTools",
            r#"{"toolName":"nope"}"#,
            "GetDynamicTools: no dynamic tool is named \"nope\"; call GetDynamicTools with a \
             pattern or no arguments to find the names",
        ),
        (
            "GetDynamicTools",
            r#"{"toolName":"tools_echo"}"#,
            "GetDynamicTools: \"tools_echo\" is in your tool list, not a dynamic tool; call it \
             directly",
        ),
        (
            "GetDynamicTools",
            r#"{"toolName":7}"#,
            "GetDynamicTools: toolName must be a string, got number",
        ),
        (
            "GetDynamicTools",
            r#"{"pattern":["look"]}"#,
            "GetDynamicTools: pattern must be a string, got array",
        ),
        (
            "GetDynamicTools",
            r#"{"pattern":"("}"#,
            "GetDynamicTools: pattern \"(\" is not a valid regex: unclosed group; fix it, or \
             omit it to list every tool",
        ),
        (
            "GetDynamicTools",
            overlong,
            "GetDynamicTools: pattern is 257 characters, over the limit of 256; shorten it",
        ),
        (
            "CallDynamicTool",
            r#"{"arguments":{}}"#,
            "CallDynamicTool: toolName is required; pass a name from GetDynamicTools",
        ),
        (
            "CallDynamicTool",
            r#"{"toolName":"nope","arguments":{}}"#,
            "CallDynamicTool: no dynamic tool is named \"nope\"; call GetDynamicTools with a \
             pattern or no arguments to find the names",
        ),
        (
            "CallDynamicTool",
            r#"{"toolName":"CallDynamicTool"}"#,
            "CallDynamicTool: \"CallDynamicTool\" is in your tool list, not a dynamic tool; \
             call it directly",
        ),
        (
            "CallDynamicTool",
            r#"{"toolName":"tools_lookup","arguments":"value=x"}"#,
            "CallDynamicTool: arguments must be an object of the tool's arguments, got string",
        ),
        (
            "tools_lookup",
            r#"{"value":"x"}"#,
            "\"tools_lookup\" is a dynamic tool; call it through CallDynamicTool with toolName \
             \"tools_lookup\"",
        ),
    ]
}

#[test]
fn every_refusal_answers_trusted_text_reports_a_failed_call_and_the_loop_goes_on() {
    let overlong = format!(r#"{{"pattern":"{}"}}"#, "a".repeat(257));
    let refused = refusals(&overlong);
    let ids: Vec<String> = (1..=refused.len()).map(|n| format!("call_{n}")).collect();
    let batches: Vec<[(&str, &str, &str); 1]> = refused
        .iter()
        .zip(&ids)
        .map(|((name, args, _), id)| [(id.as_str(), *name, *args)])
        .collect();
    let requested: Vec<&[(&str, &str, &str)]> = batches.iter().map(<[_; 1]>::as_slice).collect();
    let (rig, gateway) = discovering(&requested);
    let texts: Vec<&str> = refused.iter().map(|(_, _, text)| *text).collect();
    assert_eq!(
        tool_turns(&gateway),
        texts,
        "each refusal is the call's text"
    );
    let reported: Vec<String> = refused
        .iter()
        .zip(&ids)
        .map(|((name, _, _), id)| {
            let tool = match *name {
                "GetDynamicTools" => "dynamic/search runvil",
                "CallDynamicTool" => "dynamic/call runvil",
                _ => "tools/lookup -",
            };
            format!("{id} {name}={tool} trusted")
        })
        .collect();
    assert_eq!(
        results(rig.trace()),
        reported,
        "each refusal reports trusted Engine text under the binding its call named"
    );
    assert!(issued_calls(&rig).is_empty(), "no refusal issues a call");
    let recorder = Recorder::default();
    rig.trace().replay(&recorder);
    let (failed, succeeded) = (
        detail::TOOL_CALL_FAILED.to_string(),
        detail::TOOL_CALL_SUCCEEDED.to_string(),
    );
    let outcomes: Vec<String> = loop_events(&recorder)
        .into_iter()
        .filter(|outcome| *outcome == failed || *outcome == succeeded)
        .collect();
    assert_eq!(outcomes, vec![failed; refused.len()]);
}
