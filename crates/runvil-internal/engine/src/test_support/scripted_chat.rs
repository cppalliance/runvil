//! The suites' chat client: a scripted model played in process.
//!
//! The Engine emits each round as a `Chat` effect for the caller to perform.
//! [`ScriptedChat`] answers each round from a fixed script instead, with
//! no HTTP and no wire code, and records what every round carried so a
//! suite asserts on the effect rather than on a request body. The reply
//! constructors beside it - `text`, `text_finish`, `calls`, and `fail` -
//! write the script, and they stay crate-private because only the in-crate
//! suites use them; the bench builds its one reply from the variant
//! directly. The bench answers its rounds from the same model through the
//! `test-support` feature.

use std::sync::{Arc, Mutex, PoisonError};

use runvil_model_client::client::{Completion, CompletionResult, Message, ToolCall, ToolSchema};
use runvil_model_client::model::{CompletionError, CompletionErrorKind, CompletionOptions};
use runvil_types::metrics::CallMetrics;
use serde_json::Value;

/// The server model the reply builders name.
pub(crate) const MOCK_MODEL: &str = "mock-model";

/// A text reply naming [`MOCK_MODEL`], with no finish reason, reasoning,
/// or metrics.
#[must_use]
pub(crate) fn text(content: &str) -> ScriptedReply {
    mock_text(content, None)
}

/// A text reply naming [`MOCK_MODEL`] that the backend stopped with
/// `finish_reason`.
#[must_use]
pub(crate) fn text_finish(content: &str, finish_reason: &str) -> ScriptedReply {
    mock_text(content, Some(finish_reason))
}

/// A text reply naming [`MOCK_MODEL`], with no reasoning or metrics.
fn mock_text(content: &str, finish_reason: Option<&str>) -> ScriptedReply {
    ScriptedReply::Text {
        model: MOCK_MODEL.to_owned(),
        content: content.to_owned(),
        finish_reason: finish_reason.map(str::to_owned),
        reasoning: None,
        metrics: None,
    }
}

/// A tool-call batch naming [`MOCK_MODEL`].
///
/// Each call is its id, wire name, and JSON-encoded arguments, in batch
/// order.
///
/// # Panics
/// When a call's arguments are not JSON.
#[must_use]
#[track_caller]
pub(crate) fn calls(calls: &[(&str, &str, &str)]) -> ScriptedReply {
    let calls = calls
        .iter()
        .map(|&(id, name, arguments)| {
            let arguments = serde_json::from_str(arguments)
                .unwrap_or_else(|error| panic!("call {id} arguments {arguments:?}: {error}"));
            (id.to_owned(), name.to_owned(), arguments)
        })
        .collect();
    ScriptedReply::ToolCalls {
        model: MOCK_MODEL.to_owned(),
        calls,
    }
}

/// A failed round of `kind`, with HTTP `status` and response `body`.
///
/// For every kind it builds the kind's phrase with the status appended and
/// the body as the detail, so each test names its kind directly.
#[must_use]
pub(crate) fn fail(kind: CompletionErrorKind, status: u16, body: &str) -> ScriptedReply {
    ScriptedReply::Failure {
        kind,
        message: format!("{} (status {status})", kind.phrase()),
        detail: Some(body.to_owned()),
    }
}

/// One scripted answer to a round.
#[derive(Clone, Debug)]
pub enum ScriptedReply {
    /// A text reply.
    ///
    /// Whitespace-only `content` is an empty reply.
    Text {
        /// The model the completion names as its server.
        model: String,
        /// The reply text.
        content: String,
        /// The backend's stop label, when it supplied one.
        finish_reason: Option<String>,
        /// The reasoning side channel, when the backend supplied one.
        reasoning: Option<String>,
        /// What the backend measured, when it reported anything.
        metrics: Option<Box<CallMetrics>>,
    },
    /// A tool-call batch: each call's id, wire name, and parsed arguments.
    ToolCalls {
        /// The model the completion names as its server.
        model: String,
        /// The calls, in batch order.
        calls: Vec<(String, String, Value)>,
    },
    /// A failed round.
    ///
    /// A fresh [`CompletionError`] is built for every answer, because the
    /// error is not `Clone`.
    Failure {
        /// The failure's kind.
        kind: CompletionErrorKind,
        /// The failure's message.
        message: String,
        /// The provider text behind the failure, when there is some.
        detail: Option<String>,
    },
}

impl ScriptedReply {
    /// Answers a round with this reply: the answer
    /// [`ScriptedChat::complete_now`] gives.
    pub(crate) fn answer(&self) -> Result<Completion, CompletionError> {
        match self {
            ScriptedReply::Text {
                model,
                content,
                finish_reason,
                reasoning,
                metrics,
            } => {
                let reasoning = reasoning.as_deref().filter(|text| !text.trim().is_empty());
                if content.trim().is_empty() {
                    let error = CompletionError::empty_reply(reasoning.is_some());
                    return Err(match finish_reason {
                        Some(reason) => error.with_finish_reason(reason),
                        None => error,
                    });
                }
                let mut completion =
                    Completion::from_result(CompletionResult::Text(content.clone()), model)?;
                if let Some(reason) = finish_reason {
                    completion = completion.with_finish_reason(reason);
                }
                if let Some(reasoning) = reasoning {
                    completion = completion.with_reasoning_content(reasoning);
                }
                if let Some(metrics) = metrics {
                    completion = completion.with_metrics(metrics.as_ref().clone());
                }
                Ok(completion)
            }
            ScriptedReply::ToolCalls { model, calls } => {
                let calls = calls
                    .iter()
                    .map(|(id, name, arguments)| ToolCall::from_parts(id, name, arguments.clone()))
                    .collect::<Result<Vec<_>, _>>()?;
                Completion::from_result(CompletionResult::ToolCalls(calls), model)
            }
            ScriptedReply::Failure {
                kind,
                message,
                detail,
            } => {
                let mut error = CompletionError::new(*kind, message);
                if let Some(detail) = detail {
                    error = error.with_detail(detail);
                }
                Err(error)
            }
        }
    }
}

/// What one round's `Chat` effect carried.
#[derive(Clone, Debug)]
pub(crate) struct ScriptedCall {
    /// The conversation the round sent.
    pub(crate) messages: Vec<Message>,
    /// The tools the round advertised, in order.
    pub(crate) tools: Vec<ToolSchema>,
    /// The options the round ran under.
    pub(crate) options: CompletionOptions,
}

impl ScriptedCall {
    /// The round's messages as JSON, in the shape each message serializes
    /// to.
    ///
    /// # Panics
    /// When a message does not serialize.
    #[must_use]
    pub(crate) fn messages_json(&self) -> Value {
        serde_json::to_value(&self.messages)
            .unwrap_or_else(|error| panic!("a message serializes to JSON: {error}"))
    }

    /// The wire names of the tools the round advertised, in order.
    #[must_use]
    pub(crate) fn tool_names(&self) -> Vec<&str> {
        self.tools.iter().map(ToolSchema::name).collect()
    }
}

/// A scripted model that answers rounds from its script in order.
///
/// It repeats the last reply once the script is exhausted, counts the
/// rounds, and records what each carried. Clones share the script, the
/// count, and the record.
#[derive(Clone, Debug)]
pub struct ScriptedChat {
    replies: Arc<Vec<ScriptedReply>>,
    requests: Arc<Mutex<Vec<ScriptedCall>>>,
}

impl ScriptedChat {
    /// A model answering with `replies` in order.
    ///
    /// # Panics
    /// Panics when `replies` is empty.
    #[must_use]
    pub fn new(replies: Vec<ScriptedReply>) -> ScriptedChat {
        assert!(
            !replies.is_empty(),
            "a scripted chat needs at least one reply"
        );
        ScriptedChat {
            replies: Arc::new(replies),
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// The number of rounds asked of the script so far.
    #[must_use]
    pub fn call_count(&self) -> usize {
        self.requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    /// Every recorded round, in the order the rounds arrived.
    #[must_use]
    pub(crate) fn requests(&self) -> Vec<ScriptedCall> {
        self.requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The most recently recorded round, if any.
    #[must_use]
    pub(crate) fn last_request(&self) -> Option<ScriptedCall> {
        self.requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .last()
            .cloned()
    }

    /// Answers one round at once.
    ///
    /// It records what the round carried, which counts it, and returns the
    /// completion or the failure.
    ///
    /// # Errors
    /// Returns the scripted failure, the `EmptyReply` failure for an empty
    /// text reply, and the `InvalidParts` failure for a tool call the
    /// validating constructors refuse.
    pub fn complete_now(
        &self,
        messages: &[Message],
        tools: &[ToolSchema],
        options: &CompletionOptions,
    ) -> Result<Completion, CompletionError> {
        let index = {
            let mut requests = self.requests.lock().unwrap_or_else(PoisonError::into_inner);
            let index = requests.len();
            requests.push(ScriptedCall {
                messages: messages.to_vec(),
                tools: tools.to_vec(),
                options: options.clone(),
            });
            index
        };
        self.replies[index.min(self.replies.len() - 1)].answer()
    }
}
