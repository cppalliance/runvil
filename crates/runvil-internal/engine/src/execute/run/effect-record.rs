//! The records a run log stores: an [`EffectRecord`] for each effect and
//! an [`AnswerRecord`] for each answer, with the failure, chat, and tool
//! answer records they hold, and [`PackToolSets`], one Pack's tools for
//! each set of a section entry, which a render effect, its answer, and
//! both their records hold.

use runvil_types::ids::RoundId;
use runvil_types::packs::PackId;
use runvil_types::tools::{OutputTrust, ToolCallOrigin, ToolId, ToolInfo};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::execute::protocol::{VfsOp, VfsOutcome};
use crate::model::{Completion, CompletionResult};

/// One catalog Pack's tools for each set of a section entry, in order: the
/// tools the section offered from the Pack, or the tools the Pack's render
/// returned for them. A set the section did not offer the Pack into is
/// empty. A render effect and its answer hold each tool's [`ToolInfo`],
/// and the effect's record holds each tool's [`ToolId`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PackToolSets<T> {
    /// The Pack.
    pub pack: PackId,
    /// The Pack's tools for the Static Set, which every model round of the
    /// section entry advertises.
    pub static_set: Vec<T>,
    /// The Pack's tools for the Dynamic Set, which the model reaches
    /// through the Engine's two discovery tools.
    pub dynamic_set: Vec<T>,
}

impl<T> PackToolSets<T> {
    /// Builds the Pack `pack`'s tools: `static_set` for the Static Set and
    /// `dynamic_set` for the Dynamic Set.
    #[must_use]
    pub fn new(pack: PackId, static_set: Vec<T>, dynamic_set: Vec<T>) -> PackToolSets<T> {
        PackToolSets {
            pack,
            static_set,
            dynamic_set,
        }
    }
}

/// An [`Effect`](super::Effect) with its live handles removed, as a run
/// log stores it. A replay compares each re-issued effect against its
/// record.
///
/// The `Chat` record keeps only what identifies the round: its id, the
/// alias of the model slot it ran under, and the frozen invocation
/// settings (temperature, generation cap, and thinking switch). It stores
/// the request's messages in wire form after the first `keep`, which repeat
/// round `after`'s request. So a caller that logs records must record every
/// Chat effect a step returns, including ones it drops, for each `after` to
/// name a recorded round. It names the slot by alias, because the
/// caller may serve the slot with a different model from the bound one.
/// The answer's [`ChatAnswerRecord`] names the model that served the round.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum EffectRecord {
    /// One model round.
    Chat {
        /// The round's run-wide id, which the round's content events also
        /// carry.
        round: RoundId,
        /// The prompt-local alias of the slot the round ran under.
        alias: String,
        /// The earlier round whose request this one extends, or `None`
        /// when the request starts here.
        after: Option<RoundId>,
        /// How many leading messages of `after`'s request this request
        /// repeats; 0 when `after` is `None`.
        keep: u64,
        /// The request's messages after the first `keep`, one wire-form
        /// message per entry.
        messages: Vec<Value>,
        /// The advertised tool names, in schema order.
        tools: Vec<String>,
        /// The frozen sampling temperature, when the bind declared one.
        temperature: Option<f64>,
        /// The frozen generation cap, when the bind declared one.
        max_tokens: Option<u32>,
        /// The frozen thinking switch, when the bind declared one.
        thinking: Option<bool>,
    },
    /// One render of a section entry's offered catalog Packs. The answer's
    /// record holds the rendered tools whole.
    Render {
        /// The prompt-local alias of the slot the entry's first round runs
        /// under.
        alias: String,
        /// The section entry, the value its Lua reads as `sys.id`.
        entry: String,
        /// Each offered Pack once, in the order its first tool was offered,
        /// with the ids of the tools the section offered from it into each
        /// set.
        offered: Vec<PackToolSets<ToolId>>,
    },
    /// One call to a Pack's tool, a run catalog tool or a tool only the
    /// model sees.
    ToolCall {
        /// The tool's stable live identity.
        tool: ToolId,
        /// The tool's wire name for a model call, or its id for a script call.
        alias: String,
        /// The call's arguments.
        args: Value,
        /// Who made the call: the run, the section and its entry, and the
        /// section's script or the model whose round asked for it.
        origin: ToolCallOrigin,
    },
    /// One operation on the run's store view.
    Vfs {
        /// The validated operation.
        op: VfsOp,
    },
    /// One sleep.
    Timer {
        /// The duration in seconds.
        seconds: f64,
    },
}

/// An [`EffectAnswer`](super::EffectAnswer) as a run log stores it: one
/// variant per answer kind, each holding its outcome with every failure
/// recorded as a [`FailureRecord`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum AnswerRecord {
    /// The model round's outcome.
    Chat(std::result::Result<ChatAnswerRecord, FailureRecord>),
    /// Each Pack's rendered tools per set, in the order asked, whole,
    /// because the `Chat` record keeps only tool names. The section's
    /// adhoc tools and the discovery tools join later, inside the Engine,
    /// so they are not here.
    Render(Vec<PackToolSets<ToolInfo>>),
    /// The tool call's outcome.
    ToolCall(std::result::Result<ToolAnswerRecord, FailureRecord>),
    /// The outcome of the operation on the run's store view: the
    /// [`VfsOutcome`] itself on success, or the failure's record.
    Vfs(std::result::Result<VfsOutcome, FailureRecord>),
    /// The timer fired.
    Timer,
    /// The caller gave up on the effect and dropped it.
    Dropped,
}

/// A failed answer as the log records it, serialized as
/// `{"kind": .., "message": ..}`.
///
/// `kind` is the stable tag of the failure's kind: the `CompletionErrorKind`
/// tag for a model round (such as `malformed_response`), the
/// `ToolErrorKind` tag for a tool call (such as `invalid_arguments`), or
/// the `VfsError` tag for a store operation (such as `not_found`).
/// `message` is the failure's display text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct FailureRecord {
    /// The stable tag of the failure's kind.
    pub kind: String,
    /// The failure's display text.
    pub message: String,
}

impl FailureRecord {
    /// Builds the record of a failure of kind `kind` with `message` as its
    /// display text.
    #[must_use]
    pub(crate) fn new(kind: impl Into<String>, message: impl Into<String>) -> FailureRecord {
        FailureRecord {
            kind: kind.into(),
            message: message.into(),
        }
    }
}

/// A completed model round as the log records it: what identifies the
/// answer. The round's request and response bodies travel separately, as
/// debug events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ChatAnswerRecord {
    /// The model that served the round, as the completion names it.
    pub model: String,
    /// The provider's finish reason, when it sent one.
    pub finish_reason: Option<String>,
    /// The reply text, when the round produced text.
    pub reply: Option<String>,
    /// The names of the tools the model requested, in call order, when it
    /// requested any.
    pub tool_calls: Vec<String>,
}

impl ChatAnswerRecord {
    /// Builds the record of a round `model` served, with no finish reason,
    /// no reply, and no tool calls.
    #[must_use]
    pub fn new(model: impl Into<String>) -> ChatAnswerRecord {
        ChatAnswerRecord {
            model: model.into(),
            finish_reason: None,
            reply: None,
            tool_calls: Vec::new(),
        }
    }

    /// Sets the provider's finish reason.
    #[must_use]
    pub fn with_finish_reason(mut self, finish_reason: Option<String>) -> ChatAnswerRecord {
        self.finish_reason = finish_reason;
        self
    }

    /// Sets the reply text.
    #[must_use]
    pub fn with_reply(mut self, reply: Option<String>) -> ChatAnswerRecord {
        self.reply = reply;
        self
    }

    /// Sets the names of the tools the model requested, in call order.
    #[must_use]
    pub fn with_tool_calls(mut self, tool_calls: Vec<String>) -> ChatAnswerRecord {
        self.tool_calls = tool_calls;
        self
    }
}

impl From<&Completion> for ChatAnswerRecord {
    fn from(completion: &Completion) -> Self {
        let (reply, tool_calls) = match completion.result() {
            CompletionResult::Text(text) => (Some(text.clone()), Vec::new()),
            CompletionResult::ToolCalls(calls) => (
                None,
                calls.iter().map(|call| call.name().to_owned()).collect(),
            ),
            // The vocabulary is `#[non_exhaustive]`; a variant this crate
            // does not know records as a round with neither product.
            _ => (None, Vec::new()),
        };
        ChatAnswerRecord {
            model: completion.model().to_owned(),
            finish_reason: completion.finish_reason().map(str::to_owned),
            reply,
            tool_calls,
        }
    }
}

/// A tool's own output as the log records it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ToolAnswerRecord {
    /// The output text, before the Engine's trust rules apply.
    pub text: String,
    /// The trust level the tool declared for its output.
    pub trust: OutputTrust,
}

impl ToolAnswerRecord {
    /// Builds the record of an output of `text` at `trust`.
    #[must_use]
    pub(crate) fn new(text: impl Into<String>, trust: OutputTrust) -> ToolAnswerRecord {
        ToolAnswerRecord {
            text: text.into(),
            trust,
        }
    }
}
