//! The answer records: an effect's answer as a run log stores it.

use runvil_types::tools::OutputTrust;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::execute::protocol::VfsOutcome;
use crate::model::{Completion, CompletionResult};

#[cfg(doc)]
use super::EffectAnswer;

/// An [`EffectAnswer`] as a run log stores it: one variant per answer
/// kind, each holding its outcome with every failure recorded as a
/// [`FailureRecord`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum AnswerRecord {
    /// The model round's outcome.
    Chat(std::result::Result<ChatAnswerRecord, FailureRecord>),
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
    pub fn new(kind: impl Into<String>, message: impl Into<String>) -> FailureRecord {
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
    // Declared first: serde writes fields in declaration order, and the
    // record's keys stay sorted so its JSON matches its `Value` form.
    /// The structured payload the tool attached beside its text, which a
    /// calling script received in place of the text. Absent from the JSON
    /// when the tool attached none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured: Option<Value>,
    /// The output text, before the Engine's trust rules apply.
    pub text: String,
    /// The trust level the tool declared for its output.
    pub trust: OutputTrust,
}

impl ToolAnswerRecord {
    /// Builds the record of an output of `text` at `trust`.
    #[must_use]
    pub fn new(text: impl Into<String>, trust: OutputTrust) -> ToolAnswerRecord {
        ToolAnswerRecord {
            text: text.into(),
            trust,
            structured: None,
        }
    }

    /// Returns this record with `value` as the tool's structured payload.
    #[must_use]
    pub fn with_structured(self, value: Value) -> ToolAnswerRecord {
        ToolAnswerRecord {
            structured: Some(value),
            ..self
        }
    }
}
