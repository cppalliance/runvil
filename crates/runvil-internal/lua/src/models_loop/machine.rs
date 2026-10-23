//! The rules of `models.loop` over typed inputs, with no Lua VM: the
//! round cap, the order a round's answer is judged in, the batch rule, an
//! adhoc tool's report and re-raise, and compaction after an overflow.
//! The machine pushes every record onto the list itself, and every Lua
//! value it only passes along is an opaque `V`, so the unit tests run over
//! a plain `MessageList` with any `V`.
//!
//! The batch rule: every call a round left unanswered goes out as one
//! `tool_calls` request, even a lone one, and a call goes out alone as a
//! `tool_call` request only when the scheduler's answer stops short of
//! it. The scheduler decides which calls run together, so a call's route
//! never depends on how many calls share its round. The round's records
//! append only once every call has its result, in the model's order.
//!
//! Compaction: an overflowed round snapshots the list's records, flags the
//! list as compacting, and calls the compactor, which edits the list in
//! place. A kept compaction retries the round without counting it, and a
//! round compacts at most once, so a retry that overflows again puts the
//! snapshot back and raises `context_exhausted`.

use std::collections::BTreeMap;

use runvil_types::metrics::ToolCallEvent;

use crate::compactors::{OverflowReason, estimate_tokens};
use crate::error::Error;
use crate::error_value::{ErrorField, ErrorKind, ErrorValue, Raised};
use crate::messages::{MessageList, Snapshot};
use crate::projection::project_messages;
use crate::protocol::{
    ChatOutcome, ChatResult, MessageContent, MessageRecord, RecordRole, ToolCallRecord,
};

/// The message the empty-reply rule raises when the answer names no
/// empty detail.
const EMPTY_MODEL_REPLY: &str = "empty model reply";

/// The message the round cap raises.
const NOT_CONVERGED: &str = "tool-call loop did not converge";

/// The message raised for an input the machine's phase does not await.
const UNEXPECTED_INPUT: &str =
    "internal invariant violated: the models.loop step received an input its phase does not await";

/// One loop call: its rules' state and the values it passes along.
#[derive(Debug)]
pub(super) struct Machine<V> {
    /// The round cap: the run's `max_tool_iterations`.
    max_rounds: usize,
    /// The rounds started so far, the current one included.
    rounds: usize,
    /// The calls answered in this loop call, a tool's own failure
    /// included.
    answered: usize,
    /// The list every record the loop adds is pushed onto.
    list: MessageList,
    /// The same list as the value every `chat` request names.
    messages: V,
    /// The receiver of a handle's `loop`, set on every `chat` request;
    /// `None` for `models.loop`.
    handle: Option<V>,
    /// The compactor argument, or `compactors.fail` as read when the
    /// call began, which is not type-checked, as in the shim.
    compactor: V,
    /// The current batch's turn, sent back on every `tool_calls` and
    /// `tool_call` request.
    turn: u32,
    /// The current batch's calls, in the order the model listed them.
    calls: Vec<ToolCallEvent>,
    /// The current batch's results so far, one per answered call.
    results: Vec<String>,
    /// The records from before this round's compaction, held until the
    /// retried round's answer arrives. Set means the round has compacted.
    snapshot: Option<Snapshot>,
    /// What the machine waits on.
    phase: Phase<V>,
}

/// What the machine waits on, which names the values the adapter reads
/// next.
#[derive(Debug)]
pub(super) enum Phase<V> {
    /// The `chat` answer: `(ok, round)`.
    Chatting,
    /// The compactor's raw `pcall` after an overflow for this reason:
    /// `(ok, value)`.
    Compacting(OverflowReason),
    /// The `tool_calls` answer for the batch's unanswered calls:
    /// `(ok, texts)`.
    Batching,
    /// The `tool_call` answer for the call the scheduler stopped at:
    /// `(ok, result, handler, args)`.
    Calling,
    /// The handler's raw `pcall`: `(ok, value)`.
    Handling,
    /// The `adhoc_tool_done` answer: `(ok, text)`, with the handler's own
    /// raised value when it raised.
    Reporting(Option<V>),
    /// The loop returned or raised.
    Done,
}

/// One typed input: the last request's answer, or the last call's
/// outcome.
#[derive(Debug)]
pub(super) enum Input<V> {
    /// The `chat` answer, or the value to raise.
    Answered(std::result::Result<Box<ChatResult>, V>),
    /// A batch's texts, one per call the scheduler issued, or the value
    /// to raise.
    Batched(std::result::Result<Vec<String>, V>),
    /// A bound tool's text, or the value to raise.
    Dispatched(std::result::Result<String, V>),
    /// An adhoc tool's handler and its arguments table.
    Adhoc { handler: V, args: V },
    /// The compactor's or the handler's raw `pcall`: its first returned
    /// or raised value, and whether the run's cancel flag was set.
    Called {
        outcome: std::result::Result<V, V>,
        cancelled: bool,
    },
    /// The `adhoc_tool_done` answer: the call's text, or the value to
    /// raise.
    Reported(std::result::Result<String, V>),
}

/// What runs next, after the records the step pushed.
#[derive(Debug)]
pub(super) enum Then<V> {
    /// Yield `{ op = "chat", messages = messages, handle = handle }`.
    Chat { messages: V, handle: Option<V> },
    /// Yield `{ op = "tool_calls", calls = { { alias = call.name, args =
    /// call.arguments, call_id = call.id }, ... }, turn = turn }`.
    ToolCalls {
        calls: Vec<ToolCallEvent>,
        turn: u32,
    },
    /// Yield `{ op = "tool_call", alias = call.name, args = call.arguments,
    /// call_id = call.id, turn = turn }`.
    ToolCall { call: Box<ToolCallEvent>, turn: u32 },
    /// Call the compactor with the reason's tag.
    Compact {
        compactor: V,
        reason: OverflowReason,
    },
    /// Call the handler with its arguments table.
    Handle { handler: V, args: V },
    /// Yield `{ op = "adhoc_tool_done", ok = true, value = value }` when
    /// the handler returned, or `{ op = "adhoc_tool_done", ok = false }`
    /// when it raised.
    Report(Option<V>),
    /// Return nil.
    Return,
    /// Raise this value unchanged.
    Raise(V),
    /// Raise a compactor's failure through `normalize_failure`.
    RaiseNormalized(V),
    /// Raise a new error table.
    RaiseNew(Raised),
}

impl<V: Clone> Machine<V> {
    /// A loop over `list`, capped at `max_rounds` rounds, and its first
    /// step: the first round's `chat`, or `tool_loop_exhausted` when the
    /// cap is 0.
    pub(super) fn begin(
        max_rounds: usize,
        list: MessageList,
        messages: V,
        handle: Option<V>,
        compactor: V,
    ) -> (Self, Then<V>) {
        let mut machine = Machine {
            max_rounds,
            rounds: 0,
            answered: 0,
            list,
            messages,
            handle,
            compactor,
            turn: 0,
            calls: Vec::new(),
            results: Vec::new(),
            snapshot: None,
            phase: Phase::Done,
        };
        let first = machine.next_round();
        (machine, first)
    }

    /// What the machine waits on.
    pub(super) fn phase(&self) -> &Phase<V> {
        &self.phase
    }

    /// Applies one input, pushes the records it settles, and returns what
    /// runs next. A reply is read as the renderer reads it, an empty
    /// string as no reply. An input other than the one
    /// [`Machine::phase`] names is a crate bug and raises an `internal`
    /// error.
    pub(super) fn step(&mut self, input: Input<V>) -> Then<V> {
        match (std::mem::replace(&mut self.phase, Phase::Done), input) {
            (Phase::Chatting, Input::Answered(answer)) => {
                let compacted = self.snapshot.take();
                match answer {
                    Ok(round) => self.judge(*round, compacted),
                    Err(failure) => Then::Raise(failure),
                }
            }
            (Phase::Compacting(reason), Input::Called { outcome, cancelled }) => {
                let Some(snapshot) = self.snapshot.take() else {
                    self.list.set_compacting(false);
                    return Then::RaiseNew(raised(ErrorKind::Internal, UNEXPECTED_INPUT));
                };
                match settle(&self.list, snapshot, outcome, cancelled, Some(reason)) {
                    Ok(snapshot) => {
                        self.snapshot = Some(snapshot);
                        self.chat()
                    }
                    Err(refusal) => refusal,
                }
            }
            (Phase::Batching, Input::Batched(Ok(texts))) => self.take_batch(texts),
            (Phase::Calling, Input::Dispatched(Ok(text)))
            | (Phase::Reporting(None), Input::Reported(Ok(text))) => {
                self.results.push(text);
                self.next_call()
            }
            (Phase::Calling, Input::Adhoc { handler, args }) => {
                self.phase = Phase::Handling;
                Then::Handle { handler, args }
            }
            (Phase::Handling, Input::Called { outcome, cancelled }) => match outcome {
                Err(failure) if cancelled => Then::Raise(failure),
                Ok(value) => {
                    self.phase = Phase::Reporting(None);
                    Then::Report(Some(value))
                }
                Err(failure) => {
                    self.phase = Phase::Reporting(Some(failure));
                    Then::Report(None)
                }
            },
            (Phase::Reporting(Some(failure)), Input::Reported(_))
            | (Phase::Batching, Input::Batched(Err(failure)))
            | (Phase::Calling, Input::Dispatched(Err(failure)))
            | (Phase::Reporting(None), Input::Reported(Err(failure))) => Then::Raise(failure),
            _ => Then::RaiseNew(raised(ErrorKind::Internal, UNEXPECTED_INPUT)),
        }
    }

    /// Starts the next round with its `chat`, or raises
    /// `tool_loop_exhausted` once the cap's rounds have all started.
    fn next_round(&mut self) -> Then<V> {
        if self.rounds >= self.max_rounds {
            return Then::RaiseNew(raised(ErrorKind::ToolLoopExhausted, NOT_CONVERGED));
        }
        self.rounds += 1;
        self.chat()
    }

    /// Sends the list in a `chat` without counting a round: a new round's
    /// send, or a compacted round's retry.
    fn chat(&mut self) -> Then<V> {
        self.phase = Phase::Chatting;
        Then::Chat {
            messages: self.messages.clone(),
            handle: self.handle.clone(),
        }
    }

    /// Judges a round's answer: an overflow goes to [`Machine::overflow`]
    /// with the snapshot a compacted round holds, tool calls start a
    /// batch, a reply is appended and the loop returns, an empty reply
    /// that finished with `stop` after an answered call is the clean exit,
    /// and anything else raises `empty_model_reply`.
    fn judge(&mut self, round: ChatResult, compacted: Option<Snapshot>) -> Then<V> {
        let detail = match round.outcome {
            ChatOutcome::Overflow(reason) => return self.overflow(reason, compacted),
            ChatOutcome::ToolCalls(calls) => {
                self.turn = round.turn;
                self.calls = calls;
                self.results.clear();
                return self.next_call();
            }
            ChatOutcome::Reply(reply) if !reply.is_empty() => {
                let record = text_record(RecordRole::Assistant, reply);
                return self.push(vec![record], |_| Then::Return);
            }
            ChatOutcome::Reply(_) => None,
            ChatOutcome::Empty(detail) => detail,
        };
        if round.finish_reason.as_deref() == Some("stop") && self.answered > 0 {
            let record = text_record(RecordRole::Assistant, String::new());
            return self.push(vec![record], |_| Then::Return);
        }
        let mut empty = raised(ErrorKind::EmptyModelReply, EMPTY_MODEL_REPLY);
        if let Some(detail) = detail {
            empty.message = detail;
        }
        if let Some(reason) = round.finish_reason {
            empty
                .fields
                .insert("finish_reason".to_owned(), ErrorField::String(reason));
        }
        Then::RaiseNew(empty)
    }

    /// Starts the round's compaction: snapshots the records, flags the
    /// list as compacting, and calls the compactor with `reason`. A round
    /// that already compacted puts its snapshot back and raises
    /// `context_exhausted` instead.
    fn overflow(&mut self, reason: OverflowReason, compacted: Option<Snapshot>) -> Then<V> {
        if let Some(snapshot) = compacted {
            self.list.restore(snapshot);
            return Then::RaiseNew(exhausted(reason));
        }
        self.snapshot = Some(self.list.snapshot());
        self.list.set_compacting(true);
        self.phase = Phase::Compacting(reason);
        Then::Compact {
            compactor: self.compactor.clone(),
            reason,
        }
    }

    /// Takes a batch answer's texts, one per call the scheduler issued.
    /// More texts than unanswered calls is a crate bug. Fewer means the
    /// scheduler stopped at the next call, which goes out alone.
    fn take_batch(&mut self, texts: Vec<String>) -> Then<V> {
        let unanswered = self.calls.len() - self.results.len();
        if texts.len() > unanswered {
            return Then::RaiseNew(raised(ErrorKind::Internal, UNEXPECTED_INPUT));
        }
        let short = texts.len() < unanswered;
        self.results.extend(texts);
        match self.calls.get(self.results.len()) {
            Some(call) if short => {
                self.phase = Phase::Calling;
                Then::ToolCall {
                    call: Box::new(call.clone()),
                    turn: self.turn,
                }
            }
            _ => self.next_call(),
        }
    }

    /// The batch's unanswered calls as one `tool_calls`, or the batch's
    /// records and the next round once every call has its result.
    fn next_call(&mut self) -> Then<V> {
        let unanswered = self.calls.get(self.results.len()..).unwrap_or_default();
        if !unanswered.is_empty() {
            self.phase = Phase::Batching;
            return Then::ToolCalls {
                calls: unanswered.to_vec(),
                turn: self.turn,
            };
        }
        let calls = std::mem::take(&mut self.calls);
        let results = std::mem::take(&mut self.results);
        self.answered += calls.len();
        let mut assistant = text_record(RecordRole::Assistant, String::new());
        assistant.tool_calls = calls
            .iter()
            .map(|call| ToolCallRecord {
                id: call.id.clone(),
                name: call.name.clone(),
                arguments: call.arguments.clone(),
            })
            .collect();
        let mut records = vec![assistant];
        records.extend(calls.into_iter().zip(results).map(|(call, result)| {
            text_record(
                RecordRole::Tool {
                    tool_call_id: call.id,
                },
                result,
            )
        }));
        self.push(records, Self::next_round)
    }

    /// Pushes `records` onto the list in order, then runs `next`. The list
    /// refuses only a system record after a non-system one, which the loop
    /// never builds, so a refusal is a crate bug and raises `internal`.
    fn push(
        &mut self,
        records: Vec<MessageRecord>,
        next: impl FnOnce(&mut Self) -> Then<V>,
    ) -> Then<V> {
        for record in records {
            if let Err(refused) = self.list.push(record) {
                self.phase = Phase::Done;
                return Then::RaiseNew(raised(ErrorKind::Internal, &refused.to_string()));
            }
        }
        next(self)
    }
}

/// Settles a compactor's raw `pcall` outcome over `list`, which held
/// `snapshot` when the compactor started, and clears the list's
/// `compacting` flag. A raise passes on, raw under cancellation and else
/// normalized. A return is checked against the projection's structural
/// rules, and after an `overflow` against the shrink rule: the projected
/// estimate must fall below the snapshot's. A raise or a broken rule puts
/// the snapshot back; a kept compaction hands the snapshot back.
pub(super) fn settle<V>(
    list: &MessageList,
    snapshot: Snapshot,
    outcome: std::result::Result<V, V>,
    cancelled: bool,
    overflow: Option<OverflowReason>,
) -> std::result::Result<Snapshot, Then<V>> {
    list.set_compacting(false);
    let refusal = match outcome {
        Err(failure) if cancelled => Then::Raise(failure),
        Err(failure) => Then::RaiseNormalized(failure),
        Ok(_) => match check(list, &snapshot, overflow) {
            Ok(()) => return Ok(snapshot),
            Err(refused) => Then::RaiseNew(refused),
        },
    };
    list.restore(snapshot);
    Err(refusal)
}

/// Checks a compacted `list` against the rules [`settle`] names: the
/// projection's refusal for a broken structural rule, and
/// `context_exhausted` for `overflow` when the list did not shrink.
fn check(
    list: &MessageList,
    snapshot: &Snapshot,
    overflow: Option<OverflowReason>,
) -> std::result::Result<(), Raised> {
    let projected = project_messages(&list.to_records())
        .map_err(|refused| raised(refused.kind(), &refused.to_string()))?;
    let Some(reason) = overflow else {
        return Ok(());
    };
    // The overflowed round projected these records, so this projection
    // cannot fail.
    let before =
        project_messages(&snapshot.to_records()).map_or(u64::MAX, |sent| estimate_tokens(&sent));
    if estimate_tokens(&projected) < before {
        Ok(())
    } else {
        Err(exhausted(reason))
    }
}

/// A new `context_exhausted` error for `reason`, as `compactors.fail`
/// raises it.
fn exhausted(reason: OverflowReason) -> Raised {
    let error = Error::ContextExhausted {
        reason: reason.tag().to_owned(),
    };
    Raised {
        kind: error.kind(),
        message: error.to_string(),
        fields: error.fields().into_iter().collect(),
    }
}

/// A record of `role` holding the plain text `content` and no `meta`.
fn text_record(role: RecordRole, content: String) -> MessageRecord {
    MessageRecord {
        role,
        content: MessageContent::Text(content),
        tool_calls: Vec::new(),
        meta: None,
    }
}

/// A new error of `kind` with `message` and no fields.
pub(super) fn raised(kind: ErrorKind, message: &str) -> Raised {
    Raised {
        kind,
        message: message.to_owned(),
        fields: BTreeMap::new(),
    }
}

#[cfg(test)]
#[path = "machine-tests.rs"]
mod tests;
