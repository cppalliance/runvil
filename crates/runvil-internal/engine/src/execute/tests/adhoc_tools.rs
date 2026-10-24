//! Tests for adhoc tools defined with `tools.adhoc` and offered with
//! `tools.offer`: the definition rules run end to end, and the
//! `models.loop` shim's adhoc-tool rounds are driven at prompt level, so a
//! model-issued call to an adhoc tool runs its handler inside the block
//! coroutine - store calls included - and its trusted result is sent back
//! to the model verbatim. `jump` is refused only while a handler runs,
//! even through a saved reference. An adhoc tool answers a model's call
//! only when offered. The refusal of an offer once the section's tool list
//! is rendered sits in `rendered_tools`.

use runvil_parser::test_support::{one_section, prompt};

use super::context::Recorder;
use super::fixtures::scoped_tool;
use super::models_loop::loop_scenario;
use super::tool_call_arm::{ToolRecorder, tool_result_lines};
use super::*;

/// The `grab` adhoc tool definition and offer the loop tests open with,
/// followed by one loop over a single user message; `handler` is the Lua
/// body of the handler function, given `args`.
fn grab_loop(handler: &str) -> String {
    one_section(&format!(
        "tools.offer(tools.adhoc({{ name = 'grab', description = 'Grab a value', parameters = {{ value = 'string' }} }}, function(args)
           {handler}
         end))
         messages:user('Use the tool.')
         models.loop()
         return messages[#messages].content"
    ))
}

/// The section the jump tests' `## Only` jumps to, returning a fixed
/// marker when a jump lands there.
const OTHER: (&str, &str) = ("Other", "return 'jumped'");

#[test]
fn adhoc_tool_handler_result_returns_to_the_model() {
    let gateway = ScriptedChat::new(vec![
        calls(&[("call_1", "grab", "{\"value\":\"hi\"}")]),
        text("final answer"),
    ]);
    let trace = loop_scenario(grab_loop("return 'got ' .. args.value"))
        .chat(&gateway)
        .run();
    assert_eq!(trace.text(), "final answer");

    let bodies = gateway.requests();
    let function = &bodies[0].tools[0];
    assert_eq!(function.name(), "grab");
    assert_eq!(function.description(), "Grab a value");
    assert_eq!(
        function.parameters(),
        &json!({
            "type": "object",
            "properties": { "value": { "type": "string" } },
            "required": ["value"]
        })
    );
    // The handler's trusted return reaches the model verbatim (no guard wrap).
    assert_eq!(last_tool_turn_content(&bodies), "got hi");
}

#[test]
fn adhoc_tool_multiple_calls_in_one_response_all_run() {
    let gateway = ScriptedChat::new(vec![
        calls(&[
            ("c1", "grab", "{\"value\":\"a\"}"),
            ("c2", "grab", "{\"value\":\"b\"}"),
        ]),
        text("final answer"),
    ]);
    let source = one_section(
        "local calls = {}
         tools.offer(tools.adhoc({ name = 'grab', description = 'Grab a value', parameters = { value = 'string' } }, function(args)
           calls[#calls + 1] = args.value
           return 'ok ' .. args.value
         end))
         messages:user('Use the tool.')
         models.loop()
         return table.concat(calls, ',') .. '|' .. messages[#messages].content",
    );
    let trace = loop_scenario(source).chat(&gateway).run();
    assert_eq!(
        trace.text(),
        "a,b|final answer",
        "both calls in the one response must run, in order"
    );

    let bodies = gateway.requests();
    let tool_turns = bodies[1]
        .messages
        .iter()
        .filter(|m| m.role() == Role::Tool)
        .count();
    assert_eq!(
        tool_turns, 2,
        "both handler results must go back: {bodies:?}"
    );
}

#[test]
fn adhoc_tool_handler_error_surfaces_as_a_tool_failure() {
    let gateway = ScriptedChat::new(vec![
        calls(&[("call_1", "grab", "{\"value\":\"hi\"}")]),
        text("unreachable"),
    ]);
    let trace = loop_scenario(grab_loop("error('handler exploded')"))
        .chat(&gateway)
        .run();
    let error = trace.error();
    assert!(
        error.to_string().contains("handler exploded"),
        "the handler's error must surface: {error}"
    );
    let recorder = ToolRecorder::default();
    trace.replay(&recorder);
    let lines = recorder.lines();
    assert!(
        lines.contains(&format!("Only: {}", detail::TOOL_CALL_FAILED)),
        "the failed handler must be observed as a tool-call failure: {lines:?}"
    );
    assert!(
        tool_result_lines(&recorder).is_empty(),
        "call_1 reports no ToolResult, so the model never sees the failure: {lines:?}"
    );
    assert_eq!(
        gateway.call_count(),
        1,
        "the author's own program failing ends the loop before another round"
    );
}

#[test]
fn cancel_during_a_looping_adhoc_tool_handler_returns_promptly() {
    // The round's answer hands the model's call to the looping handler in
    // the next step. A cancel landing before that step starts ends the run
    // the same way, and a hang fails through nextest's timeout.
    let gateway = ScriptedChat::new(vec![
        calls(&[("call_1", "grab", "{\"value\":\"hi\"}")]),
        text("unreachable"),
    ]);
    let trace = loop_scenario(grab_loop("while true do end"))
        .chat(&gateway)
        .cancel_when(|effect| matches!(effect, Effect::Chat { .. }))
        .run();
    assert!(
        matches!(trace.result(), RunResult::Cancelled),
        "expected the cancelled run, got {:?}",
        trace.result()
    );
}

#[test]
fn a_loop_handler_writes_and_reads_the_store_and_the_model_gets_the_text() {
    let gateway = ScriptedChat::new(vec![
        calls(&[("call_1", "grab", "{\"value\":\"hi\"}")]),
        text("final answer"),
    ]);
    let source = grab_loop(
        "store.write('grab.txt', 'kept ' .. args.value)
         return store.read('grab.txt')",
    );
    let trace = loop_scenario(source).chat(&gateway).run();
    assert_eq!(trace.text(), "final answer");
    assert_eq!(
        last_tool_turn_content(&gateway.requests()),
        "kept hi",
        "the text the handler read back from the store reaches the model"
    );
}

#[test]
fn a_handler_that_calls_jump_fails_the_run() {
    let gateway = ScriptedChat::new(vec![
        calls(&[("call_1", "grab", "{\"value\":\"hi\"}")]),
        text("unreachable"),
    ]);
    let only = "tools.offer(tools.adhoc({ name = 'grab', description = 'Grab a value', parameters = { value = 'string' } }, function(args)
                  jump('## Other')
                end))
                messages:user('Use the tool.')
                models.loop()
                return 'no jump'";
    let trace = loop_scenario(prompt("", &[("Only", only), OTHER]))
        .chat(&gateway)
        .run();
    let message = trace.error().to_string();
    assert!(
        message.contains(JUMP_REFUSAL),
        "the handler's call is refused with the guard's message: {message}"
    );
}

/// The start of the message `jump` fails with inside an adhoc tool handler.
const JUMP_REFUSAL: &str = "jump is unavailable inside an adhoc tool handler";

/// The `grab` definition whose handler calls a `jump` reference the
/// block saved before defining it.
const SAVED_JUMP_GRAB: &str = "local j = jump\n\
     tools.offer(tools.adhoc({ name = 'grab', description = 'Grab a value', parameters = { value = 'string' } }, function(args)\n\
       j('## Other')\n\
     end))\n";

#[test]
fn a_saved_jump_reference_is_refused_in_a_handler_the_script_calls() {
    let only = format!(
        "{SAVED_JUMP_GRAB}tools.call('adhoc/grab', {{ value = 'hi' }})\n\
         return 'no jump'"
    );
    let trace = loop_scenario(prompt("", &[("Only", &only), OTHER])).run();
    let message = trace.error().to_string();
    assert!(
        message.contains(JUMP_REFUSAL),
        "the refusal fails the run, and no jump lands: {message}"
    );
}

#[test]
fn a_saved_jump_reference_is_refused_in_a_handler_the_model_calls() {
    let gateway = ScriptedChat::new(vec![
        calls(&[("call_1", "grab", "{\"value\":\"hi\"}")]),
        text("unreachable"),
    ]);
    let only = format!(
        "{SAVED_JUMP_GRAB}messages:user('Use the tool.')\n\
         models.loop()\n\
         return 'no jump'"
    );
    let trace = loop_scenario(prompt("", &[("Only", &only), OTHER]))
        .chat(&gateway)
        .run();
    let message = trace.error().to_string();
    assert!(
        message.contains(JUMP_REFUSAL),
        "the refusal fails the run, and no jump lands: {message}"
    );
}

#[test]
fn a_caller_that_catches_the_jump_refusal_continues_the_block() {
    let only = format!(
        "{SAVED_JUMP_GRAB}local ok, err = pcall(tools.call, 'adhoc/grab', {{ value = 'hi' }})\n\
         assert(not ok, 'the refusal reaches the call site')\n\
         assert(tostring(err):find('{JUMP_REFUSAL}', 1, true), tostring(err))\n\
         return 'no jump'"
    );
    let trace = loop_scenario(prompt("", &[("Only", &only), OTHER])).run();
    assert_eq!(trace.text(), "no jump");
}

#[test]
fn jump_stays_refused_in_an_outer_handler_after_an_inner_one_returns() {
    let only = format!(
        "tools.offer(tools.adhoc({{ name = 'inner', description = 'Inner' }}, function() return 'inner' end))
         tools.offer(tools.adhoc({{ name = 'outer', description = 'Outer' }}, function()
           local inner = tools.call('adhoc/inner', {{}})
           local ok, err = pcall(jump, '## Other')
           assert(not ok, 'jump is still refused in the outer handler')
           assert(tostring(err):find('{JUMP_REFUSAL}', 1, true), tostring(err))
           return inner
         end))
         assert(tools.call('adhoc/outer', {{}}) == 'inner', 'the outer handler returns')
         jump('## Other')"
    );
    let trace = loop_scenario(prompt("", &[("Only", &only), OTHER])).run();
    assert_eq!(trace.text(), "jumped");
}

#[test]
fn jump_works_in_the_same_block_after_the_loop_returns() {
    let gateway = ScriptedChat::new(vec![
        calls(&[("call_1", "grab", "{\"value\":\"hi\"}")]),
        text("final answer"),
    ]);
    let only = "tools.offer(tools.adhoc({ name = 'grab', description = 'Grab a value', parameters = { value = 'string' } }, function(args)
                  return 'got ' .. args.value
                end))
                messages:user('Use the tool.')
                models.loop()
                jump('## Other')";
    let trace = loop_scenario(prompt("", &[("Only", only), OTHER]))
        .chat(&gateway)
        .run();
    assert_eq!(trace.text(), "jumped");
}

#[test]
fn a_handler_returning_a_table_raises_and_is_observed_as_a_failure() {
    let gateway = ScriptedChat::new(vec![
        calls(&[("call_1", "grab", "{\"value\":\"hi\"}")]),
        text("unreachable"),
    ]);
    let trace = loop_scenario(grab_loop("return { args.value }"))
        .chat(&gateway)
        .run();
    let error = trace.error();
    assert!(
        error
            .to_string()
            .contains("cannot return a table as a result"),
        "the scalar-return rule names the rejected type: {error}"
    );
    let recorder = Recorder::default();
    trace.replay(&recorder);
    assert!(
        recorder
            .events()
            .contains(&("Only".to_string(), detail::TOOL_CALL_FAILED.to_string())),
        "the rejected return is observed as a tool-call failure"
    );
}

#[test]
fn a_model_call_to_an_offered_catalog_name_never_reaches_an_unoffered_adhoc_tool_of_that_name() {
    let gateway = ScriptedChat::new(vec![
        calls(&[("call_1", "tools_concrete", "{}")]),
        text("final answer"),
    ]);
    let (tool, called) = scoped_tool("concrete", "Concrete description.");
    let source = one_section(
        "tools.adhoc({ name = 'tools_concrete', description = 'Local concrete' }, function() return 'local' end)
         tools.offer('tools/concrete')
         messages:user('Use the tool.')
         models.loop()
         return messages[#messages].content",
    );
    let trace = loop_scenario(source)
        .offer("tools_concrete", tool)
        .chat(&gateway)
        .run();
    assert_eq!(trace.text(), "final answer");
    let bodies = gateway.requests();
    assert_eq!(bodies[0].tools.len(), 1);
    assert_eq!(bodies[0].tools[0].description(), "Concrete description.");
    assert_eq!(
        called.load(Ordering::SeqCst),
        1,
        "the catalog tool answers the model's call"
    );
    assert_ne!(last_tool_turn_content(&bodies), "local");
}

#[test]
fn an_adhoc_tool_may_take_an_offered_wire_name_and_wins_in_scope() {
    let gateway = ScriptedChat::new(vec![
        calls(&[("call_1", "tools_concrete", "{}")]),
        text("final answer"),
    ]);
    let (tool, called) = scoped_tool("concrete", "Concrete description.");
    let source = one_section(
        "tools.offer('tools/concrete')
         tools.offer(tools.adhoc({ name = 'tools_concrete', description = 'Local concrete' }, function() return 'local' end))
         messages:user('Use the tool.')
         models.loop()
         return messages[#messages].content",
    );
    let trace = loop_scenario(source)
        .offer("tools_concrete", tool)
        .chat(&gateway)
        .run();
    assert_eq!(trace.text(), "final answer");
    let bodies = gateway.requests();
    assert_eq!(
        bodies[0].tools.len(),
        1,
        "the model sees one tool under the name"
    );
    assert_eq!(bodies[0].tools[0].description(), "Local concrete");
    assert_eq!(last_tool_turn_content(&bodies), "local");
    assert_eq!(
        called.load(Ordering::SeqCst),
        0,
        "the offered catalog tool is never called"
    );
}

#[test]
fn an_adhoc_tool_name_cannot_be_defined_twice() {
    let source = one_section(
        "tools.offer(tools.adhoc({ name = 'grab', description = 'First grab' }, function() return 'first' end))
         tools.offer(tools.adhoc({ name = 'grab', description = 'Second grab' }, function() return 'second' end))",
    );
    let trace = Scenario::new(source).run();
    let error = trace.error();
    assert!(
        error
            .to_string()
            .contains("tools.adhoc name \"grab\" is already registered"),
        "the error must identify the duplicate adhoc name: {error}"
    );
}

#[path = "adhoc_tools-wrappers.rs"]
mod wrappers;
