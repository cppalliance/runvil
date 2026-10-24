//! Tests for an adhoc tool that wraps a catalog tool: it renders the
//! catalog tool's description and schema unchanged, and a model's call to
//! it gives the model the same result text as a call to the catalog tool.

use runvil_types::event::Event;

use super::super::fixtures::untrusted_echo_tool;
use super::*;

/// The wrapper of the untrusted catalog tool `t`, offered as `search`,
/// whose function returns `tools.call(t, args)`.
const WRAPPER: &str = "local t = tools.get('tools/untrusted_echo')
     tools.offer(tools.adhoc({ name = 'search', description = t.description, parameters = t.parameters }, function(args)
       return tools.call(t, args)
     end))";

/// One loop over a section that runs `offer`, in which the model calls
/// `name` with `value = 'hi'` and then replies.
fn called(offer: &str, name: &str) -> (Trace, Vec<ScriptedCall>) {
    let gateway = ScriptedChat::new(vec![
        calls(&[("call_1", name, r#"{"value":"hi"}"#)]),
        text("done"),
    ]);
    let source = one_section(&format!(
        "{offer}
         messages:user('search')
         models.loop()
         return 'ok'"
    ));
    let trace = loop_scenario(source)
        .offer("untrusted_echo", untrusted_echo_tool())
        .chat(&gateway)
        .run();
    assert_eq!(trace.text(), "ok");
    (trace, gateway.requests())
}

#[test]
fn a_wrapper_renders_the_catalog_tools_description_and_schema_unchanged() {
    let (_, direct) = called("tools.offer('tools/untrusted_echo')", "untrusted_echo");
    let (_, through) = called(WRAPPER, "search");
    let (catalog, wrapper) = (&direct[0].tools[0], &through[0].tools[0]);
    assert_eq!(wrapper.name(), "search");
    assert_eq!(
        wrapper.description(),
        catalog.description(),
        "the description is the catalog text, never in the envelope"
    );
    assert_eq!(
        serde_json::to_string(wrapper.parameters()).expect("a schema serializes"),
        serde_json::to_string(catalog.parameters()).expect("a schema serializes"),
        "the schema is the same text"
    );
}

#[test]
fn a_model_call_to_the_wrapper_gives_the_model_the_catalog_calls_text_byte_for_byte() {
    let (_, direct) = called("tools.offer('tools/untrusted_echo')", "untrusted_echo");
    let (trace, through) = called(WRAPPER, "search");
    let text = last_tool_turn_content(&direct);
    assert!(
        text.contains("<untrusted_input_") && text.contains("echoed: hi"),
        "the catalog tool's result is enveloped: {text}"
    );
    assert_eq!(last_tool_turn_content(&through), text);
    let result = trace
        .events()
        .iter()
        .find_map(|event| match event {
            Event::ToolResult {
                tool_call_id,
                tool,
                vendor,
                trusted,
                ..
            } if tool_call_id == "call_1" => Some((
                tool.as_ref().map(ToString::to_string),
                vendor.clone(),
                *trusted,
            )),
            _ => None,
        })
        .expect("the wrapper's call reports its result");
    assert_eq!(
        result,
        (Some("adhoc/search".to_owned()), None, true),
        "the result names the adhoc tool's id and no vendor, and is trusted because \
         the author's function produced the text"
    );
}
