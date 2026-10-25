//! Adhoc tools: a handler runs inside the calling block coroutine
//! with no leaf work of its own and reports its trusted result under
//! the model's call id or none; it uses the store, raises to a script
//! caller that catches it, calls another adhoc tool innermost first,
//! and dispatches a bound tool as one leaf request, whose failure
//! raises kind `tool` in the handler and whose untrusted wrapper
//! survives. A model-issued call resolves in the section's frozen sets,
//! so those cases run one loop round first.

use runvil_parser::test_support::prompt;

use super::*;
use crate::execute::tests::fixtures::{echo_tool, failing_tool, untrusted_echo_tool};

/// The `grab` adhoc tool definition every adhoc-tool test opens with,
/// offered to the section.
const ADD_ADHOC_GRAB: &str = "tools.offer(tools.adhoc({ name = 'grab', description = 'Grab a \
     value', parameters = { value = 'string' } }, function(args) return 'got ' .. args.value \
     end))\n";

/// One loop round the model answers with text, which freezes the
/// section's sets so a model-issued call can resolve in them.
const RENDER: &str = "messages:user('go')\nmodels.loop()\n";

/// The arm scenario over `source` with one text round for [`RENDER`].
fn rendered_scenario(source: impl Into<String>) -> (Scenario, ScriptedChat) {
    let gateway = ScriptedChat::new(vec![text("rendered")]);
    (arm_scenario(source), gateway)
}

/// How many tool-call effects `trace` issued.
fn tool_call_effects(trace: &Trace) -> usize {
    trace
        .effects()
        .iter()
        .filter(|(effect, _)| matches!(effect, EffectRecord::ToolCall { .. }))
        .count()
}

#[test]
fn an_adhoc_tool_call_issues_no_leaf_work() {
    let source = one_section(&format!(
        "{ADD_ADHOC_GRAB}\
         local out = tools.call('adhoc/grab', {{ value = 'hi' }})\n\
         return out .. '|' .. tostring(tools.stats.tools['adhoc/grab'])"
    ));
    let trace = arm_scenario(source).run();
    assert_eq!(
        trace.text(),
        "got hi|1",
        "the handler's text resumes and the call counts"
    );
    assert_eq!(
        trace.effects().len(),
        0,
        "an adhoc tool call spawns no leaf request"
    );
    let lines = tool_lines(&trace).lines();
    assert!(
        lines
            .iter()
            .any(|line| line == &format!("Only: {}", detail::TOOL_CALL_SUCCEEDED)),
        "the adhoc call is observed as a succeeded tool call: {lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line == "Only: tool_result id= alias=grab trust=Trusted content=got hi"),
        "a script-initiated adhoc call reports its trusted result under no call id: {lines:?}"
    );
}

#[test]
fn a_script_calls_an_unoffered_adhoc_tool_by_id_or_object_before_and_after_the_render() {
    let (scenario, gateway) = rendered_scenario(one_section(&format!(
        "local grab = tools.adhoc({{ name = 'grab', description = 'Grab a value', \
           parameters = {{ value = 'string' }} }}, function(args) return 'got ' .. args.value end)\n\
         local before = tools.call(grab, {{ value = 'a' }})\n\
         {RENDER}\
         local after = tools.call('adhoc/grab', {{ value = 'b' }})\n\
         return before .. '|' .. after .. '|' .. tostring(tools.stats.tools['adhoc/grab'])"
    )));
    let trace = scenario.chat(&gateway).run();
    assert_eq!(
        trace.text(),
        "got a|got b|2",
        "an adhoc tool needs no offer to be called, and the render leaves it callable"
    );
    assert!(
        gateway.requests()[0].tools.is_empty(),
        "an unoffered adhoc tool is never rendered"
    );
}

#[test]
fn a_model_issued_adhoc_tool_call_reports_under_its_call_id() {
    let (scenario, gateway) = rendered_scenario(one_section(&format!(
        "{ADD_ADHOC_GRAB}{RENDER}\
         return tools.call_as_model('call_7', 'grab', {{ value = 'hi' }})"
    )));
    let trace = scenario.chat(&gateway).run();
    assert_eq!(trace.text(), "got hi");
    assert_eq!(
        tool_call_effects(&trace),
        0,
        "a model-issued adhoc call spawns no leaf request"
    );
    let lines = tool_lines(&trace).lines();
    assert!(
        lines
            .iter()
            .any(|line| line
                == "Only: tool_result id=call_7 alias=grab trust=Trusted content=got hi"),
        "ToolResult fires under the model's call id: {lines:?}"
    );
}

#[test]
fn a_script_called_adhoc_handler_uses_the_store() {
    let source = one_section(
        "tools.offer(tools.adhoc({ name = 'grab', description = 'Grab a value', parameters = { value = 'string' } }, function(args)
           store.write('grab.txt', 'kept ' .. args.value)
           return store.read('grab.txt')
         end))
         return tools.call('adhoc/grab', { value = 'hi' })",
    );
    let trace = arm_scenario(source).run();
    assert_eq!(trace.text(), "kept hi");
    assert_eq!(
        tool_result_lines(&tool_lines(&trace)),
        vec!["Only: tool_result id= alias=grab trust=Trusted content=kept hi".to_owned()]
    );
}

#[test]
fn a_script_caller_catches_the_handlers_own_error_and_jump_is_restored() {
    let only = "local raised = { kind = 'custom', message = 'handler exploded' }
                tools.offer(tools.adhoc({ name = 'grab', description = 'Grab a value' }, function() error(raised) end))
                local ok, err = pcall(tools.call, 'adhoc/grab', {})
                assert(not ok, 'the handler raise reaches the call site')
                assert(err == raised and err.kind == 'custom', 'the caller catches the handler table')
                jump('## Other')";
    let source = prompt("", &[("Only", only), ("Other", "return 'jumped'")]);
    let trace = arm_scenario(source).run();
    assert_eq!(
        trace.text(),
        "jumped",
        "after the caught failure the block's `jump` transfers"
    );
    let lines = tool_lines(&trace).lines();
    assert!(
        lines
            .iter()
            .any(|line| line == &format!("Only: {}", detail::TOOL_CALL_FAILED)),
        "the raising handler is observed as a failed tool call: {lines:?}"
    );
    assert!(
        !lines.iter().any(|line| line.contains("tool_result")),
        "a raising handler reports no ToolResult: {lines:?}"
    );
}

#[test]
fn a_nested_adhoc_call_reports_both_results_innermost_first() {
    let (scenario, gateway) = rendered_scenario(one_section(&format!(
        "tools.offer(tools.adhoc({{ name = 'inner', description = 'Inner', parameters = {{ value = 'string' }} }}, function(args)
           return 'inner ' .. args.value
         end))
         tools.offer(tools.adhoc({{ name = 'outer', description = 'Outer', parameters = {{ value = 'string' }} }}, function(args)
           return 'outer ' .. tools.call('adhoc/inner', {{ value = args.value }})
         end))
         {RENDER}return tools.call_as_model('call_1', 'outer', {{ value = 'hi' }})",
    )));
    let trace = scenario.chat(&gateway).run();
    assert_eq!(trace.text(), "outer inner hi");
    assert_eq!(
        tool_result_lines(&tool_lines(&trace)),
        vec![
            "Only: tool_result id= alias=inner trust=Trusted content=inner hi".to_owned(),
            "Only: tool_result id=call_1 alias=outer trust=Trusted content=outer inner hi"
                .to_owned(),
        ],
        "each call reports under its own id, the inner call closing first"
    );
}

#[test]
fn a_handler_dispatches_a_bound_tool_as_one_leaf_request() {
    let source = one_section(
        "tools.offer('tools/echo')
         tools.offer(tools.adhoc({ name = 'grab', description = 'Grab a value', parameters = { value = 'string' } }, function(args)
           return tools.call('tools/echo', { value = args.value })
         end))
         local out = tools.call('adhoc/grab', { value = 'hi' })
         return out .. '|' .. tostring(tools.stats.tools['adhoc/grab']) .. '|' .. tostring(tools.stats.tools['tools/echo'])",
    );
    let trace = arm_scenario(source).offer("echo", echo_tool()).run();
    assert_eq!(trace.text(), "echoed: hi|1|1", "both calls count once");
    assert_eq!(
        trace.effects().len(),
        1,
        "the echo call is the one leaf request; the adhoc call issues none"
    );
}

#[test]
fn a_bound_failure_inside_a_model_issued_adhoc_call_raises_kind_tool_in_the_handler() {
    let (scenario, gateway) = rendered_scenario(one_section(&format!(
        "tools.offer('tools/failing')
         tools.offer(tools.adhoc({{ name = 'grab', description = 'Grab a value' }}, function()
           local ok, err = pcall(tools.call, 'tools/failing', {{}})
           assert(not ok, 'the bound failure raises inside the handler')
           return err.kind
         end))
         {RENDER}return tools.call_as_model('call_1', 'grab', {{}})",
    )));
    let trace = scenario.offer("fail", failing_tool()).chat(&gateway).run();
    assert_eq!(
        trace.text(),
        "tool",
        "the handler's call is a script call, raising kind `tool`"
    );
    assert_eq!(
        tool_result_lines(&tool_lines(&trace)),
        vec!["Only: tool_result id=call_1 alias=grab trust=Trusted content=tool".to_owned()],
        "the inner failure fires no ToolResult; only the adhoc call reports under call_1"
    );
}

#[test]
fn a_handlers_text_stays_trusted_and_a_bound_tools_wrapper_survives() {
    let source = one_section(
        "tools.offer('tools/untrusted_echo')
         tools.offer(tools.adhoc({ name = 'plain', description = 'Plain' }, function() return 'plain text' end))
         tools.offer(tools.adhoc({ name = 'grab', description = 'Grab a value', parameters = { value = 'string' } }, function(args)
           return tools.call('tools/untrusted_echo', { value = args.value })
         end))
         return tools.call('adhoc/plain', {}) .. '||' .. tools.call('adhoc/grab', { value = 'hi' })",
    );
    let trace = arm_scenario(source)
        .offer("echo", untrusted_echo_tool())
        .run();
    let (plain, wrapped) = trace
        .text()
        .split_once("||")
        .expect("the block joins both results");
    assert_eq!(plain, "plain text", "a handler's own text gets no wrapper");
    assert!(
        wrapped.contains("<untrusted_input_")
            && wrapped.contains("</untrusted_input_")
            && wrapped.contains("echoed: hi"),
        "the bound tool's untrusted output keeps its wrapper through the handler: {wrapped}"
    );
    let lines = tool_result_lines(&tool_lines(&trace));
    assert!(
        lines.contains(
            &"Only: tool_result id= alias=plain trust=Trusted content=plain text".to_owned()
        ),
        "the plain handler reports trusted: {lines:?}"
    );
    assert!(
        lines.iter().any(|line| {
            line.starts_with("Only: tool_result id= alias=grab trust=Trusted content=")
                && line.contains("<untrusted_input_")
        }),
        "the wrapping handler reports trusted, its text still wrapped: {lines:?}"
    );
}
