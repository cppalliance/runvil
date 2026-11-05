//! The `Chat` effect's record keeps `after` and `keep` and logs only the
//! request's messages after the first `keep`, while the effect still holds
//! the whole request, and the `Render` effect's record keeps the slot's
//! alias, the entry, and the offered ids. Then the answer record: a `Chat`
//! answer projects to the reply or the requested tool names beside the
//! model and finish reason, never the bodies; a `Render` answer keeps its
//! rendered tools whole; a `ToolCall` answer projects to its text and
//! trust; and every failure projects to its kind's tag and its display
//! text, a model round's or a tool call's to its code's tag and its prose. Each record round-trips through serde, as a run log and a replay
//! depend on, the public JSON record of every kind is the typed record's
//! JSON and reads back identical, and an effect or answer record reads
//! from its JSON form. The store answer's records are in the `store`
//! child.

use std::fmt;

use runvil_model_client::client::{CompletionResult, RawExchange, ToolCall};
use runvil_model_client::model::completion_error;
use runvil_types::error::ErrorCode;
use runvil_types::packs::PackId;
use runvil_types::tools::{OutputTrust, tool_error};
use serde::de::DeserializeOwned;
use serde_json::json;

use super::*;
use crate::test_support::scenario::test_binding;

#[path = "effect-tests-store.rs"]
mod store;

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
    let record = effect.to_record();
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
    let record = chat_effect(three_messages(), Some((RoundId::new(2), 3))).to_record();
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
    let record = effect.to_record();
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
    let record = EffectAnswer::Render(tools_pack("Shout the value.", "shout")).to_record();
    let canonical = serde_json::to_value(&record).expect("a render answer record converts");
    assert_eq!(
        canonical.to_string(),
        r#"{"Render":[{"dynamic_set":[],"pack":"tools","static_set":[{"describes_tools":false,"description":"Shout the value.","id":"tools/echo","model_only_call":false,"parameters_schema":{"type":"object"},"structured_output":false,"survives_stop":false,"wire_name":"shout"}]}]}"#
    );
    let back: AnswerRecord =
        serde_json::from_value(canonical).expect("the canonical form reads back");
    assert_eq!(back, record);
    assert_eq!(answer_round_trip(&record), record);
}

#[test]
fn effect_and_answer_records_read_from_their_json_form() {
    let effect: EffectRecord =
        serde_json::from_str(r#"{"Timer":{"seconds":0.5}}"#).expect("the record reads");
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
        let read: AnswerRecord = serde_json::from_str(line).expect("the record reads");
        assert_eq!(read, expected, "{line}");
    }
}

#[test]
fn a_first_send_records_no_after_and_logs_every_message() {
    let record = chat_effect(three_messages(), None).to_record();
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
    let record = answer.to_record();
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
    let record = answer.to_record();
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
        answer.to_record(),
        AnswerRecord::Chat(Ok(
            ChatAnswerRecord::new("canned-model").with_reply(Some("canned".to_owned()))
        ))
    );
}

#[test]
fn a_failed_chat_answer_records_the_codes_tag_and_the_errors_prose() {
    let mut error = completion_error(
        ErrorCode::MalformedResponse,
        "the model backend sent a reply that could not be understood: \
         completion stream ended without the [DONE] sentinel",
    );
    error.message.push_str(": provider said <hidden>");
    assert!(
        error.message.contains("hidden"),
        "the message holds the detail"
    );
    let expected = error.prose.clone();
    assert!(
        expected.ends_with("without the [DONE] sentinel"),
        "the recorded text keeps the crate-authored specific: {expected}"
    );
    assert!(!expected.contains("hidden"), "the detail is never recorded");
    let record = EffectAnswer::Chat(Err(error)).to_record();
    assert_eq!(
        record,
        AnswerRecord::Chat(Err(FailureRecord::new("malformed_response", expected)))
    );
    assert_eq!(answer_round_trip(&record), record);
}

#[test]
fn a_tool_call_answer_records_the_outputs_text_and_trust() {
    let trusted = EffectAnswer::ToolCall(Ok(ToolOutput::trusted("done"))).to_record();
    assert_eq!(
        trusted,
        AnswerRecord::ToolCall(Ok(ToolAnswerRecord::new("done", OutputTrust::Trusted)))
    );
    assert_eq!(
        serde_json::to_string(&trusted).expect("a tool answer records"),
        r#"{"ToolCall":{"Ok":{"text":"done","trust":"trusted"}}}"#
    );
    assert_eq!(answer_round_trip(&trusted), trusted);

    let untrusted = EffectAnswer::ToolCall(Ok(ToolOutput::untrusted("<html>"))).to_record();
    assert_eq!(
        untrusted,
        AnswerRecord::ToolCall(Ok(ToolAnswerRecord::new("<html>", OutputTrust::Untrusted)))
    );
    assert_eq!(answer_round_trip(&untrusted), untrusted);
}

#[test]
fn a_failed_tool_call_answer_records_the_errors_prose_and_hides_its_cause() {
    let error = tool_error(ErrorCode::ToolBackend, "backend failed")
        .with_cause(&std::io::Error::other("boom"));
    assert!(
        error.message.contains("boom"),
        "the message holds the cause"
    );
    let record = EffectAnswer::ToolCall(Err(error)).to_record();
    assert_eq!(
        record,
        AnswerRecord::ToolCall(Err(FailureRecord::new("tool_backend", "backend failed"))),
        "the prose is the model-safe text; the cause stays in the message"
    );
    assert_eq!(answer_round_trip(&record), record);
}

#[test]
fn a_failed_tool_call_answer_records_its_codes_tag() {
    for code in [
        ErrorCode::ToolInvalidArguments,
        ErrorCode::ToolBackend,
        ErrorCode::ToolTransport,
        ErrorCode::ToolOther,
        ErrorCode::Internal,
    ] {
        let record = EffectAnswer::ToolCall(Err(tool_error(code, "failed"))).to_record();
        assert_eq!(
            record,
            AnswerRecord::ToolCall(Err(FailureRecord::new(code.tag(), "failed"))),
            "{code:?}"
        );
    }
}

#[test]
fn a_failure_record_of_each_kind_round_trips_through_json() {
    let cases = [
        (
            EffectAnswer::Chat(Err(completion_error(
                ErrorCode::RateLimited,
                "the model backend is limiting the request rate",
            ))),
            r#"{"Chat":{"Err":{"kind":"rate_limited","message":"the model backend is limiting the request rate"}}}"#,
        ),
        (
            EffectAnswer::ToolCall(Err(tool_error(
                ErrorCode::ToolInvalidArguments,
                "path is required",
            ))),
            r#"{"ToolCall":{"Err":{"kind":"tool_invalid_arguments","message":"path is required"}}}"#,
        ),
        (
            EffectAnswer::Vfs(Err(VfsError::ReadOnly {
                path: "notes.md".to_owned(),
            })),
            r#"{"Vfs":{"Err":{"kind":"read_only","message":"read-only: notes.md is served read-only and cannot change"}}}"#,
        ),
    ];
    for (answer, line) in cases {
        let record = answer.to_record();
        assert_eq!(
            serde_json::to_string(&record).expect("a failure record serializes"),
            line
        );
        assert_eq!(answer_round_trip(&record), record);
    }
}

/// Checks that `record`, a public record, is the JSON of `typed`, and that
/// it reads back identical through its text and through the typed form.
#[track_caller]
fn assert_exact_round_trip<T>(record: &Value, typed: &T)
where
    T: Serialize + DeserializeOwned + PartialEq + fmt::Debug,
{
    let text = serde_json::to_string(record).expect("a record serializes");
    assert_eq!(
        record,
        &serde_json::to_value(typed).expect("a typed record converts"),
        "the public record is the typed record's JSON: {text}"
    );
    let reread: Value = serde_json::from_str(&text).expect("a record's text is JSON");
    assert_eq!(&reread, record, "the text reads back to the same value");
    let back: T = serde_json::from_value(record.clone()).expect("a record reads back typed");
    assert_eq!(&back, typed);
    assert_eq!(
        &serde_json::to_value(&back).expect("a typed record converts"),
        record,
        "the typed form converts back to the same value"
    );
}

#[test]
fn every_kinds_public_record_is_json_that_round_trips_exactly() {
    let access = || {
        Arc::new(
            runvil_vfs::VfsRef::default()
                .acquire(runvil_vfs::Origin::new("effect record fixture"))
                .expect("the stock backend acquires"),
        )
    };
    let effects = [
        chat_effect(three_messages(), Some((RoundId::new(2), 1))),
        Effect::Render {
            entry: "0.1".to_owned(),
            binding: test_binding("writer", "A general model for tests", "test-model"),
            offered: tools_pack("Echo the value.", "echo"),
        },
        Effect::ToolCall {
            tool: ToolId::parse("tools/echo").expect("a fixture tool id"),
            alias: "echo".to_owned(),
            args: json!({ "value": 0.1, "nested": { "b": 1, "a": [true, null] } }),
            origin: ToolCallOrigin::new("run-1", "Only", ToolCaller::Model).with_entry("0.1"),
            access: access(),
            survives_stop: false,
        },
        Effect::Vfs {
            access: access(),
            op: VfsOp::Write {
                path: "note.md".to_owned(),
                contents: "hello".to_owned(),
            },
        },
        Effect::Timer {
            duration: Duration::from_millis(500),
        },
    ];
    for effect in &effects {
        assert_exact_round_trip(&effect.record(), &effect.to_record());
    }
    let answers = [
        EffectAnswer::Chat(Ok(Box::new(finished_completion(
            CompletionResult::Text("the reply".to_owned()),
            "stop",
        )))),
        EffectAnswer::Chat(Err(completion_error(
            ErrorCode::RateLimited,
            "the model backend is limiting the request rate",
        ))),
        EffectAnswer::Render(tools_pack("Shout the value.", "shout")),
        EffectAnswer::ToolCall(Ok(ToolOutput::untrusted("<html>"))),
        EffectAnswer::Vfs(Ok(VfsOutcome::Paths(vec!["a.txt".to_owned()]))),
        EffectAnswer::Timer,
        EffectAnswer::Dropped,
    ];
    for answer in &answers {
        assert_exact_round_trip(&answer.record(), &answer.to_record());
    }
}
