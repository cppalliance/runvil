//! The compactor surface: overflow reasons, the shipped policies, and the
//! pre-dispatch precheck.
//!
//! A model request can fail to fit the model's context window twice: the
//! pre-dispatch [`precheck`] can count the projected conversation, plus the
//! room kept for the reply, past the window before anything leaves, and the
//! broker can report the request
//! as too large (a `ContextOverflow` failure, which the `chat` arm turns
//! into the provider overflow). Either path calls the selected compactor
//! with the [`OverflowReason`]'s tag, and the compactor edits the section's
//! `messages` list in place; the loop then checks that the list shrank and
//! retries the round. The shipped [`Compactor`] policies are
//! `compactors.fail`, the omitted-compactor default, which always raises
//! typed context exhaustion ([`Error::ContextExhausted`]), and
//! `compactors.clear`, which removes every record and works only inside a
//! compactor.
//!
//! The surface sits in this crate for the same reason the projection does:
//! it owns the message records, the `chat` arm's precheck and overflow
//! classification depend on it, and the loop's compactor call runs in Lua
//! over the `compactors` global installed here.

use std::borrow::Cow;
use std::num::NonZeroU32;

use mlua::{Function, Lua, Table};
use runvil_model_client::client::{Message, Role};
use runvil_model_client::detail::{message_content_value, message_raw_tool_calls};
use runvil_types::metrics::Usage;
use serde_json::Value;

use crate::error::{Error, Result};
use crate::messages::MessageList;

/// Why a request cannot fit the model's context window.
///
/// These are the two overflow tags the loop passes to a compactor, which
/// [`exhaustion_detail`] turns into a description. A compactor may also
/// receive a caller's own reason through `models.compact`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverflowReason {
    /// The pre-dispatch estimate exceeds the model's context window; the
    /// check ran before any request was sent.
    Precheck,
    /// The provider rejected the request as too large for the model's
    /// context window.
    Provider,
}

impl OverflowReason {
    /// Parses the invocation tag the compactor callback receives.
    pub(crate) fn from_tag(tag: &str) -> Option<OverflowReason> {
        match tag {
            "precheck" => Some(OverflowReason::Precheck),
            "provider" => Some(OverflowReason::Provider),
            _ => None,
        }
    }

    /// The invocation tag the compactor callback receives, also the
    /// `reason` field of a `context_exhausted` error table.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            OverflowReason::Precheck => "precheck",
            OverflowReason::Provider => "provider",
        }
    }
}

impl std::fmt::Display for OverflowReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let detail = match self {
            OverflowReason::Precheck => {
                "the request precheck overflowed the model's context window"
            }
            OverflowReason::Provider => {
                "the provider rejected the request as exceeding the model's context window"
            }
        };
        formatter.write_str(detail)
    }
}

/// An overflow tag's description, or any other reason as given.
///
/// This is the detail a context exhaustion's message shows: an exact
/// [`OverflowReason::tag`] reads as that overflow's [`Display`](std::fmt::Display),
/// and a caller's own reason reads as itself.
#[must_use]
pub fn exhaustion_detail(reason: &str) -> Cow<'_, str> {
    match OverflowReason::from_tag(reason) {
        Some(overflow) => Cow::Owned(overflow.to_string()),
        None => Cow::Borrowed(reason),
    }
}

/// The compactor policies the `compactors` global ships.
///
/// `Fail` is the omitted-compactor default: invoked with any reason, it
/// always raises typed context exhaustion. `Clear` removes every record,
/// so a section can start its conversation over without leaving, and
/// refuses unless a compactor is running over the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Compactor {
    /// `compactors.fail`: always raises [`Error::ContextExhausted`].
    #[default]
    Fail,
    /// `compactors.clear`: removes every record from the list, leaving its
    /// last send and its rendered tool list. It works only inside a
    /// compactor.
    Clear,
}

impl Compactor {
    /// Invokes the policy on `list` with `reason`, an overflow's tag or a
    /// caller's own.
    ///
    /// # Errors
    /// `Fail` always returns [`Error::ContextExhausted`] holding `reason`;
    /// `Clear` returns [`Error::Lua`] when no compactor is running over
    /// `list`, and leaves it unchanged.
    pub fn invoke(self, list: &MessageList, reason: &str) -> Result<()> {
        match self {
            Compactor::Fail => Err(Error::ContextExhausted {
                reason: reason.to_owned(),
            }),
            Compactor::Clear if !list.compacting() => Err(Error::Lua(
                "compactors.clear works only inside a compactor; empty messages with \
                 models.compact(compactors.clear, reason)"
                    .to_owned(),
            )),
            Compactor::Clear => {
                list.clear();
                Ok(())
            }
        }
    }
}

/// Rough characters-per-token divisor for the pre-dispatch estimate.
///
/// The estimate only ever gates the compactor invocation, so a conservative
/// heuristic suffices: four characters per token tracks prose and code
/// closely enough that a conversation well under the window passes and one
/// well over it compacts.
const CHARS_PER_TOKEN: u64 = 4;

/// Per-message framing overhead, in tokens: the role, separators, and
/// tool-call envelope a serialized message adds beyond its text.
const MESSAGE_OVERHEAD_TOKENS: u64 = 4;

/// The default output reserve never exceeds this many tokens, however
/// large the window is.
const DEFAULT_RESERVE_CAP: u32 = 8192;

/// The tokens the precheck keeps free in the window for the model's reply.
///
/// This is the binding's `max_tokens` when it sets one, otherwise one
/// eighth of the window capped at 8,192 tokens. Either way it never
/// exceeds half the window: nothing checks `max_tokens` against the
/// window, and a raw model id carries a guessed window, so without the cap
/// a `max_tokens` at or above the window would refuse every request, an
/// empty one included. The provider's own overflow reply is the backstop
/// for a request the cap lets through.
#[must_use]
pub fn output_reserve(context: NonZeroU32, max_tokens: Option<NonZeroU32>) -> u32 {
    let window = context.get();
    let wanted = max_tokens.map_or_else(|| (window / 8).min(DEFAULT_RESERVE_CAP), NonZeroU32::get);
    wanted.min(window / 2)
}

/// What a served round taught the precheck about its conversation: the
/// projected messages the round sent, and the tokens the provider counted
/// for them plus the reply.
///
/// The next request counts from it when it extends the sent messages with
/// the reply (see [`precheck`]). It keeps the sent messages themselves, not
/// a hash, so the prefix test is exact: a rewritten, compacted, or merged
/// history can never anchor on a count that was measured for another
/// conversation.
#[derive(Debug, Clone)]
pub struct UsageAnchor {
    sent: Vec<Message>,
    tokens: u64,
}

impl UsageAnchor {
    /// Anchors on `sent`, the messages a round carried, and the `usage` the
    /// provider reported for that round.
    ///
    /// The count is the prompt plus the completion tokens, less any
    /// reported reasoning tokens: the history never resends reasoning, so
    /// counting it would overflow early. The total already covers the
    /// reply, so the reply is never estimated again.
    #[must_use]
    pub fn new(sent: Vec<Message>, usage: &Usage) -> UsageAnchor {
        let total = u64::from(usage.prompt_tokens) + u64::from(usage.completion_tokens);
        let tokens = total.saturating_sub(u64::from(usage.reasoning_tokens.unwrap_or(0)));
        UsageAnchor { sent, tokens }
    }

    /// The anchored token count: the request's prompt plus its reply.
    #[must_use]
    pub fn tokens(&self) -> u64 {
        self.tokens
    }

    /// The request's token count measured from this anchor, or `None` when
    /// the request does not extend the sent messages with an assistant
    /// reply. The count is the anchor plus the estimate of the messages
    /// after the reply.
    fn count(&self, messages: &[Message]) -> Option<u64> {
        let sent = self.sent.len();
        let reply = messages.get(sent)?;
        if reply.role() != Role::Assistant || messages[..sent] != self.sent[..] {
            return None;
        }
        Some(self.tokens + estimate_tokens(&messages[sent + 1..]))
    }
}

/// The pre-dispatch context precheck: count the request's prompt tokens
/// from the projected wire messages and refuse the dispatch when the count
/// plus the `reserve` kept for the reply exceeds the model's context
/// window. A count plus reserve equal to the window passes.
///
/// With an `anchor`, the count starts from the provider's own token numbers
/// when the request has more messages than the anchor sent, its first
/// messages equal the sent ones, and the next message is an assistant
/// message, the reply the anchor already covers. The count is then the
/// anchor plus the estimate of the messages after that reply. Any other
/// request, and a call with no anchor, counts by the full estimate.
///
/// The estimate counts message text and tool-call argument characters; an
/// image part contributes nothing (its token cost bears no relation to the
/// data-URI length), and tool schemas and the system template are not
/// counted, so an image-heavy conversation can still overflow at the
/// provider - the provider-overflow path covers it.
///
/// # Errors
/// Returns [`OverflowReason::Precheck`] when the count plus the reserve
/// exceeds the window.
pub fn precheck(
    messages: &[Message],
    context: NonZeroU32,
    reserve: u32,
    anchor: Option<&UsageAnchor>,
) -> std::result::Result<(), OverflowReason> {
    let count = anchor
        .and_then(|anchor| anchor.count(messages))
        .unwrap_or_else(|| estimate_tokens(messages));
    if count.saturating_add(u64::from(reserve)) > u64::from(context.get()) {
        return Err(OverflowReason::Precheck);
    }
    Ok(())
}

/// The request's estimated prompt tokens: text characters over
/// [`CHARS_PER_TOKEN`], plus per-message framing overhead. A compaction
/// after an overflow must bring this below the estimate it started from.
pub(crate) fn estimate_tokens(messages: &[Message]) -> u64 {
    let chars: u64 = messages.iter().map(message_chars).sum();
    chars / CHARS_PER_TOKEN + MESSAGE_OVERHEAD_TOKENS * messages.len() as u64
}

/// One message's estimated character weight: its text (a plain string, or
/// the text parts of a multimodal array) plus its serialized tool calls.
fn message_chars(message: &Message) -> u64 {
    let content = match message_content_value(message) {
        Value::String(text) => text.len() as u64,
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .map(|text| text.len() as u64)
            .sum(),
        _ => 0,
    };
    let calls: u64 = message_raw_tool_calls(message).map_or(0, |calls| {
        calls.iter().map(|call| call.to_string().len() as u64).sum()
    });
    content + calls
}

/// Installs the `compactors` global holding the shipped policies over the
/// section's `list`.
///
/// Each policy is a Rust-backed function taking one reason string, the
/// overflow's tag or the caller's own. `compactors.fail` raises typed
/// context exhaustion holding that reason as an external error, so
/// the [`Error::ContextExhausted`] value crosses the Lua boundary
/// downcastable rather than flattened to text. The loop, which calls the
/// selected compactor on an overflow round, normalizes that raise into
/// the structured error table before re-raising it, so the kind reaches
/// author code and the Engine alike; a direct call the block does not
/// catch keeps the kind through the block's failure mapping, so the run
/// ends as context exhaustion either way. `compactors.clear` empties `list`,
/// and only while a compactor runs over it.
/// The namespace installs during Engine injection, right after the
/// section's `messages` list.
///
/// # Errors
/// Returns [`Error::Lua`] if a function or the global install fails.
pub(crate) fn install_compactors(lua: &Lua, globals: &Table, list: &MessageList) -> Result<()> {
    let compactors = lua.create_table().map_err(Error::lua)?;
    for (name, policy) in [("fail", Compactor::Fail), ("clear", Compactor::Clear)] {
        let list = list.clone();
        let function: Function = lua
            .create_function(move |_lua, reason: String| {
                policy.invoke(&list, &reason).map_err(mlua::Error::external)
            })
            .map_err(Error::lua)?;
        compactors.raw_set(name, function).map_err(Error::lua)?;
    }
    globals
        .raw_set("compactors", compactors)
        .map_err(Error::lua)
}

#[cfg(test)]
#[path = "compactors-tests.rs"]
mod tests;
