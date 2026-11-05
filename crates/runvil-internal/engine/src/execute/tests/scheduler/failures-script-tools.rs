//! Script-initiated `tools.call` dispatch on the scheduler by canonical id:
//! string and table results, tool objects, unbound names, offered tools
//! outside the section scope, cancellation of a slow call, untrusted output
//! wrapping, and the model install around a call.

use super::*;
use crate::execute::tests::fixtures::{
    echo_tool, model_only_tool, structured_tool, untrusted_echo_tool,
};

#[test]
fn a_script_tools_call_dispatches_and_resumes_as_a_string() {
    // The whole script path in one pass: the shim yields, the scheduler
    // dispatches the bound tool, the plain binding resumes as a Lua
    // string, and the counts land in the same `tools.stats` table the
    // prose loop feeds.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        # ToolCall\n\n\
        ## Only\n\n\
        ```lua\n\
        tools.offer('tools/echo')\n\
        local out = tools.call('tools/echo', { value = 'hi' })\n\
        return out .. '|' .. tostring(tools.stats.tools['tools/echo'])\n\
        ```\n";
    let trace = scheduler_scenario(md).offer("echo", echo_tool()).run();
    assert_eq!(trace.text(), "echoed: hi|1");
}

#[test]
fn a_script_tools_call_with_a_tool_object_dispatches_its_binding() {
    // The object form: `tools.get` reads the tool object, and passing it as
    // the leading argument dispatches the tool it names, identically to the
    // bare id string.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        # ToolCall\n\n\
        ## Only\n\n\
        ```lua\n\
        tools.offer('tools/echo')\n\
        local echo = tools.get('tools/echo')\n\
        assert(type(echo) == 'userdata', 'tools.get reads a tool object')\n\
        return tools.call(echo, { value = 'hi' })\n\
        ```\n";
    let trace = scheduler_scenario(md).offer("echo", echo_tool()).run();
    assert_eq!(trace.text(), "echoed: hi");
}

#[test]
fn a_script_tools_call_with_an_unbound_name_lists_the_offered_ids() {
    // Script-initiated resolution runs against the run's whole offering,
    // so the unknown-name error lists every offered id, not the section's
    // frozen sets.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        # ToolCall\n\n\
        ## Only\n\n\
        ```lua\ntools.offer('tools/echo')\nreturn tools.call('tools/missing', {})\n```\n";
    let trace = scheduler_scenario(md).offer("echo", echo_tool()).run();
    assert_eq!(
        trace.error(),
        &crate::test_support::placed(
            crate::error::unbound_tool("tools/missing", &["tools/echo".to_owned()]),
            13
        )
    );
}

#[test]
fn a_script_tools_call_reaches_an_offered_tool_outside_the_section_scope() {
    // A catalog tool the section never offered (no `tools.offer`) still
    // dispatches for a script: the frozen sets shape what the model is
    // offered, and the author's own code is not the model.
    // The count lands in the same shared map `tools.stats` reads.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        # ToolCall\n\n\
        ## Only\n\n\
        ```lua\n\
        local out = tools.call('tools/echo', { value = 'hi' })\n\
        return out .. '|' .. tostring(tools.stats.tools['tools/echo'])\n\
        ```\n";
    let trace = scheduler_scenario(md).offer("echo", echo_tool()).run();
    assert_eq!(trace.text(), "echoed: hi|1");
}

#[test]
fn a_script_tools_call_of_a_model_only_tool_is_refused_naming_it() {
    // The Pack marked the tool model-only, so the catalog still names it
    // and a section may offer it, while a script's call fails at
    // dispatch: only a model receives arguments that suit the schema a
    // render answered.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        # ToolCall\n\n\
        ## Only\n\n\
        ```lua\n\
        tools.offer('tools/echo')\n\
        return tools.call('tools/replace', { value = 'hi' })\n\
        ```\n";
    let trace = scheduler_scenario(md)
        .offer("echo", echo_tool())
        .offer("replace", model_only_tool())
        .run();
    assert_eq!(
        trace.error(),
        &crate::test_support::placed(
            crate::error::model_only_tool("tools/replace", "tools/replace"),
            13
        )
    );
}

#[test]
fn a_script_pcall_catches_the_model_only_refusal_and_the_run_goes_on() {
    // The refusal is a Lua error like any other failed `tools.call`, so
    // an author can catch it and offer the tool instead.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        # ToolCall\n\n\
        ## Only\n\n\
        ```lua\n\
        tools.offer('tools/replace')\n\
        local ok = pcall(tools.call, 'tools/replace', { value = 'hi' })\n\
        assert(not ok, 'the model-only call is refused')\n\
        return 'caught'\n\
        ```\n";
    let trace = scheduler_scenario(md)
        .offer("replace", model_only_tool())
        .run();
    assert_eq!(trace.text(), "caught", "the run continues after the catch");
}

#[test]
fn a_script_tools_call_of_a_model_only_tool_by_its_object_is_refused_the_same_way() {
    // The object form resolves through the same lookup as the id, so the
    // refusal cannot be skipped by holding the tool object.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        # ToolCall\n\n\
        ## Only\n\n\
        ```lua\n\
        tools.offer('tools/replace')\n\
        local tool = tools.get('tools/replace')\n\
        return tools.call(tool, { value = 'hi' })\n\
        ```\n";
    let trace = scheduler_scenario(md)
        .offer("replace", model_only_tool())
        .run();
    let error = trace.error();
    assert_eq!(error.code, ErrorCode::ModelOnlyTool, "got {error:?}");
    assert!(
        error.message.contains("(tools/replace) is model-only"),
        "the refusal names the catalog id: {error}"
    );
}

#[test]
fn a_model_only_tool_is_still_offerable_alone_and_with_its_pack() {
    // The flag forbids the call, never the offer: a prompt offers the
    // tool and the model calls it.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        # ToolCall\n\n\
        ## Only\n\n\
        ```lua\n\
        tools.offer('tools/replace')\n\
        tools.offer('tools')\n\
        return 'offered'\n\
        ```\n";
    let trace = scheduler_scenario(md)
        .offer("replace", model_only_tool())
        .run();
    assert_eq!(trace.text(), "offered");
}

#[test]
fn cancellation_cancels_a_slow_script_tools_call() {
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        # ToolCall\n\n\
        ## Only\n\n\
        ```lua\ntools.offer('test/tools/slow')\nreturn tools.call('test/tools/slow', {})\n```\n";
    let started = Arc::new(AtomicUsize::new(0));
    let dispatched = Arc::clone(&started);
    let trace = scheduler_scenario(md)
        .offer("slow", slow_tool())
        .hold(move |effect| {
            let slow = calls_slow_tool(effect);
            if slow {
                dispatched.fetch_add(1, Ordering::SeqCst);
            }
            slow
        })
        .cancel_when(calls_slow_tool)
        .run();

    assert!(
        matches!(trace.outcome(), RunOutcome::Cancelled),
        "canceling a suspended tools.call must cancel the run, got {:?}",
        trace.outcome()
    );
    assert_eq!(
        started.load(Ordering::SeqCst),
        1,
        "the cancellation must land after the tool call was in flight"
    );
}

#[test]
fn an_untrusted_script_tools_call_result_is_nonce_wrapped() {
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        # ToolCall\n\n\
        ## Only\n\n\
        ```lua\n\
        tools.offer('tools/untrusted_echo')\n\
        return tools.call('tools/untrusted_echo', { value = 'hi' })\n\
        ```\n";
    let trace = scheduler_scenario(md)
        .offer("fetch", untrusted_echo_tool())
        .run();
    let out = trace.text();
    assert!(
        out.contains("<untrusted_input_") && out.contains("</untrusted_input_"),
        "the script must receive the nonce-wrapped envelope, got: {out}"
    );
    assert!(
        out.contains("echoed: hi"),
        "the wrapped block must still include the tool output, got: {out}"
    );
}

#[test]
fn a_structured_binding_resumes_as_a_lua_table() {
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        # ToolCall\n\n\
        ## Only\n\n\
        ```lua\n\
        tools.offer('tools/structured')\n\
        local r = tools.call('tools/structured', {})\n\
        return r.text .. '|' .. tostring(#r.images)\n\
        ```\n";
    let trace = scheduler_scenario(md)
        .offer(
            "form",
            structured_tool("{\"text\":\"typed\",\"images\":[]}", true),
        )
        .run();
    assert_eq!(trace.text(), "typed|0");
}

#[test]
fn invalid_json_from_a_structured_tool_is_a_tool_error() {
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        # ToolCall\n\n\
        ## Only\n\n\
        ```lua\ntools.offer('tools/structured')\nreturn tools.call('tools/structured', {})\n```\n";
    let trace = scheduler_scenario(md)
        .offer("form", structured_tool("not json", true))
        .run();
    let error = trace.error();
    assert_eq!(error.code, ErrorCode::ToolOther, "got {error:?}");
    assert!(
        error.message.contains("returned invalid JSON"),
        "the tool error names the invalid JSON, got: {error}"
    );
}

#[test]
fn an_untrusted_structured_output_is_wrapped_before_classification() {
    // The untrusted nonce wrap precedes the structured JSON parse, so an
    // untrusted binding's valid JSON still fails the call: this ordering is
    // what restricts structured output to trusted tools. If classification
    // ever ran on the raw output, this test would resume a table and fail.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        # ToolCall\n\n\
        ## Only\n\n\
        ```lua\ntools.offer('tools/structured')\nreturn tools.call('tools/structured', {})\n```\n";
    let trace = scheduler_scenario(md)
        .offer("form", structured_tool("{\"text\":\"typed\"}", false))
        .run();
    let error = trace.error();
    assert_eq!(error.code, ErrorCode::ToolOther, "got {error:?}");
    assert!(
        error.message.contains("returned invalid JSON"),
        "the wrap must precede the parse, got: {error}"
    );
}

#[test]
fn a_script_tools_call_before_infer_keeps_the_model_install() {
    // The section's first-call install is shared between the first script
    // dispatch and the model resolution: a script `tools.call` that runs
    // first must not swallow the install a later `models.infer` relies on.
    let gateway = ScriptedChat::new(vec![text("prose answer")]);
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        # ToolCall\n\n\
        ## Only\n\n\
        ```lua\ntools.offer('tools/echo')\ntools.call('tools/echo', { value = 'x' })\n```\n\n\
        Say something.\n\n\
        ```lua\nreturn models.infer(prose)\n```\n";
    let trace = scheduler_scenario(md)
        .offer("echo", echo_tool())
        .chat(&gateway)
        .run();
    assert_eq!(trace.text(), "prose answer");
}

#[test]
fn a_first_tool_call_sets_sys_model_to_the_model_models_use_selected() {
    // The first tool call takes the section's current model, so the switch
    // `models.use` made before it is what `sys.model` names.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        # ToolCall\n\n\
        ## Only\n\n\
        ```lua\n\
        models.use('strong')\n\
        tools.call('tools/echo', { value = 'y' })\n\
        return sys.model\n\
        ```\n";
    let mut models = ModelSet::default();
    models.push(test_binding(
        "default",
        "A general model for tests",
        "test-model",
    ));
    models.push(test_binding("strong", "A second model", "other-model"));
    let trace = Scenario::new(md)
        .models(models)
        .offer("echo", echo_tool())
        .run();
    assert_eq!(trace.text(), "other-model");
}
#[test]
fn a_document_prompt_without_tools_call_is_unaffected() {
    // Bindings installed, shim present, `tools.call` never called: the
    // section runs exactly as before the dispatch arm existed.
    let md = "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n\
        # ToolCall\n\n\
        ## Only\n\n\
        ```lua\ntools.offer('tools/echo')\nreturn 'plain'\n```\n";
    let trace = scheduler_scenario(md).offer("echo", echo_tool()).run();
    assert_eq!(trace.text(), "plain");
}
