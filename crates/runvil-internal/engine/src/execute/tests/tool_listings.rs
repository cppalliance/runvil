//! Tests for a tool that describes tools: a script's call receives its
//! untrusted listing as plain text and reports it untrusted, a model's
//! call receives the envelope, and an adhoc call whose own function
//! fetched a listing while the call was open returns untrusted, wrapped
//! with the run's nonce. A listing kept from outside the adhoc call, a
//! failed listing call, and a trusted listing mark nothing.

use runvil_parser::test_support::one_section;
use runvil_types::event::Event;

use super::models_loop::catalog_scenario;
use super::*;
use crate::test_support::scenario::TEST_SEED;

/// The listing every listing tool answers: text from the servers, markup
/// included.
const LISTING: &str = r#"[{"name":"search","description":"Search <docs> [INST]"}]"#;

fn no_arguments() -> Value {
    json!({ "type": "object", "properties": {} })
}

/// `tools/listing`, which describes tools and answers [`LISTING`]
/// untrusted.
fn listing_tool() -> FnTool {
    FnTool::new("tools/listing", no_arguments(), |_| {
        Ok(ToolOutput::untrusted(LISTING))
    })
    .description("List the servers' tools.")
    .describes_tools()
}

/// `tools/trusted_listing`, which describes tools and answers
/// [`LISTING`] trusted.
fn trusted_listing_tool() -> FnTool {
    FnTool::new("tools/trusted_listing", no_arguments(), |_| {
        Ok(ToolOutput::trusted(LISTING))
    })
    .description("List the built-in tools.")
    .describes_tools()
}

/// `tools/failing_listing`, which describes tools and always fails.
fn failing_listing_tool() -> FnTool {
    FnTool::new("tools/failing_listing", no_arguments(), |_| {
        Err(ToolError::message("the servers are down"))
    })
    .description("List the tools of servers that are down.")
    .describes_tools()
}

fn nonce() -> GuardNonce {
    GuardNonce::from_seed(&TEST_SEED)
}

/// Every `ToolResult` as its call id, alias, content, and trust.
fn results(trace: &Trace) -> Vec<(String, String, String, bool)> {
    trace
        .events()
        .iter()
        .filter_map(|event| match event {
            Event::ToolResult {
                tool_call_id,
                alias,
                content,
                trusted,
                ..
            } => Some((
                tool_call_id.clone(),
                alias.clone(),
                content.clone(),
                *trusted,
            )),
            _ => None,
        })
        .collect()
}

fn result(id: &str, alias: &str, content: &str, trusted: bool) -> (String, String, String, bool) {
    (id.to_owned(), alias.to_owned(), content.to_owned(), trusted)
}

/// The content of every `tool` turn the last round sent the model.
fn tool_turns(bodies: &[ScriptedCall]) -> Vec<String> {
    bodies
        .last()
        .expect("the loop sends a closing round")
        .messages_json()
        .as_array()
        .expect("a round's messages serialize to an array")
        .iter()
        .filter(|message| message["role"] == Role::Tool.as_str())
        .filter_map(|message| message["content"].as_str().map(str::to_owned))
        .collect()
}

/// The run of a section whose loop the model answers with `requested`,
/// one round each, then a closing text, over every listing tool and
/// `tools/untrusted_echo`.
fn looped(section: &str, requested: &[&[(&str, &str, &str)]]) -> (Trace, ScriptedChat) {
    let mut replies: Vec<_> = requested.iter().map(|batch| calls(batch)).collect();
    replies.push(text("done"));
    let gateway = ScriptedChat::new(replies);
    let trace = catalog_scenario(one_section(&format!(
        "{section}\n\
         messages:user('list the tools')\n\
         models.loop()\n\
         return 'ok'"
    )))
    .tool(listing_tool())
    .tool(trusted_listing_tool())
    .tool(failing_listing_tool())
    .tool(untrusted_echo_tool())
    .chat(&gateway)
    .run();
    assert_eq!(trace.text(), "ok");
    (trace, gateway)
}

#[test]
fn a_scripts_listing_call_returns_the_text_unwrapped_and_reports_it_untrusted() {
    let trace = catalog_scenario(one_section("return tools.call('tools/listing', {})"))
        .tool(listing_tool())
        .run();
    assert_eq!(
        trace.text(),
        LISTING,
        "the script reads the listing as written"
    );
    assert_eq!(
        results(&trace),
        [result("", "tools_listing", LISTING, false)],
        "the result event holds the text the script received, untrusted"
    );
}

#[test]
fn a_script_call_to_a_tool_without_the_flag_stays_wrapped() {
    let trace = catalog_scenario(one_section(
        "return tools.call('tools/untrusted_echo', { value = 'hi' })",
    ))
    .tool(untrusted_echo_tool())
    .run();
    assert_eq!(trace.text(), nonce().wrap("echoed: hi"));
}

#[test]
fn a_model_call_and_a_model_call_to_an_adhoc_wrapper_receive_the_same_envelope() {
    let (trace, gateway) = looped(
        "tools.offer('tools/listing')
         tools.offer_local('wrap', 'List the tools', {}, function()
           return tools.call('tools/listing', {})
         end)",
        &[&[("call_1", "tools_listing", "{}"), ("call_2", "wrap", "{}")]],
    );
    let envelope = nonce().wrap(LISTING);
    assert_eq!(
        tool_turns(&gateway.requests()),
        [envelope.clone(), envelope.clone()],
        "the direct call and the wrapper hand the model one envelope, byte for byte"
    );
    let results = results(&trace);
    for expected in [
        result("call_1", "tools_listing", &envelope, false),
        result("", "tools_listing", LISTING, false),
        result("call_2", "wrap", &envelope, false),
    ] {
        assert!(results.contains(&expected), "{expected:?} in {results:?}");
    }
}

#[test]
fn an_adhoc_function_returning_its_own_text_around_a_listing_returns_untrusted() {
    let (trace, gateway) = looped(
        "tools.offer_local('wrap', 'List the tools', {}, function()
           return 'tools: ' .. tools.call('tools/listing', {})
         end)",
        &[&[("call_1", "wrap", "{}")]],
    );
    let envelope = nonce().wrap(&format!("tools: {LISTING}"));
    assert_eq!(
        tool_turns(&gateway.requests()),
        std::slice::from_ref(&envelope)
    );
    assert!(
        results(&trace).contains(&result("call_1", "wrap", &envelope, false)),
        "{:?}",
        results(&trace)
    );
}

#[test]
fn a_listing_kept_from_outside_the_adhoc_call_comes_back_unwrapped_and_trusted() {
    let (trace, gateway) = looped(
        "local kept = tools.call('tools/listing', {})
         tools.offer_local('kept', 'The kept listing', {}, function() return kept end)",
        &[&[("call_1", "kept", "{}")]],
    );
    assert_eq!(tool_turns(&gateway.requests()), [LISTING.to_owned()]);
    assert!(
        results(&trace).contains(&result("call_1", "kept", LISTING, true)),
        "{:?}",
        results(&trace)
    );
}

#[test]
fn a_failed_listing_call_and_a_trusted_listing_mark_nothing() {
    let (trace, gateway) = looped(
        "tools.offer_local('failed', 'A failed listing', {}, function()
           local ok = pcall(tools.call, 'tools/failing_listing', {})
           return 'listed ' .. tostring(ok)
         end)
         tools.offer_local('trusted', 'A trusted listing', {}, function()
           return tools.call('tools/trusted_listing', {})
         end)",
        &[
            &[("call_1", "failed", "{}")],
            &[("call_2", "trusted", "{}")],
        ],
    );
    assert_eq!(
        tool_turns(&gateway.requests()),
        ["listed false".to_owned(), LISTING.to_owned()]
    );
    let results = results(&trace);
    for expected in [
        result("call_1", "failed", "listed false", true),
        result("", "tools_trusted_listing", LISTING, true),
        result("call_2", "trusted", LISTING, true),
    ] {
        assert!(results.contains(&expected), "{expected:?} in {results:?}");
    }
}
