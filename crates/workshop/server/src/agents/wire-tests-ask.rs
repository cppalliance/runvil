//! How a script's ask result frames: as the operator's message only when
//! both its vendor and its tool id are the ask's, whatever its alias, and
//! as a tool call update for anything else and for a model's ask.

use super::*;

/// The wire kind `event` frames as under the ask tool `ask`.
fn kind_under(event: &Event, ask: Option<&AskTool>) -> serde_json::Value {
    wire(&AgentEventFrame::new(0, None, event, ask).expect("a tool result frames"))["event"]["kind"]
        .clone()
}

#[test]
fn a_script_side_ask_result_frames_as_the_operators_message() {
    let ask = ask();
    let result = tool_result("", "ask", Some(ask_id()), RUNVIL, "two words");
    let frame = wire(
        &AgentEventFrame::new(7, None, &result, Some(&ask)).expect("a script-side ask frames"),
    );
    assert_eq!(
        frame["event"],
        serde_json::json!({
            "kind": "user_message", "section": "chat", "turn": 0, "content": "two words",
        }),
        "a script's ask result is the operator's message, framed on turn 0 without a tool-call id"
    );
}

#[test]
fn a_scripts_ask_is_recognized_by_its_vendor_and_tool_whatever_its_alias() {
    let ask = ask();
    let renamed = tool_result("", "question", Some(ask_id()), RUNVIL, "hi");
    assert_eq!(kind_under(&renamed, Some(&ask)), "user_message");
    let unbound = tool_result("", ASK_TOOL, None, RUNVIL, "hi");
    let acme = tool_result("", "ask", Some(ask_id()), Some("acme"), "hi");
    let vendorless = tool_result("", "ask", Some(ask_id()), None, "hi");
    let elsewhere = AskTool {
        id: ToolId::parse("asker/ask").expect("a valid tool id"),
        ..ask.clone()
    };
    let fetch = ToolId::parse("web/fetch").expect("a valid tool id");
    let other = tool_result("", "fetch", Some(fetch), RUNVIL, "the page");
    for (event, ask, why) in [
        (&unbound, Some(&ask), "a result with no tool is not the ask"),
        (
            &acme,
            Some(&ask),
            "the ask id of another vendor's Pack is not the ask",
        ),
        (
            &vendorless,
            Some(&ask),
            "the ask id with no vendor is not the ask",
        ),
        (&renamed, Some(&elsewhere), "another tool id is not the ask"),
        (&renamed, None, "a Host without the ask frames none"),
        (&other, Some(&ask), "only the ask is the operator's message"),
    ] {
        assert_eq!(kind_under(event, ask), "tool_call_update", "{why}");
    }
}

#[test]
fn a_model_issued_ask_result_stays_a_tool_call_update() {
    let ask = ask();
    let result = tool_result("call_9", "ask", Some(ask_id()), RUNVIL, "two words");
    let frame = wire(
        &AgentEventFrame::new(8, None, &result, Some(&ask)).expect("a model-issued ask frames"),
    );
    assert_eq!(
        frame["event"],
        serde_json::json!({
            "kind": "tool_call_update", "section": "chat", "turn": 2,
            "content": "two words", "tool_call_id": "call_9",
        }),
        "a model's ask answers the model's own tool call, so it stays a tool call update"
    );
}
