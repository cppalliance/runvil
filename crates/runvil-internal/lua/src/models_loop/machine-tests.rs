//! Tests for the `models.loop` machine, with no VM: `&str` values in
//! place of the Lua values it passes along, and a plain list read back
//! through `records()`. Messages that reach the chain's inbox append
//! under their arrival line, and the loop reads its inbox once more before
//! it returns.

use runvil_types::error::ErrorFact;

use super::{Input, Machine, Phase, Then};
use crate::compactors::OverflowReason;
use crate::error_value::error_kind;
use crate::messages::MessageList;
use crate::models_loop::tests::{echo_call, round};
use crate::protocol::{ChatOutcome, ChatResult, MessageContent, MessageRecord};

#[path = "machine-tests-compaction.rs"]
mod compaction;

type V = &'static str;

/// One step as a line: the action and the values it names.
fn show(then: &Then<V>) -> String {
    match then {
        Then::Chat { messages, handle } => format!("chat {messages} {}", handle.unwrap_or("-")),
        Then::ToolCall { call, turn } => {
            format!(
                "tool_call {} {} {}@{turn}",
                call.id, call.name, call.arguments
            )
        }
        Then::ToolCalls { calls, turn } => {
            let ids: Vec<&str> = calls.iter().map(|call| call.id.as_str()).collect();
            format!("tool_calls {}@{turn}", ids.join(","))
        }
        Then::Compact { compactor, reason } => format!("compact {compactor} {}", reason.tag()),
        Then::Handle { handler, args } => format!("handle {handler} {args}"),
        Then::Report(returned) => format!("report {}", returned.unwrap_or("raised")),
        Then::Arrivals => "arrivals".to_owned(),
        Then::Return => "return".to_owned(),
        Then::Raise(value) => format!("raise {value}"),
        Then::RaiseNormalized(value) => format!("normalize {value}"),
        Then::RaiseNew(raised) => {
            let kind = error_kind(raised);
            match raised.facts.get(&ErrorFact::FinishReason) {
                Some(serde_json::Value::String(reason)) => {
                    format!("new {kind}|{}|{reason}", raised.message)
                }
                Some(other) => panic!("finish_reason must be a string fact, actual {other}"),
                None => format!("new {kind}|{}", raised.message),
            }
        }
    }
}

/// The list's records as lines: role, text, calls, and the answered id.
fn records(list: &MessageList) -> Vec<String> {
    let line = |record: &MessageRecord| {
        let MessageContent::Text(text) = &record.content else {
            panic!("the loop adds text records only");
        };
        let calls: Vec<String> = record
            .tool_calls
            .iter()
            .map(|call| call.id.clone())
            .collect();
        let id = record.role.tool_call_id().unwrap_or("");
        format!(
            "{} {text}|{}|{id}",
            record.role.kind().as_str(),
            calls.join(",")
        )
    };
    list.to_records().iter().map(line).collect()
}

/// The `chat` answer `round`.
fn answered(round: ChatResult) -> Input<V> {
    Input::Answered(Ok(Box::new(round)))
}

/// A round requesting one call per id, in order.
fn batch(ids: &[&str]) -> Input<V> {
    answered(ChatResult {
        outcome: ChatOutcome::ToolCalls(ids.iter().copied().map(echo_call).collect()),
        ..round()
    })
}

/// A round replying `text`.
fn reply(text: &str) -> Input<V> {
    answered(ChatResult {
        outcome: ChatOutcome::Reply(text.to_owned()),
        ..round()
    })
}

/// An empty round that finished with `stop`.
fn stop() -> Input<V> {
    answered(ChatResult {
        finish_reason: Some("stop".to_owned()),
        ..round()
    })
}

/// A bound tool's answer `text`.
fn dispatched(text: &str) -> Input<V> {
    Input::Dispatched(Ok(text.to_owned()))
}

/// A batch's answer: one text per call the scheduler issued, in order.
fn batched(texts: &[&str]) -> Input<V> {
    Input::Batched(Ok(texts.iter().copied().map(str::to_owned).collect()))
}

/// A raw `pcall`'s `outcome`, under the cancel flag `canceled`.
fn called(outcome: Result<V, V>, canceled: bool) -> Input<V> {
    Input::Called { outcome, canceled }
}

/// The messages taken from the chain's inbox, oldest first.
pub(super) fn arrived(texts: &[&str]) -> Input<V> {
    Input::Arrived(texts.iter().copied().map(str::to_owned).collect())
}

/// The record line of a message appended on the main chain.
pub(super) fn from_operator(text: &str) -> String {
    format!("user This message arrived while you were working:\n\n{text}||")
}

/// A `models.loop` call on the main chain capped at `cap` rounds over
/// `list`, at its first chat.
fn chatting(cap: usize, list: &MessageList) -> Machine<V> {
    let (machine, first) = Machine::begin(cap, list.clone(), "list", None, "compactor", false);
    assert_eq!(show(&first), "chat list -");
    machine
}

/// Answers the final check with no message, which returns.
#[track_caller]
pub(super) fn settle(machine: &mut Machine<V>) {
    assert_eq!(show(&machine.step(arrived(&[]))), "return");
    assert!(matches!(machine.phase(), Phase::Done));
}

#[test]
fn a_zero_cap_raises_tool_loop_exhausted_before_any_chat() {
    let (machine, first) =
        Machine::begin(0, MessageList::default(), "list", None, "compactor", false);
    assert_eq!(
        show(&first),
        "new tool_loop_exhausted|tool-call loop did not converge"
    );
    assert!(matches!(machine.phase(), Phase::Done));
}

#[test]
fn a_round_starts_with_the_chat_naming_the_handle_and_pushes_nothing() {
    let list = MessageList::default();
    let (machine, first) = Machine::begin(2, list.clone(), "list", Some("h"), "compactor", false);
    assert_eq!(show(&first), "chat list h");
    assert!(matches!(machine.phase(), Phase::Chatting(_)));
    assert!(
        records(&list).is_empty(),
        "a round's start pushes no record"
    );
}

#[test]
fn each_round_outcome_is_judged_overflow_calls_reply_or_empty() {
    let finished = |outcome| ChatResult {
        outcome,
        finish_reason: Some("stop".to_owned()),
        ..round()
    };
    let cases = [
        (
            finished(ChatOutcome::Overflow(OverflowReason::Precheck)),
            "compact compactor precheck",
        ),
        (
            finished(ChatOutcome::ToolCalls(vec![echo_call("c1")])),
            "tool_calls c1@7",
        ),
        (finished(ChatOutcome::Reply("text".to_owned())), "arrivals"),
        (
            finished(ChatOutcome::Reply(String::new())),
            "new empty_model_reply|empty model reply|stop",
        ),
        (
            finished(ChatOutcome::Empty(Some("nothing".to_owned()))),
            "new empty_model_reply|nothing|stop",
        ),
    ];
    for (answer, expected) in cases {
        let list = MessageList::default();
        let mut machine = chatting(3, &list);
        assert_eq!(show(&machine.step(answered(answer))), expected);
    }
}

#[test]
fn a_batch_dispatches_in_order_and_appends_its_records_only_once_complete() {
    let list = MessageList::default();
    let mut machine = chatting(3, &list);
    assert_eq!(
        show(&machine.step(batch(&["c1", "c2"]))),
        "tool_calls c1,c2@7"
    );
    assert!(
        records(&list).is_empty(),
        "an unanswered batch adds nothing"
    );
    assert_eq!(show(&machine.step(batched(&["one", "two"]))), "chat list -");
    assert_eq!(show(&machine.step(reply("done"))), "arrivals");
    settle(&mut machine);
    assert_eq!(
        records(&list),
        [
            "assistant |c1,c2|",
            "tool one||c1",
            "tool two||c2",
            "assistant done||"
        ]
    );
}

#[test]
fn a_short_batch_answer_sends_the_next_call_alone_then_the_rest_together() {
    let list = MessageList::default();
    let mut machine = chatting(3, &list);
    assert_eq!(
        show(&machine.step(batch(&["c1", "c2", "c3", "c4"]))),
        "tool_calls c1,c2,c3,c4@7"
    );
    assert_eq!(
        show(&machine.step(batched(&["one"]))),
        r#"tool_call c2 echo {"id":"c2"}@7"#
    );
    assert_eq!(show(&machine.step(dispatched("two"))), "tool_calls c3,c4@7");
    assert!(
        records(&list).is_empty(),
        "a half-answered batch adds nothing"
    );
    assert_eq!(
        show(&machine.step(batched(&["three", "four"]))),
        "chat list -"
    );
    assert_eq!(
        records(&list),
        [
            "assistant |c1,c2,c3,c4|",
            "tool one||c1",
            "tool two||c2",
            "tool three||c3",
            "tool four||c4"
        ]
    );
}

#[test]
fn the_clean_exit_counts_answered_calls_across_batches_and_resets_per_call() {
    let list = MessageList::default();
    let mut machine = chatting(4, &list);
    for id in ["c1", "c2"] {
        machine.step(batch(&[id]));
        assert_eq!(show(&machine.step(batched(&["ok"]))), "chat list -");
    }
    assert_eq!(show(&machine.step(stop())), "arrivals");
    settle(&mut machine);
    assert_eq!(
        records(&list).last().map(String::as_str),
        Some("assistant ||")
    );
    let mut fresh = chatting(4, &list);
    assert_eq!(
        show(&fresh.step(stop())),
        "new empty_model_reply|empty model reply|stop",
        "a new call starts its count at zero"
    );
}

#[test]
fn the_cap_raises_after_the_last_allowed_batch_and_before_another_chat() {
    let list = MessageList::default();
    let mut machine = chatting(1, &list);
    machine.step(batch(&["c1"]));
    assert_eq!(
        show(&machine.step(batched(&["ok"]))),
        "new tool_loop_exhausted|tool-call loop did not converge"
    );
    assert!(matches!(machine.phase(), Phase::Done));
    assert_eq!(records(&list), ["assistant |c1|", "tool ok||c1"]);
}

#[test]
fn an_empty_reply_raises_its_detail_or_the_fallback() {
    let detailed = ChatResult {
        outcome: ChatOutcome::Empty(Some("the reply was empty".to_owned())),
        finish_reason: Some("length".to_owned()),
        ..round()
    };
    let mut machine = chatting(2, &MessageList::default());
    assert_eq!(
        show(&machine.step(answered(detailed))),
        "new empty_model_reply|the reply was empty|length"
    );
    let mut machine = chatting(2, &MessageList::default());
    assert_eq!(
        show(&machine.step(answered(round()))),
        "new empty_model_reply|empty model reply"
    );
}

#[test]
fn a_failed_answer_raises_its_value_unchanged() {
    let mut machine = chatting(2, &MessageList::default());
    assert_eq!(
        show(&machine.step(Input::Answered(Err("chat")))),
        "raise chat"
    );
    let mut machine = chatting(2, &MessageList::default());
    machine.step(batch(&["c1"]));
    assert_eq!(
        show(&machine.step(batched(&[]))),
        r#"tool_call c1 echo {"id":"c1"}@7"#
    );
    assert_eq!(
        show(&machine.step(Input::Dispatched(Err("tool")))),
        "raise tool"
    );
    let mut machine = chatting(2, &MessageList::default());
    machine.step(batch(&["c1", "c2"]));
    assert_eq!(
        show(&machine.step(Input::Batched(Err("batch")))),
        "raise batch"
    );
}

/// A machine waiting on an adhoc call's handler, over `list`.
fn handling(list: &MessageList) -> Machine<V> {
    let mut machine = chatting(2, list);
    machine.step(batch(&["c1"]));
    assert_eq!(
        show(&machine.step(batched(&[]))),
        r#"tool_call c1 echo {"id":"c1"}@7"#
    );
    let adhoc = Input::Adhoc {
        handler: "handler",
        args: "args",
    };
    assert_eq!(show(&machine.step(adhoc)), "handle handler args");
    machine
}

#[test]
fn an_adhoc_call_reports_its_outcome_then_raises_the_handlers_value_first() {
    let list = MessageList::default();
    let mut machine = handling(&list);
    assert_eq!(
        show(&machine.step(called(Ok("value"), false))),
        "report value"
    );
    assert_eq!(
        show(&machine.step(Input::Reported(Ok("text".to_owned())))),
        "chat list -"
    );
    assert_eq!(records(&list)[1], "tool text||c1");
    let mut machine = handling(&list);
    machine.step(called(Ok("value"), false));
    assert_eq!(
        show(&machine.step(Input::Reported(Err("refused")))),
        "raise refused"
    );
    for answer in [Ok("text".to_owned()), Err("refused")] {
        let mut machine = handling(&list);
        assert_eq!(
            show(&machine.step(called(Err("own"), false))),
            "report raised"
        );
        assert_eq!(show(&machine.step(Input::Reported(answer))), "raise own");
    }
}

#[test]
fn cancellation_raises_a_failed_handler_or_compactor_raw_and_a_return_still_wins() {
    let list = MessageList::default();
    let mut machine = handling(&list);
    assert_eq!(
        show(&machine.step(called(Err("abort"), true))),
        "raise abort"
    );
    let mut machine = handling(&list);
    assert_eq!(
        show(&machine.step(called(Ok("value"), true))),
        "report value"
    );
    let overflow = ChatResult {
        outcome: ChatOutcome::Overflow(OverflowReason::Provider),
        ..round()
    };
    let cases = [
        (Err("abort"), true, "raise abort"),
        (Err("failure"), false, "normalize failure"),
        (
            Ok("returned"),
            true,
            "new context_exhausted|context exhausted: the provider rejected the request as \
             exceeding the model's context window",
        ),
    ];
    for (outcome, canceled, expected) in cases {
        let mut machine = chatting(2, &list);
        assert_eq!(
            show(&machine.step(answered(overflow.clone()))),
            "compact compactor provider"
        );
        assert_eq!(show(&machine.step(called(outcome, canceled))), expected);
    }
}

#[test]
fn an_input_its_phase_does_not_await_raises_internal() {
    let unexpected = "new internal|internal invariant violated: the models.loop step received \
                      an input its phase does not await";
    let (mut machine, _) =
        Machine::begin(2, MessageList::default(), "list", None, "compactor", false);
    assert_eq!(show(&machine.step(dispatched("early"))), unexpected);
    assert_eq!(
        show(&machine.step(reply("late"))),
        unexpected,
        "a raise ends the call"
    );
    let mut machine = chatting(2, &MessageList::default());
    machine.step(batch(&["c1"]));
    assert_eq!(
        show(&machine.step(batched(&["one", "two"]))),
        unexpected,
        "a batch answer holds no more texts than the calls it was sent"
    );
    let mut machine = chatting(2, &MessageList::default());
    assert_eq!(show(&machine.step(reply("done"))), "arrivals");
    assert_eq!(
        show(&machine.step(reply("again"))),
        unexpected,
        "the final check awaits the inbox's messages, not a round"
    );
}

#[test]
fn messages_that_answer_a_chat_append_under_the_main_chains_line_and_send_again_uncounted() {
    let list = MessageList::default();
    let mut machine = chatting(1, &list);
    assert_eq!(
        show(&machine.step(arrived(&["one", "two"]))),
        "chat list -",
        "the loop sends again, and a cap of 1 is not spent"
    );
    assert_eq!(
        records(&list),
        [from_operator("one"), from_operator("two")],
        "each message is its own user record under its arrival line"
    );
    assert_eq!(show(&machine.step(reply("done"))), "arrivals");
    settle(&mut machine);
    assert_eq!(
        records(&list).last().map(String::as_str),
        Some("assistant done||")
    );
}

#[test]
fn on_a_task_a_message_appends_under_the_owners_line() {
    let list = MessageList::default();
    let (mut machine, first) = Machine::begin(2, list.clone(), "list", None, "compactor", true);
    assert_eq!(show(&first), "chat list -");
    assert_eq!(show(&machine.step(arrived(&["shorter"]))), "chat list -");
    assert_eq!(
        records(&list),
        ["user The task that started you sent this message while you were working:\n\nshorter||"]
    );
}

#[test]
fn the_final_check_appends_late_messages_after_the_reply_and_starts_a_counted_round() {
    let list = MessageList::default();
    let mut machine = chatting(2, &list);
    assert_eq!(show(&machine.step(reply("done"))), "arrivals");
    assert_eq!(
        show(&machine.step(arrived(&["late"]))),
        "chat list -",
        "a message the final check takes keeps the loop going"
    );
    assert_eq!(
        records(&list),
        ["assistant done||".to_owned(), from_operator("late")]
    );
    assert_eq!(show(&machine.step(reply("again"))), "arrivals");
    settle(&mut machine);
    let mut capped = chatting(1, &MessageList::default());
    assert_eq!(show(&capped.step(reply("done"))), "arrivals");
    assert_eq!(
        show(&capped.step(arrived(&["late"]))),
        "new tool_loop_exhausted|tool-call loop did not converge",
        "the round a late message starts counts against the cap"
    );
}

#[test]
fn the_clean_exit_reads_the_inbox_once_more_before_it_returns() {
    let list = MessageList::default();
    let mut machine = chatting(3, &list);
    machine.step(batch(&["c1"]));
    machine.step(batched(&["ok"]));
    assert_eq!(show(&machine.step(stop())), "arrivals");
    assert_eq!(show(&machine.step(arrived(&["more"]))), "chat list -");
    assert_eq!(
        records(&list)[2..],
        ["assistant ||".to_owned(), from_operator("more")]
    );
}
