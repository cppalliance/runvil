//! Exit rules: the section walk's (fall-through, explicit return, the
//! generic result, H1-only prompts, the version gate, a caller code ending
//! the run as an internal error, cross-section store persistence) and the
//! `models.loop` shim's (the terminal reply, the model's clean empty exit
//! after tool work, and the empty rounds that raise `empty_model_reply`).

use runvil_prompt::test_support::{PromptSource, one_section, prompt};
use runvil_types::error::{ErrorCode, ErrorFact, SourceLoc};

use super::context::Recorder;
use super::fixtures::echo_tool;
use super::models_loop::{LOOP_TO_TEXT, loop_events, loop_scenario};
use super::*;
use crate::test_support::scenario::Trace;

/// The backend's finish reason a model failure names, when it names one.
fn finish_reason(error: &runvil_types::error::Error) -> Option<&str> {
    error
        .facts
        .get(&ErrorFact::FinishReason)
        .and_then(Value::as_str)
}

#[test]
fn falls_through_to_next_section() {
    let source = prompt(
        "",
        &[("First", "local x = 1"), ("Second", "return \"second\"")],
    );
    assert_eq!(Scenario::new(source).run().text(), "second");
}

#[test]
fn explicit_return_stops_fall_through() {
    let source = prompt(
        "",
        &[
            ("First", "return \"first\""),
            ("Second", "return \"unreached\""),
        ],
    );
    assert_eq!(Scenario::new(source).run().text(), "first");
}

#[test]
fn generic_result_when_nothing_produced() {
    let trace = Scenario::new(one_section("local x = 1")).run();
    assert_eq!(trace.text(), "done");
}

#[test]
fn sys_id_increments_per_section() {
    // First section files nothing and falls through; second returns its id:
    // entry 2 of the root chain (entry 0 is the H1 pass, present or not).
    let source = prompt(
        "",
        &[
            ("First", "local x = 1"),
            ("Second", "return tostring(sys.id)"),
        ],
    );
    assert_eq!(Scenario::new(source).run().text(), "0.2");
}

// --- H1-only prompts (no ## sections) ---

#[test]
fn h1_only_lua_return() {
    let source = PromptSource::new().title("Title").h1("return \"hello\"");
    assert_eq!(Scenario::new(source.build()).run().text(), "hello");
}

#[test]
fn h1_only_lua_no_return() {
    let source = PromptSource::new().title("Title").h1("local x = 1");
    assert_eq!(Scenario::new(source.build()).run().text(), "done");
}

// --- Version gate at the top of `run` ---

/// The `## Only` prompt returning `ran`, under the frontmatter version
/// line `version`, or with none when `version` is empty.
fn ran_under(version: &str) -> String {
    one_section("return \"ran\"").replacen("runvil: 0\n", version, 1)
}

#[test]
fn supported_major_zero_proceeds() {
    // A `runvil: 0` prompt clears the gate and runs to completion.
    let trace = Scenario::new(ran_under("runvil: 0\n")).run();
    assert_eq!(trace.text(), "ran");
}

#[test]
fn unsupported_major_one_is_refused() {
    // Major 1 is unsupported: the gate refuses it and names the
    // declared version rather than silently degrading to major 0.
    let trace = Scenario::new(ran_under("runvil: 1\n")).run();
    let error = trace.error();
    assert_eq!(
        (error.code, error.located_message()),
        (
            ErrorCode::UnsupportedVersion,
            "unsupported runvil version: 1 (this build supports major 0) (at prompt:4)".to_owned()
        ),
        "the gate refuses major 1 by name, on its `runvil:` line"
    );
}

#[test]
fn unsupported_major_is_refused() {
    // Any major other than 0 is refused, never silently degraded to major 0.
    let trace = Scenario::new(ran_under("runvil: 2\n")).run();
    let error = trace.error();
    assert_eq!(
        (error.code, error.message.as_str()),
        (
            ErrorCode::UnsupportedVersion,
            "unsupported runvil version: 2 (this build supports major 0)"
        ),
        "the gate refuses major 2 by name"
    );
    let line = std::num::NonZeroU32::new(4).expect("4 is non-zero");
    assert_eq!(
        error.at,
        Some(SourceLoc::line("prompt", line)),
        "the refusal points at the `runvil:` line"
    );
    assert_eq!(
        error.facts.get(&ErrorFact::Name).and_then(Value::as_str),
        Some("t"),
        "the refusal names the prompt"
    );
}

#[test]
fn unsupported_major_refusal_gets_the_prompt_context() {
    // The gate's refusal goes through the parser's prompt context, so its
    // prose leads with the `runvil:` line, as a parse failure's does.
    let trace = Scenario::new(ran_under("runvil: 3\n")).run();
    let message = "unsupported runvil version: 3 (this build supports major 0)";
    let mut expected = runvil_types::error::Error::new(
        ErrorCode::UnsupportedVersion,
        message,
        format!(
            "prompt:4: {message}. {}",
            ErrorCode::UnsupportedVersion.prose()
        ),
    );
    let line = std::num::NonZeroU32::new(4).expect("4 is non-zero");
    expected.at = Some(SourceLoc::line("prompt", line));
    expected.facts.insert(ErrorFact::Name, Value::from("t"));
    assert_eq!(
        trace.error(),
        &expected,
        "the whole refusal: its prose, its line, and the prompt's name"
    );
}

#[test]
fn missing_version_is_not_a_runvil_prompt() {
    // No `runvil:` key: not our prompt, so `run` declines with a Parse
    // error rather than executing it.
    let trace = Scenario::new(ran_under("")).run();
    let error = trace.error();
    assert_eq!(error.code, ErrorCode::StructureInvalid, "got {error:?}");
    assert_eq!(error.at, None, "a key the prompt lacks has no line");
    assert_eq!(
        error
            .facts
            .get(&ErrorFact::Name)
            .and_then(serde_json::Value::as_str),
        Some("t"),
        "the error reports the parsed prompt's frontmatter name"
    );
    assert!(
        error.to_string().contains("not a runvil prompt"),
        "the Parse message must name the missing version, got: {error}"
    );
    assert_eq!(
        error.located_message(),
        "not a runvil prompt: no runvil version (prompt t)",
        "the located message names the prompt the failure's location named"
    );
}

// --- A caller code ends the run as an internal error ---

#[test]
fn a_raised_error_table_with_a_caller_code_ends_the_run_with_an_internal_error() {
    // A shim's own raise is an error table that keeps no error behind it,
    // so the code its `code` field names is the code it is read back as.
    let source = one_section(
        "local ok, err = pcall(models.infer, 'a', 'b')
         assert(not ok and err.kind == 'lua', tostring(err))
         err.code = 'recorder_failed'
         err.message = 'the recorder refused the write'
         error(err)",
    );
    let trace = Scenario::new(source).run();
    let error = trace.error();
    assert_eq!(
        (error.code, error.prose.as_str(), error.located_message()),
        (
            ErrorCode::Internal,
            ErrorCode::Internal.prose(),
            "the recorder refused the write (at prompt:16)".to_owned()
        ),
        "the caller code becomes internal and keeps the table's message and line"
    );
}

#[test]
fn a_model_error_with_a_caller_code_ends_the_run_with_an_internal_error() {
    let refused = runvil_types::error::make_failure_error(
        ErrorCode::RecorderFailed,
        "the recorder refused the round",
    );
    let trace = Scenario::new(one_section("return models.infer('hi')"))
        .bound()
        .chat_with(move |_| EffectAnswer::Chat(Err(refused.clone())))
        .run();
    let error = trace.error();
    assert_eq!(
        (error.code, error.message.as_str(), error.prose.as_str()),
        (
            ErrorCode::Internal,
            "the recorder refused the round",
            ErrorCode::Internal.prose()
        ),
        "the model error keeps its message under the internal code"
    );
}

// --- Cross-section store persistence ---

#[test]
fn store_persists_across_sections() {
    // One store is created for the run and threaded to every section. The
    // first section's Lua writes a file; the second, in a fresh context,
    // reads it back - proving the store outlives the context-clearing
    // transition. The read lands in `var`, so it round-trips the value.
    let source = prompt(
        "",
        &[
            ("Writer", "store.write('note.txt', 'handoff text')"),
            (
                "Reader",
                "var.seen = store.read('note.txt')\nreturn var.seen",
            ),
        ],
    );
    let trace = Scenario::new(source).run();
    assert_eq!(
        trace.text(),
        "handoff text",
        "the second section must read what the first wrote"
    );
    // The very same handle still holds the file after the run, confirming
    // both sections shared one store rather than each getting a fresh one.
    assert_eq!(
        trace.read("note.txt").expect("read"),
        "handoff text",
        "the run's store must retain the written file"
    );
}

// --- The `models.loop` shim's exit rules ---

/// The section body every loop exit-rule test runs: the shared loop over a
/// single user message, returning the list's length ahead of the terminal
/// record's text.
fn loop_to_count_and_text() -> String {
    LOOP_TO_TEXT.replacen("return ", "return #messages .. '|' .. ", 1)
}

/// A run of [`loop_to_count_and_text`] against `replies`, with `offered`,
/// when given, in scope under the wire name `echo`.
fn exit_scenario(replies: Vec<ScriptedReply>, offered: Option<FnTool>) -> Scenario {
    let offer = offered.as_ref().map_or_else(String::new, |tool| {
        format!("tools.offer('{}')\n", tool.id())
    });
    let scenario = loop_scenario(one_section(&format!("{offer}{}", loop_to_count_and_text())))
        .chat(&ScriptedChat::new(replies));
    match offered {
        Some(tool) => scenario.offer("echo", tool),
        None => scenario,
    }
}

/// The loop's observation sequence in `trace`.
fn exit_events(trace: &Trace) -> Vec<String> {
    let recorder = Recorder::default();
    trace.replay(&recorder);
    loop_events(&recorder)
}

#[test]
fn a_text_reply_is_the_loops_terminal_record() {
    let trace = exit_scenario(vec![text_finish("all done", "stop")], None).run();
    let events = exit_events(&trace);
    assert_eq!(trace.text(), "2|all done");
    assert_eq!(trace.turns(), 1);
    assert_eq!(events, vec![detail::MODEL_TURN_COMPLETED.to_string()]);
}

#[test]
fn length_finish_reason_reports_model_turn_truncated() {
    let trace = exit_scenario(vec![text_finish("partial answer", "length")], None).run();
    let events = exit_events(&trace);
    assert_eq!(
        trace.text(),
        "2|partial answer",
        "a truncated reply still ends the loop"
    );
    assert_eq!(trace.turns(), 1);
    assert_eq!(
        events,
        vec![
            detail::MODEL_TURN_COMPLETED.to_string(),
            detail::MODEL_TURN_TRUNCATED.to_string(),
        ]
    );
}

#[test]
fn empty_stop_turn_after_tool_call_is_a_clean_exit() {
    // The model stopped deliberately after doing its work through a tool
    // call: the loop accepts the empty turn and appends an empty assistant
    // record as the terminal text.
    let trace = exit_scenario(
        vec![
            calls(&[("call_1", "echo", "{\"value\":\"hi\"}")]),
            text_finish("", "stop"),
        ],
        Some(echo_tool()),
    )
    .run();
    let events = exit_events(&trace);
    assert_eq!(
        trace.text(),
        "4|",
        "a clean stop-exit appends an empty terminal record after the exchange"
    );
    assert_eq!(
        trace.turns(),
        2,
        "the tool-call turn and the accepted empty turn"
    );
    assert_eq!(
        events,
        vec![
            detail::MODEL_TURN_COMPLETED.to_string(),
            detail::TOOL_CALL_SUCCEEDED.to_string(),
            detail::MODEL_TURN_COMPLETED.to_string(),
        ]
    );
}

#[test]
fn empty_stop_turn_without_tool_calls_fails() {
    // Zero prior dispatches: the acceptance conditions cannot hold, so the
    // empty "stop" turn is an `EmptyModelReply` failure. The round itself
    // completed - the scheduler counts and reports it - and the shim's exit
    // rule raises against its finish reason.
    for offered in [None, Some(echo_tool())] {
        let trace = exit_scenario(vec![text_finish("", "stop")], offered).run();
        let events = exit_events(&trace);
        let error = trace.error();
        assert_eq!(error.code, ErrorCode::EmptyReply, "got {error:?}");
        assert_eq!(finish_reason(error), Some("stop"));
        assert_eq!(
            error.prose, "the model replied with no text and no tool calls",
            "the fixed phrase for the code is the prose"
        );
        assert_eq!(trace.turns(), 1, "the empty round is a completed turn");
        assert_eq!(events, vec![detail::MODEL_TURN_COMPLETED.to_string()]);
    }
}

#[test]
fn empty_truncated_final_text_fails_without_truncation_detail() {
    // `finish_reason: "length"` is never a clean exit, even with empty text,
    // and an empty round reports no truncation: there is no reply to have
    // truncated.
    let trace = exit_scenario(vec![text_finish("", "length")], None).run();
    let events = exit_events(&trace);
    let error = trace.error();
    assert_eq!(error.code, ErrorCode::EmptyReply, "got {error:?}");
    assert_eq!(finish_reason(error), Some("length"));
    assert_eq!(events, vec![detail::MODEL_TURN_COMPLETED.to_string()]);
}

#[test]
fn empty_turn_without_finish_reason_after_tool_call_fails() {
    // Fail closed: a missing finish reason is not "stop", so the empty turn
    // is an error even after a successful dispatch.
    let trace = exit_scenario(
        vec![calls(&[("call_1", "echo", "{\"value\":\"hi\"}")]), text("")],
        Some(echo_tool()),
    )
    .run();
    let events = exit_events(&trace);
    let error = trace.error();
    assert_eq!(error.code, ErrorCode::EmptyReply, "got {error:?}");
    assert_eq!(finish_reason(error), None);
    assert_eq!(
        trace.turns(),
        2,
        "the tool-call turn and the completed empty round"
    );
    assert_eq!(
        events,
        vec![
            detail::MODEL_TURN_COMPLETED.to_string(),
            detail::TOOL_CALL_SUCCEEDED.to_string(),
            detail::MODEL_TURN_COMPLETED.to_string(),
        ]
    );
}

#[test]
fn an_empty_reply_is_readable_at_the_call_site_and_appends_nothing() {
    // The raise is pcall-able as the `empty_model_reply` kind holding the
    // finish reason as its field and the client's phrase, after the loop
    // call's line, as its message, and the rejected round leaves the
    // author's list untouched.
    let gateway = ScriptedChat::new(vec![text_finish("", "stop")]);
    let source = one_section(
        "messages:user('say nothing')
         local ok, err = pcall(models.loop)
         assert(not ok, 'the empty round raises')
         assert(#messages == 1, 'a rejected round appends nothing')
         return err.kind .. '|' .. tostring(err.finish_reason) .. '|' .. tostring(err)",
    );
    let trace = loop_scenario(source).chat(&gateway).run();
    assert_eq!(
        trace.text(),
        "empty_model_reply|stop|prompt:13: the model replied with no text and no tool calls"
    );
}

#[test]
fn an_empty_reply_that_ignored_reasoning_says_so_at_the_call_site() {
    // The crate's own wording extends the fixed phrase after a colon, so the
    // author sees why the round counted as empty.
    let gateway = ScriptedChat::new(vec![ScriptedReply::Text {
        model: MOCK_MODEL.to_owned(),
        content: String::new(),
        finish_reason: Some("stop".to_owned()),
        reasoning: Some("only thinking".to_owned()),
        metrics: None,
    }]);
    let source = one_section(
        "messages:user('think only')
         local ok, err = pcall(models.loop)
         return err.kind .. '|' .. tostring(err)",
    );
    let trace = loop_scenario(source).chat(&gateway).run();
    assert_eq!(
        trace.text(),
        "empty_model_reply|prompt:13: the model replied with no text and no tool calls: \
         reasoning content was present but ignored"
    );
}

#[test]
fn an_empty_reply_with_whitespace_only_reasoning_keeps_the_plain_phrase() {
    // The scripted model treats a whitespace-only reasoning channel as
    // absent, so the message keeps the plain phrase.
    let gateway = ScriptedChat::new(vec![ScriptedReply::Text {
        model: MOCK_MODEL.to_owned(),
        content: String::new(),
        finish_reason: Some("stop".to_owned()),
        reasoning: Some("   \n\t ".to_owned()),
        metrics: None,
    }]);
    let source = one_section(
        "messages:user('think blank')
         local ok, err = pcall(models.loop)
         return err.kind .. '|' .. tostring(err)",
    );
    let trace = loop_scenario(source).chat(&gateway).run();
    assert_eq!(
        trace.text(),
        "empty_model_reply|prompt:13: the model replied with no text and no tool calls"
    );
}
