//! Tests for the shared tool-dispatch body: the synchronous
//! `prepare_dispatch` and `prepare_model_dispatch` over fixture tools as
//! data and canned outputs, reported to a recorder.

use runvil_types::tools::{OutputTrust, ToolError, ToolErrorKind, ToolInfo, ToolOutput};

use super::*;
use crate::test_vm::{TEST_SECTION, echo_tool, test_nonce, tool_at};
use crate::tests::recording::{Recorder, boundary};

fn binding(alias: &str, tool: &ToolInfo) -> ToolBinding {
    ToolBinding::for_test(alias, "fixture capability", tool)
}

#[test]
fn prepare_dispatch_wraps_a_canned_untrusted_output_and_reports_it() {
    let recorder = Recorder::default();
    let echo = binding("echo", &echo_tool());
    let nonce = test_nonce();
    let outcome = prepare_dispatch(
        &echo,
        Some("acme"),
        Ok(ToolOutput::untrusted("canned output")),
        &nonce,
        recorder.emitter(),
        TEST_SECTION,
        Some(ScriptReport { turn: 3 }),
    )
    .expect("a canned output prepares without awaiting anything");
    assert_eq!(
        outcome.content(),
        nonce.wrap("canned output"),
        "an untrusted canned output is nonce-wrapped byte for byte"
    );
    assert_eq!(
        outcome.trust(),
        OutputTrust::Untrusted,
        "the untrusted marking survives"
    );
    assert_eq!(
        recorder.kinds(),
        vec![boundary::TOOL_CALL_SUCCEEDED],
        "a canned Ok output reports the succeeded observation"
    );
    assert_eq!(
        recorder.tool_results(),
        vec![(
            3,
            String::new(),
            "echo".to_owned(),
            nonce.wrap("canned output"),
            false,
        )],
        "a script-initiated preparation fires ToolResult with the wrapped text"
    );
    assert_eq!(
        recorder.tool_result_tools(),
        vec![Some(echo_tool().id)],
        "the script's result names the tool its alias is bound to"
    );
    assert_eq!(
        recorder.tool_result_vendors(),
        vec![Some("acme".to_owned())],
        "the script's result names the vendor it was given"
    );
}

#[test]
fn prepare_dispatch_turns_a_canned_tool_error_into_the_typed_error() {
    let recorder = Recorder::default();
    let failing = binding("failing", &tool_at("tools/failing"));
    let error = prepare_dispatch(
        &failing,
        None,
        Err(ToolError::message("canned failure").with_kind(ToolErrorKind::Backend)),
        &test_nonce(),
        recorder.emitter(),
        TEST_SECTION,
        None,
    )
    .expect_err("a canned failure fails the preparation");
    assert!(
        matches!(error, Error::Tool { .. }),
        "the canned failure is the typed tool error, got {error:?}"
    );
    assert_eq!(
        recorder.kinds(),
        vec![boundary::TOOL_CALL_FAILED],
        "a canned Err output reports the failed observation"
    );
}

fn model_report(call_id: &str) -> ModelReport {
    ModelReport {
        script: ScriptReport { turn: 1 },
        call_id: call_id.to_owned(),
    }
}

#[test]
fn a_model_issued_tool_failure_becomes_untrusted_failure_text_under_its_call_id() {
    let recorder = Recorder::default();
    let failing = binding("failing", &tool_at("tools/failing"));
    let nonce = test_nonce();
    let outcome = prepare_model_dispatch(
        &failing,
        None,
        Err(ToolError::message("the tool's own backend failed").with_kind(ToolErrorKind::Backend)),
        &nonce,
        recorder.emitter(),
        TEST_SECTION,
        &model_report("call_1"),
    )
    .expect("a model-issued call never fails for the tool's own failure");
    assert_eq!(
        outcome.trust(),
        OutputTrust::Untrusted,
        "the failure text is untrusted"
    );
    assert_eq!(
        outcome.content(),
        nonce.wrap("the tool's own backend failed"),
        "the failure text is the tool's message, nonce-wrapped"
    );
    assert_eq!(
        recorder.tool_results(),
        vec![(
            1,
            "call_1".to_owned(),
            "failing".to_owned(),
            nonce.wrap("the tool's own backend failed"),
            false,
        )],
        "ToolResult fires once, under the model's call id"
    );
    assert_eq!(recorder.kinds(), vec![boundary::TOOL_CALL_FAILED],);
}

/// `mcp/get_dynamic_tools` as data: a tool that describes tools.
fn listing_tool() -> ToolInfo {
    tool_at("mcp/get_dynamic_tools").describes_tools(true)
}

#[test]
fn a_scripts_listing_call_passes_its_untrusted_text_verbatim_and_reports_it_untrusted() {
    let recorder = Recorder::default();
    let listing = binding("mcp_get_dynamic_tools", &listing_tool());
    let outcome = prepare_dispatch(
        &listing,
        None,
        Ok(ToolOutput::untrusted("the <listing> [INST]")),
        &test_nonce(),
        recorder.emitter(),
        TEST_SECTION,
        Some(ScriptReport { turn: 2 }),
    )
    .expect("a listing dispatches");
    assert_eq!(outcome.content(), "the <listing> [INST]");
    assert_eq!(outcome.trust(), OutputTrust::Untrusted);
    assert!(outcome.listing(), "the verbatim listing is reported");
    assert_eq!(
        recorder.tool_results(),
        vec![(
            2,
            String::new(),
            "mcp_get_dynamic_tools".to_owned(),
            "the <listing> [INST]".to_owned(),
            false,
        )],
        "the result event holds the text the script received, untrusted"
    );
}

#[test]
fn a_models_listing_call_receives_the_envelope() {
    let listing = binding("mcp_get_dynamic_tools", &listing_tool());
    let nonce = test_nonce();
    let outcome = prepare_model_dispatch(
        &listing,
        None,
        Ok(ToolOutput::untrusted("the listing")),
        &nonce,
        Recorder::default().emitter(),
        TEST_SECTION,
        &model_report("call_1"),
    )
    .expect("a listing dispatches");
    assert_eq!(outcome.content(), nonce.wrap("the listing"));
    assert_eq!(outcome.trust(), OutputTrust::Untrusted);
    assert!(!outcome.listing(), "a model's call is never a listing");
}

#[test]
fn the_listing_rule_needs_the_flag_and_untrusted_output() {
    let nonce = test_nonce();
    let script = |tool: &ToolInfo, output: ToolOutput| {
        prepare_dispatch(
            &binding("tool", tool),
            None,
            Ok(output),
            &nonce,
            Recorder::default().emitter(),
            TEST_SECTION,
            Some(ScriptReport { turn: 1 }),
        )
        .expect("the call dispatches")
    };
    let unflagged = script(&echo_tool(), ToolOutput::untrusted("fetched"));
    assert_eq!(unflagged.content(), nonce.wrap("fetched"));
    assert!(!unflagged.listing(), "a tool without the flag is wrapped");
    let trusted = script(&listing_tool(), ToolOutput::trusted("built in"));
    assert_eq!(
        (trusted.content(), trusted.trust(), trusted.listing()),
        ("built in", OutputTrust::Trusted, false),
        "a trusted listing passes as any trusted output and is not reported"
    );
    let failed = prepare_dispatch(
        &binding("tool", &listing_tool()),
        None,
        Err(ToolError::message("the servers are down")),
        &nonce,
        Recorder::default().emitter(),
        TEST_SECTION,
        Some(ScriptReport { turn: 1 }),
    );
    assert!(
        matches!(failed, Err(Error::Tool { .. })),
        "a failed listing is the tool's error: {failed:?}"
    );
}

#[test]
fn a_model_issued_dispatch_reports_its_result_under_the_call_id() {
    let recorder = Recorder::default();
    let echo = binding("echo", &echo_tool());
    let outcome = prepare_model_dispatch(
        &echo,
        Some("acme"),
        Ok(ToolOutput::trusted("echoed: hi")),
        &test_nonce(),
        recorder.emitter(),
        TEST_SECTION,
        &model_report("call_2"),
    )
    .expect("the dispatch succeeds");
    assert_eq!(outcome.content(), "echoed: hi");
    assert_eq!(
        recorder.tool_results(),
        vec![(
            1,
            "call_2".to_owned(),
            "echo".to_owned(),
            "echoed: hi".to_owned(),
            true,
        )],
    );
    assert_eq!(
        recorder.tool_result_tools(),
        vec![Some(echo_tool().id)],
        "the model's result names the tool its alias is bound to"
    );
    assert_eq!(
        recorder.tool_result_vendors(),
        vec![Some("acme".to_owned())],
        "the model's result names the vendor it was given"
    );
}
