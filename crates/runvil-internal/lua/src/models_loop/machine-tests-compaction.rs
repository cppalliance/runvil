//! Tests for the machine's compaction after an overflow: the snapshot and
//! the `compacting` flag around the compactor, the settled outcome, the
//! uncounted retry, and the one compaction a round gets. Each test edits
//! the list from Rust where a compactor would edit it from Lua.

use serde_json::json;

use super::{answered, batch, batched, called, chatting, records, reply, show};
use crate::compactors::OverflowReason;
use crate::messages::MessageList;
use crate::models_loop::machine::Input;
use crate::models_loop::tests::round;
use crate::protocol::{
    ChatOutcome, ChatResult, MessageContent, MessageRecord, RecordRole, ToolCallRecord,
};
use crate::tests::user;

/// The precheck's and the provider's exhaustion, as the machine raises it.
const PRECHECK: &str = "new context_exhausted|context exhausted: the request precheck overflowed \
                        the model's context window";
const PROVIDER: &str = "new context_exhausted|context exhausted: the provider rejected the \
                        request as exceeding the model's context window";

/// The round answer that overflowed for `reason`.
fn overflow(reason: OverflowReason) -> Input<&'static str> {
    answered(ChatResult {
        outcome: ChatOutcome::Overflow(reason),
        ..round()
    })
}

/// A list holding one user record long enough for `short` to shrink it.
fn long_list() -> MessageList {
    let list = MessageList::default();
    list.push(user("a prompt long enough to shrink")).unwrap();
    list
}

/// Replaces `list`'s records with one short user record, as a compactor
/// that shrank the list would.
fn short(list: &MessageList) {
    list.clear();
    list.push(user("short")).unwrap();
}

#[test]
fn an_overflow_flags_the_list_and_a_raising_compactor_restores_and_clears_it() {
    let list = long_list();
    for (outcome, cancelled, expected) in [
        (Err("failure"), false, "normalize failure"),
        (Err("abort"), true, "raise abort"),
    ] {
        let mut machine = chatting(2, &list);
        assert_eq!(
            show(&machine.step(overflow(OverflowReason::Precheck))),
            "compact compactor precheck"
        );
        assert!(list.compacting(), "the flag is set around the compactor");
        short(&list);
        assert_eq!(show(&machine.step(called(outcome, cancelled))), expected);
        assert!(
            !list.compacting(),
            "the flag clears when the compactor raises"
        );
        assert_eq!(records(&list), ["user a prompt long enough to shrink||"]);
    }
}

#[test]
fn a_compaction_that_shrank_retries_without_counting_and_keeps_its_records() {
    let list = long_list();
    let mut machine = chatting(1, &list);
    machine.step(overflow(OverflowReason::Precheck));
    short(&list);
    assert_eq!(
        show(&machine.step(called(Ok("returned"), false))),
        "chat list -",
        "the retry is not counted against a cap of 1"
    );
    assert!(!list.compacting());
    assert_eq!(show(&machine.step(reply("done"))), "return");
    assert_eq!(records(&list), ["user short||", "assistant done||"]);
}

#[test]
fn a_second_overflow_restores_the_snapshot_and_a_failed_retry_keeps_the_compaction() {
    let list = long_list();
    let mut machine = chatting(2, &list);
    machine.step(overflow(OverflowReason::Precheck));
    short(&list);
    machine.step(called(Ok("returned"), false));
    assert_eq!(
        show(&machine.step(overflow(OverflowReason::Provider))),
        PROVIDER
    );
    assert_eq!(records(&list), ["user a prompt long enough to shrink||"]);
    let mut machine = chatting(2, &list);
    machine.step(overflow(OverflowReason::Provider));
    short(&list);
    machine.step(called(Ok("returned"), false));
    assert_eq!(
        show(&machine.step(Input::Answered(Err("stopped")))),
        "raise stopped"
    );
    assert_eq!(
        records(&list),
        ["user short||"],
        "a retry that fails another way keeps the compacted records"
    );
}

#[test]
fn a_retry_answered_with_tool_calls_drops_the_snapshot_so_a_later_round_compacts_again() {
    let list = long_list();
    let mut machine = chatting(2, &list);
    machine.step(overflow(OverflowReason::Precheck));
    short(&list);
    machine.step(called(Ok("returned"), false));
    assert_eq!(show(&machine.step(batch(&["c1"]))), "tool_calls c1@7");
    assert_eq!(show(&machine.step(batched(&["r1"]))), "chat list -");
    assert_eq!(
        show(&machine.step(overflow(OverflowReason::Provider))),
        "compact compactor provider",
        "the next round's overflow compacts instead of restoring"
    );
    assert_eq!(
        records(&list),
        ["user short||", "assistant |c1|", "tool r1||c1"]
    );
    short(&list);
    assert_eq!(
        show(&machine.step(called(Ok("returned"), false))),
        "chat list -"
    );
    assert_eq!(show(&machine.step(reply("done"))), "return");
    assert_eq!(records(&list), ["user short||", "assistant done||"]);
}

#[test]
fn a_return_that_breaks_a_rule_restores_the_snapshot_and_raises() {
    let list = long_list();
    let mut machine = chatting(2, &list);
    machine.step(overflow(OverflowReason::Precheck));
    assert_eq!(
        show(&machine.step(called(Ok("returned"), false))),
        PRECHECK,
        "a list that did not shrink"
    );
    let mut machine = chatting(2, &list);
    machine.step(overflow(OverflowReason::Precheck));
    list.clear();
    list.push(MessageRecord {
        role: RecordRole::Assistant,
        content: MessageContent::Text(String::new()),
        tool_calls: vec![ToolCallRecord {
            id: "c1".to_owned(),
            name: "echo".to_owned(),
            arguments: json!({}),
        }],
        meta: None,
    })
    .unwrap();
    assert_eq!(
        show(&machine.step(called(Ok("returned"), false))),
        "new lua|messages[1] tool call \"c1\" has no tool result",
        "a list that breaks a structural rule"
    );
    assert!(!list.compacting());
    assert_eq!(records(&list), ["user a prompt long enough to shrink||"]);
}
