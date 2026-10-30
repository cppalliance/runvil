//! The `Chat` effect's record keeps `after` and `keep` and logs only the
//! request's messages after the first `keep`, while the effect still holds
//! the whole request, and the `Render` effect's record keeps the slot's
//! alias, the entry, and the offered ids. Then the answer record: a `Chat`
//! answer projects to the reply or the requested tool names beside the
//! model and finish reason, never the bodies; a `Render` answer keeps its
//! rendered tools whole; a `ToolCall` answer projects to its text and
//! trust; and every failure projects to its kind's tag and its display
//! text. Each record round-trips through serde, as a run log and a replay
//! depend on, and a record written before the render existed reads
//! unchanged.

use runvil_model_client::client::{CompletionResult, RawExchange, ToolCall};
use runvil_model_client::model::CompletionErrorKind;
use runvil_types::packs::PackId;
use runvil_types::tools::{OutputTrust, ToolErrorKind};
use serde_json::json;

use super::*;
use crate::test_support::scenario::test_binding;

/// Serializes the answer record and reads it back.
#[track_caller]
fn answer_round_trip(record: &AnswerRecord) -> AnswerRecord {
    let text = serde_json::to_string(record).expect("an answer record serializes");
    serde_json::from_str(&text).expect("a serialized answer record deserializes")
}

/// Round 3's effect over `messages`, extending `after`, an earlier round
/// with how many of the messages it shares.
fn chat_effect(messages: Vec<Message>, after: Option<(RoundId, u64)>) -> Effect {
    let binding = test_binding("writer", "A general model for tests", "test-model");
    Effect::Chat {
        binding,
        messages,
        after,
        tools: Vec::new(),
        round: Round::new(RoundId::new(3), ReplyOrigin::Chat),
    }
}

/// The three-message request the record cases send.
fn three_messages() -> Vec<Message> {
    vec![
        Message::user("ask"),
        Message::assistant("reply"),
        Message::user("again"),
    ]
}

#[test]
fn a_chat_record_copies_after_and_keep_and_logs_only_the_messages_after_keep() {
    let effect = chat_effect(three_messages(), Some((RoundId::new(2), 2)));
    let record = effect.record();
    let EffectRecord::Chat {
        round,
        after,
        keep,
        messages,
        ..
    } = &record
    else {
        panic!("a chat effect records a chat record: {record:?}");
    };
    assert_eq!(*round, RoundId::new(3));
    assert_eq!(*after, Some(RoundId::new(2)));
    assert_eq!(*keep, 2);
    assert_eq!(
        *messages,
        vec![json!({ "role": "user", "content": "again" })],
        "the first two messages repeat round 2's request"
    );
    let Effect::Chat { messages, .. } = &effect else {
        panic!("the effect is a chat round");
    };
    assert_eq!(
        messages.len(),
        3,
        "the effect still sends the whole request"
    );

    let text = serde_json::to_string(&record).expect("a record serializes");
    let wire: Value = serde_json::from_str(&text).expect("a record is JSON");
    assert_eq!(
        wire["Chat"]["after"],
        json!(2),
        "after is a bare round number"
    );
    assert_eq!(wire["Chat"]["keep"], json!(2));
    let read: EffectRecord = serde_json::from_str(&text).expect("a record deserializes");
    assert_eq!(read, record);
}

#[test]
fn a_keep_equal_to_the_message_count_logs_no_messages() {
    let record = chat_effect(three_messages(), Some((RoundId::new(2), 3))).record();
    let EffectRecord::Chat {
        after,
        keep,
        messages,
        ..
    } = &record
    else {
        panic!("a chat effect records a chat record: {record:?}");
    };
    assert_eq!((*after, *keep), (Some(RoundId::new(2)), 3));
    assert!(
        messages.is_empty(),
        "the request repeats round 2's whole: {messages:?}"
    );
}

/// The `tools` Pack's one tool, `tools/echo`, as `description` describes
/// it under the wire name `wire_name`.
#[track_caller]
fn tools_pack(description: &str, wire_name: &str) -> Vec<PackToolSets<ToolInfo>> {
    let echo = ToolInfo::new(
        ToolId::parse("tools/echo").expect("a fixture tool id"),
        description,
        json!({ "type": "object" }),
    )
    .wire_name(wire_name);
    vec![PackToolSets::new(
        PackId::parse("tools").expect("a fixture Pack id"),
        vec![echo],
        Vec::new(),
    )]
}

#[test]
fn a_render_record_keeps_the_slots_alias_the_entry_and_the_offered_ids() {
    let effect = Effect::Render {
        entry: "0.1".to_owned(),
        binding: test_binding("writer", "A general model for tests", "test-model"),
        offered: tools_pack("Echo the value.", "echo"),
    };
    let record = effect.record();
    let canonical = serde_json::to_value(&record).expect("a render record converts");
    assert_eq!(
        canonical.to_string(),
        r#"{"Render":{"alias":"writer","entry":"0.1","offered":[{"dynamic_set":[],"pack":"tools","static_set":["tools/echo"]}]}}"#
    );
    let back: EffectRecord =
        serde_json::from_value(canonical).expect("the canonical form reads back");
    assert_eq!(back, record);
}

#[test]
fn a_render_answer_records_each_rendered_tool_whole() {
    let record = EffectAnswer::Render(tools_pack("Shout the value.", "shout")).record();
    let canonical = serde_json::to_value(&record).expect("a render answer record converts");
    assert_eq!(
        canonical.to_string(),
        r#"{"Render":[{"dynamic_set":[],"pack":"tools","static_set":[{"describes_tools":false,"description":"Shout the value.","id":"tools/echo","parameters_schema":{"type":"object"},"structured_output":false,"survives_stop":false,"wire_name":"shout"}]}]}"#
    );
    let back: AnswerRecord =
        serde_json::from_value(canonical).expect("the canonical form reads back");
    assert_eq!(back, record);
    assert_eq!(answer_round_trip(&record), record);
}

#[test]
fn records_written_before_the_render_existed_read_unchanged() {
    let effect: EffectRecord =
        serde_json::from_str(r#"{"Timer":{"seconds":0.5}}"#).expect("an older record reads");
    assert_eq!(effect, EffectRecord::Timer { seconds: 0.5 });
    for (line, expected) in [
        (
            r#"{"Chat":{"Ok":{"model":"m","finish_reason":null,"reply":"hi","tool_calls":[]}}}"#,
            AnswerRecord::Chat(Ok(
                ChatAnswerRecord::new("m").with_reply(Some("hi".to_owned()))
            )),
        ),
        (r#""Dropped""#, AnswerRecord::Dropped),
    ] {
        let read: AnswerRecord = serde_json::from_str(line).expect("an older record reads");
        assert_eq!(read, expected, "{line}");
    }
}

#[test]
fn a_first_send_records_no_after_and_logs_every_message() {
    let record = chat_effect(three_messages(), None).record();
    let EffectRecord::Chat {
        after,
        keep,
        messages,
        ..
    } = &record
    else {
        panic!("a chat effect records a chat record: {record:?}");
    };
    assert_eq!((*after, *keep), (None, 0));
    assert_eq!(messages.len(), 3, "the whole request starts here");
}

/// A completion with `finish_reason` and both bodies set: what a
/// transport hands the loop, as opposed to the bare canned one.
fn finished_completion(result: CompletionResult, finish_reason: &str) -> Completion {
    let raw = RawExchange::new(
        json!({ "messages": [{ "role": "user", "content": "ask" }] }),
        json!({ "model": "test-model", "choices": [{ "index": 0, "finish_reason": finish_reason }] }),
    );
    Completion::from_result(result, "test-model")
        .expect("a whole result is accepted")
        .with_finish_reason(finish_reason)
        .with_raw(raw)
}

#[test]
fn a_chat_answer_records_the_reply_model_and_finish_reason_without_the_bodies() {
    let answer = EffectAnswer::Chat(Ok(Box::new(finished_completion(
        CompletionResult::Text("the reply".to_owned()),
        "stop",
    ))));
    let record = answer.record();
    assert_eq!(
        record,
        AnswerRecord::Chat(Ok(ChatAnswerRecord::new("test-model")
            .with_finish_reason(Some("stop".to_owned()))
            .with_reply(Some("the reply".to_owned()))))
    );
    let text = serde_json::to_string(&record).expect("an answer record serializes");
    assert!(
        !text.contains("choices") && !text.contains("messages"),
        "the round's bodies travel as debug events, not in the answer: {text}"
    );
    assert_eq!(answer_round_trip(&record), record);
}

#[test]
fn a_chat_answer_with_tool_calls_records_their_names_in_call_order_and_no_reply() {
    let calls = CompletionResult::ToolCalls(vec![
        ToolCall::from_parts("call-1", "grab", json!({ "value": "hi" })).expect("a whole call"),
        ToolCall::from_parts("call-2", "echo", json!({})).expect("a whole call"),
    ]);
    let answer = EffectAnswer::Chat(Ok(Box::new(finished_completion(calls, "tool_calls"))));
    let record = answer.record();
    assert_eq!(
        record,
        AnswerRecord::Chat(Ok(ChatAnswerRecord::new("test-model")
            .with_finish_reason(Some("tool_calls".to_owned()))
            .with_tool_calls(vec!["grab".to_owned(), "echo".to_owned()])))
    );
    let text = serde_json::to_string(&record).expect("an answer record serializes");
    assert!(
        !text.contains("call-1") && !text.contains("value"),
        "the calls' ids and arguments stay in the tool-call event: {text}"
    );
    assert_eq!(answer_round_trip(&record), record);
}

#[test]
fn a_canned_chat_answer_records_no_finish_reason() {
    let completion =
        Completion::from_result(CompletionResult::Text("canned".to_owned()), "canned-model")
            .expect("a text result is accepted");
    let answer = EffectAnswer::Chat(Ok(Box::new(completion)));
    assert_eq!(
        answer.record(),
        AnswerRecord::Chat(Ok(
            ChatAnswerRecord::new("canned-model").with_reply(Some("canned".to_owned()))
        ))
    );
}

#[test]
fn a_failed_chat_answer_records_the_kinds_tag_and_the_errors_display_text() {
    let error = CompletionError::new(
        CompletionErrorKind::MalformedResponse,
        "the model backend sent a reply that could not be understood: \
         completion stream ended without the [DONE] sentinel",
    )
    .with_detail("provider said <hidden>");
    let expected = error.to_string();
    assert!(
        expected.ends_with("without the [DONE] sentinel"),
        "the recorded text keeps the crate-authored specific: {expected}"
    );
    assert!(!expected.contains("hidden"), "the detail is never recorded");
    let record = EffectAnswer::Chat(Err(error)).record();
    assert_eq!(
        record,
        AnswerRecord::Chat(Err(FailureRecord::new("malformed_response", expected)))
    );
    assert_eq!(answer_round_trip(&record), record);
}

#[test]
fn a_tool_call_answer_records_the_outputs_text_and_trust() {
    let trusted = EffectAnswer::ToolCall(Ok(ToolOutput::trusted("done"))).record();
    assert_eq!(
        trusted,
        AnswerRecord::ToolCall(Ok(ToolAnswerRecord::new("done", OutputTrust::Trusted)))
    );
    assert_eq!(
        serde_json::to_string(&trusted).expect("a tool answer records"),
        r#"{"ToolCall":{"Ok":{"text":"done","trust":"trusted"}}}"#
    );
    assert_eq!(answer_round_trip(&trusted), trusted);

    let untrusted = EffectAnswer::ToolCall(Ok(ToolOutput::untrusted("<html>"))).record();
    assert_eq!(
        untrusted,
        AnswerRecord::ToolCall(Ok(ToolAnswerRecord::new("<html>", OutputTrust::Untrusted)))
    );
    assert_eq!(answer_round_trip(&untrusted), untrusted);
}

#[test]
fn a_failed_tool_call_answer_records_the_errors_display_text_and_hides_its_source() {
    let error = ToolError::new(ToolErrorKind::Backend, "backend failed")
        .with_source(std::io::Error::other("boom"));
    let record = EffectAnswer::ToolCall(Err(error)).record();
    assert_eq!(
        record,
        AnswerRecord::ToolCall(Err(FailureRecord::new("backend", "backend failed"))),
        "the display text is the model-safe message; the source stays behind it"
    );
    assert_eq!(answer_round_trip(&record), record);
}

#[test]
fn a_failure_record_of_each_kind_round_trips_through_json() {
    let cases = [
        (
            EffectAnswer::Chat(Err(CompletionError::new(
                CompletionErrorKind::RateLimited,
                "the model backend is limiting the request rate",
            ))),
            r#"{"Chat":{"Err":{"kind":"rate_limited","message":"the model backend is limiting the request rate"}}}"#,
        ),
        (
            EffectAnswer::ToolCall(Err(ToolError::new(
                ToolErrorKind::InvalidArguments,
                "path is required",
            ))),
            r#"{"ToolCall":{"Err":{"kind":"invalid_arguments","message":"path is required"}}}"#,
        ),
        (
            EffectAnswer::Vfs(Err(VfsError::ReadOnly {
                path: "notes.md".to_owned(),
            })),
            r#"{"Vfs":{"Err":{"kind":"read_only","message":"read-only: notes.md is served read-only and cannot change"}}}"#,
        ),
    ];
    for (answer, line) in cases {
        let record = answer.record();
        assert_eq!(
            serde_json::to_string(&record).expect("a failure record serializes"),
            line
        );
        assert_eq!(answer_round_trip(&record), record);
    }
}

#[test]
fn the_store_answer_records_byte_identical_json_for_every_outcome() {
    // `AnswerRecord::Vfs` holds the `VfsOutcome` itself, and its JSON
    // is a fixed run-log format: the unit outcome as a bare string, the
    // payload outcomes in serde's externally tagged form.
    let unit = EffectAnswer::Vfs(Ok(VfsOutcome::Unit)).record();
    assert_eq!(
        serde_json::to_string(&unit).expect("a store answer records"),
        r#"{"Vfs":{"Ok":"Unit"}}"#
    );
    assert_eq!(answer_round_trip(&unit), unit);

    let text = EffectAnswer::Vfs(Ok(VfsOutcome::Text("the read".to_owned()))).record();
    assert_eq!(
        serde_json::to_string(&text).expect("a store answer records"),
        r#"{"Vfs":{"Ok":{"Text":"the read"}}}"#
    );
    assert_eq!(answer_round_trip(&text), text);

    let paths = EffectAnswer::Vfs(Ok(VfsOutcome::Paths(vec![
        "a.txt".to_owned(),
        "b.txt".to_owned(),
    ])))
    .record();
    assert_eq!(
        serde_json::to_string(&paths).expect("a store answer records"),
        r#"{"Vfs":{"Ok":{"Paths":["a.txt","b.txt"]}}}"#
    );
    assert_eq!(answer_round_trip(&paths), paths);

    let flag = EffectAnswer::Vfs(Ok(VfsOutcome::Bool(true))).record();
    assert_eq!(
        serde_json::to_string(&flag).expect("a store answer records"),
        r#"{"Vfs":{"Ok":{"Bool":true}}}"#
    );
    assert_eq!(answer_round_trip(&flag), flag);
}

#[test]
fn a_failed_store_answer_records_the_vfs_errors_tag_and_display_text() {
    let error = VfsError::NotFound {
        path: "missing.txt".to_owned(),
    };
    let expected = error.to_string();
    assert_eq!(expected, "not found: missing.txt");
    let record = EffectAnswer::Vfs(Err(error)).record();
    assert_eq!(
        record,
        AnswerRecord::Vfs(Err(FailureRecord::new("not_found", expected)))
    );
    assert_eq!(
        serde_json::to_string(&record).expect("a store answer records"),
        r#"{"Vfs":{"Err":{"kind":"not_found","message":"not found: missing.txt"}}}"#
    );
    assert_eq!(answer_round_trip(&record), record);
}
