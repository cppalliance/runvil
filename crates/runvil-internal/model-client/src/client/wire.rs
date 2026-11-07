//! Wire types for the chat-completions protocol: messages, tool schemas,
//! tool calls, and completion results. The types are `#[non_exhaustive]`,
//! so a caller that ran no transport builds them only through their
//! constructors, which refuse as `InvalidParts` the tool-call shapes a
//! wire decoder refuses.

use std::collections::HashSet;

use runvil_types::error::Error;
use runvil_types::metrics::CallMetrics;
use serde_json::Value;

use super::Role;
use crate::normalize::{
    check_call_id, check_call_name, check_unique_call_id, checked_call_arguments,
    make_empty_reply_error,
};

/// One message in a chat-completions conversation.
///
/// A message serializes to a JSON object with `role` and `content` keys.
/// The optional `tool_call_id` and `tool_calls` keys appear only when set,
/// so a plain `user` message serializes to just `{"role":..,"content":..}`.
// `PartialEq`/`Eq` compare messages structurally. `serde_json::Value`
// implements `Eq` (its `Number` compares/hashes float bits), so the
// `tool_calls` field does not block a total equivalence.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[non_exhaustive]
pub struct Message {
    /// Who speaks the message.
    pub(crate) role: Role,
    /// The message content, serialized into the request verbatim: a JSON
    /// string for a plain text message (every inherent constructor), or an
    /// OpenAI content-parts array for a multimodal message built through
    /// [`crate::detail::message_from_validated_parts`].
    pub(crate) content: Value,
    /// For a `tool` message, the id of the tool call this result answers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tool_call_id: Option<String>,
    /// For an `assistant` turn that requested tools, the raw `tool_calls` array
    /// as received from the backend, echoed back verbatim on the live path. The
    /// projection path instead re-renders each call from its neutral
    /// `ToolCallRecord` into the OpenAI function-call shape, so key order and
    /// whitespace can differ from the provider's original.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tool_calls: Option<Vec<Value>>,
}

impl Message {
    /// Constructs a `user` message.
    #[must_use]
    pub fn user(content: impl Into<String>) -> Message {
        Message {
            role: Role::User,
            content: Value::String(content.into()),
            tool_call_id: None,
            tool_calls: None,
        }
    }

    /// Constructs a `tool` message holding the result of a tool call.
    ///
    /// `tool_call_id` must match the `id` of the [`ToolCall`] this answers.
    #[must_use]
    pub fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Message {
        Message {
            role: Role::Tool,
            content: Value::String(content.into()),
            tool_call_id: Some(tool_call_id.into()),
            tool_calls: None,
        }
    }

    /// Constructs a plain `assistant` text turn that serializes to just
    /// `role` and `content`.
    #[must_use]
    pub fn assistant(content: impl Into<String>) -> Message {
        Message {
            role: Role::Assistant,
            content: Value::String(content.into()),
            tool_call_id: None,
            tool_calls: None,
        }
    }

    /// Returns who speaks the message.
    #[must_use]
    pub fn role(&self) -> Role {
        self.role
    }

    /// Returns the message text, or `""` when the content is a multimodal
    /// content-parts array.
    ///
    /// Only the Engine builds multimodal messages. The `user`, `tool`, and
    /// `assistant` constructors always produce text.
    #[must_use]
    pub fn content(&self) -> &str {
        self.content.as_str().unwrap_or("")
    }
}

/// A tool offered to the model: a name, a description, and a JSON Schema
/// for the tool's parameters.
///
/// A request lists each schema as an `OpenAI` function tool:
/// `{"type":"function","function":{"name":..,"description":..,"parameters":..}}`.
// `PartialEq`/`Eq` compare schemas structurally. `serde_json::Value`
// implements `Eq`, so the `parameters` schema does not block equivalence.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[non_exhaustive]
pub struct ToolSchema {
    /// The tool's wire name.
    pub(crate) name: String,
    /// A one-sentence description shown to the model.
    pub(crate) description: String,
    /// The JSON Schema for the tool's parameters.
    pub(crate) parameters: Value,
}

impl ToolSchema {
    /// Returns the tool's name as sent to the model. The
    /// `crate::detail::tool_schema_new` builder is the only one, and it
    /// accepts a leading ASCII letter followed by up to 63 letters, digits,
    /// `_`, or `-`.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the one-sentence description shown to the model.
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Returns the JSON Schema for the tool's parameters, always a JSON
    /// object.
    #[must_use]
    pub fn parameters(&self) -> &Value {
        &self.parameters
    }
}

/// A tool call the model asked for: a call id, a tool name, and arguments.
///
/// `OpenAI` sends a call's `function.arguments` as a JSON-encoded string.
/// A decoded `ToolCall` holds that string parsed into a JSON object.
/// Decoding fails with an error when the arguments are missing, are not a
/// string, are not valid JSON, or do not parse to an object.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ToolCall {
    /// The id the model assigned to this call, echoed back with its result.
    id: String,
    /// The name of the tool to invoke.
    name: String,
    /// The parsed arguments for the call. The raw wire JSON stays
    /// crate-private: the caller inspects arguments through
    /// [`ToolCall::arguments`].
    pub(crate) arguments: serde_json::Map<String, Value>,
}

impl ToolCall {
    /// Builds a tool call from its id, tool name, and arguments.
    ///
    /// `arguments` is the parsed argument payload, already decoded from
    /// JSON text.
    ///
    /// # Errors
    /// Returns an [`Error`] with code `InvalidParts` when `id` or `name` is
    /// blank or `arguments` is not a JSON object. It is not retryable: the
    /// same parts fail the same way.
    pub fn from_parts(
        id: impl Into<String>,
        name: impl Into<String>,
        arguments: Value,
    ) -> Result<ToolCall, Error> {
        let id = id.into();
        let name = name.into();
        check_call_id(&id)?;
        check_call_name(&name)?;
        Ok(ToolCall {
            id,
            name,
            arguments: checked_call_arguments(arguments)?,
        })
    }

    /// Returns the id the model assigned to this call.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the name of the tool to invoke.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns a borrowed view of the call's arguments.
    ///
    /// The [`ToolArguments`] view gives the arguments as canonical JSON text,
    /// says whether a key is present, and lists the argument names.
    #[must_use]
    pub fn arguments(&self) -> ToolArguments<'_> {
        ToolArguments {
            map: &self.arguments,
        }
    }
}

/// A borrowed, read-only view of one [`ToolCall`]'s arguments.
///
/// The arguments are always a JSON object: decoding and
/// [`ToolCall::from_parts`] both reject any other value.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct ToolArguments<'a> {
    map: &'a serde_json::Map<String, Value>,
}

impl ToolArguments<'_> {
    /// Returns the arguments object serialized as canonical JSON text.
    #[must_use]
    pub fn to_json_string(&self) -> String {
        // A map built from a `Value` tree always serializes, so the fallback
        // arm is unreachable. `unwrap_or_default` is still wrong here: it
        // would publish an empty arguments string, which a tool cannot parse.
        serde_json::to_string(self.map).unwrap_or_else(|_| "{}".to_owned())
    }

    /// Returns whether the arguments object is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Returns whether the arguments object has a top-level key named `key`.
    #[must_use]
    pub fn contains(&self, key: &str) -> bool {
        self.map.contains_key(key)
    }

    /// Returns the top-level argument names of the arguments object.
    pub fn names(&self) -> ArgumentNames<'_> {
        ArgumentNames {
            keys: self.map.keys(),
        }
    }
}

/// Yields the top-level argument names of one [`ToolCall`]'s arguments.
///
/// [`ToolArguments::names`] returns it.
#[derive(Debug, Clone)]
#[must_use = "an iterator does nothing until it is consumed"]
pub struct ArgumentNames<'a> {
    /// The object's keys.
    keys: serde_json::map::Keys<'a>,
}

impl<'a> Iterator for ArgumentNames<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        self.keys.next().map(String::as_str)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.keys.size_hint()
    }
}

/// What the model returned for one completion: a final text reply or a
/// request to call tools.
///
/// For a tool request, read each call through [`ToolCall::id`],
/// [`ToolCall::name`], and [`ToolCall::arguments`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CompletionResult {
    /// The model returned a final text reply.
    Text(String),
    /// The model asked to call one or more tools.
    ToolCalls(Vec<ToolCall>),
}

/// The raw JSON request and response of one model call, kept for debug
/// capture.
///
/// The caller that sends a request in a JSON wire format can attach the
/// pair to the [`Completion`] it returns, so the application's debug
/// capture shows exactly what was sent and received. The Engine treats
/// both values as opaque and passes them only to the debug capture.
/// Build one with [`RawExchange::new`]. `Completion::from_result` builds a
/// completion with `raw` set to `None`, and the caller attaches one with
/// `Completion::with_raw`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RawExchange {
    /// The request as it left for the backend.
    request: Value,
    /// The response as the backend returned it, reassembled into the
    /// buffered chat-completion shape when it streamed.
    response: Value,
}

impl RawExchange {
    /// Creates a raw exchange from the request that was sent and the
    /// response that was read. It accepts any two JSON values and stores
    /// them as given.
    #[must_use]
    pub fn new(request: Value, response: Value) -> RawExchange {
        RawExchange { request, response }
    }

    /// Returns the request as it left for the backend.
    #[must_use]
    pub fn request(&self) -> &Value {
        &self.request
    }

    /// Returns the response as the backend returned it.
    #[must_use]
    pub fn response(&self) -> &Value {
        &self.response
    }
}

/// The parsed result of one chat-completions call, with its metadata.
///
/// The [`CompletionResult`] is what the tool loop matches on. Beside it, a
/// completion carries the backend's `finish_reason` and reasoning text, so
/// observers can report them straight from the completion. It also
/// carries the name of the model that served the call and the
/// [`CallMetrics`] the call measured, for attribution and accounting. The
/// caller may also attach the call's [`RawExchange`] for debug capture.
#[derive(Debug)]
#[non_exhaustive]
pub struct Completion {
    /// The text or tool-call outcome the tool loop consumes.
    pub(crate) result: CompletionResult,
    /// The choice's `finish_reason`, when the backend supplied one.
    finish_reason: Option<String>,
    /// The message's reasoning side channel, when the backend supplied one.
    reasoning_content: Option<String>,
    /// The model that served the call, empty when the body named none.
    model: String,
    /// Everything the call measured, when anything reported: the backend's
    /// `usage` and timing extensions, and the client's own clock.
    metrics: Option<CallMetrics>,
    /// One line per response metadata section that was present but
    /// malformed and degraded to `None`; empty for a well-formed body. The
    /// Engine reports each line as a `model_metadata_degraded` event.
    metadata_diagnostics: Vec<String>,
    /// The request and response the broker attached for debug capture.
    raw: Option<RawExchange>,
}

impl Completion {
    /// Builds a completion from a result alone.
    ///
    /// `model` is the model name the completion reports. Every optional
    /// field is absent, so `metrics` and `raw` are `None`. A text result is
    /// accepted as given. [`with_metrics`](Completion::with_metrics),
    /// [`with_raw`](Completion::with_raw),
    /// [`with_finish_reason`](Completion::with_finish_reason),
    /// [`with_reasoning_content`](Completion::with_reasoning_content),
    /// [`with_metadata_diagnostics`](Completion::with_metadata_diagnostics),
    /// and [`with_model`](Completion::with_model) add what the caller knows
    /// beyond the result.
    ///
    /// # Errors
    /// Returns an [`Error`] with code `EmptyReply` for an empty tool-call
    /// batch. Returns one with code `InvalidParts` when two calls in the
    /// batch share an id. Neither is retryable: the same parts fail the same
    /// way.
    pub fn from_result(
        result: CompletionResult,
        model: impl Into<String>,
    ) -> Result<Completion, Error> {
        if let CompletionResult::ToolCalls(calls) = &result {
            if calls.is_empty() {
                return Err(make_empty_reply_error());
            }
            let mut seen = HashSet::new();
            for call in calls {
                check_unique_call_id(&mut seen, &call.id)?;
            }
        }
        Ok(Completion {
            result,
            finish_reason: None,
            reasoning_content: None,
            model: model.into(),
            metrics: None,
            metadata_diagnostics: Vec::new(),
            raw: None,
        })
    }

    /// Returns the completion with `metrics` set to what the model call
    /// measured. Only a caller with measurements to report calls it.
    /// Otherwise the completion stays as
    /// [`from_result`](Completion::from_result) built it.
    #[must_use]
    pub fn with_metrics(mut self, metrics: CallMetrics) -> Completion {
        self.metrics = Some(metrics);
        self
    }

    /// Returns the completion with `raw` set to the request and response
    /// that debug capture records.
    ///
    /// Only a caller that has the request and response attaches them.
    /// Debug capture runs only when the application turns it on.
    #[must_use]
    pub fn with_raw(mut self, raw: RawExchange) -> Completion {
        self.raw = Some(raw);
        self
    }

    /// Returns the completion with `finish_reason` as the stop label the
    /// backend supplied.
    #[must_use]
    pub fn with_finish_reason(mut self, finish_reason: impl Into<String>) -> Completion {
        self.finish_reason = Some(finish_reason.into());
        self
    }

    /// Returns the completion with `reasoning_content` as the reasoning side
    /// channel the backend supplied. It stays separate from the answer.
    #[must_use]
    pub fn with_reasoning_content(mut self, reasoning_content: impl Into<String>) -> Completion {
        self.reasoning_content = Some(reasoning_content.into());
        self
    }

    /// Returns the completion with `diagnostics` as its metadata
    /// diagnostics, replacing any it held.
    ///
    /// Each diagnostic is one line about a metadata section of the response
    /// that was present but malformed. The Engine reports each line as a
    /// `model_metadata_degraded` event.
    #[must_use]
    pub fn with_metadata_diagnostics(mut self, diagnostics: Vec<String>) -> Completion {
        self.metadata_diagnostics = diagnostics;
        self
    }

    /// Returns the completion with `model` as the name of the model that
    /// served it, replacing the name it held.
    ///
    /// A caller that routed the round to a model labels the completion with
    /// that model's name. The round's reply and thinking events and its
    /// answer record report this name.
    #[must_use]
    pub fn with_model(mut self, model: impl Into<String>) -> Completion {
        self.model = model.into();
        self
    }

    /// Returns the text or tool-call outcome the tool loop consumes.
    #[must_use]
    pub fn result(&self) -> &CompletionResult {
        &self.result
    }

    /// Returns the `finish_reason` the backend gave for the reply, when it
    /// supplied one.
    #[must_use]
    pub fn finish_reason(&self) -> Option<&str> {
        self.finish_reason.as_deref()
    }

    /// Returns the reasoning text the backend sent beside the reply, when it
    /// sent any. It stays separate from the answer.
    #[must_use]
    pub fn reasoning_content(&self) -> Option<&str> {
        self.reasoning_content.as_deref()
    }

    /// Returns one line for each metadata section of the response that was
    /// present but malformed and so was treated as absent. The list is
    /// empty for a well-formed response. The Engine reports each line as a
    /// `model_metadata_degraded` event.
    #[must_use]
    pub fn metadata_diagnostics(&self) -> &[String] {
        &self.metadata_diagnostics
    }

    /// Returns the name of the model that served the call.
    ///
    /// That is the name last set through
    /// [`with_model`](Completion::with_model), or else the name the
    /// completion was built with. A completion decoded from a response is
    /// labeled with the model the request body named, or, when the request
    /// body named none, with the model the response body named.
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Returns what the call measured, or `None` when nothing reported. It
    /// covers token usage, llama.cpp's `timings`, vLLM's `metrics`, and the
    /// timing the client measured on its own clock. Each section is present
    /// only when its source reported it.
    #[must_use]
    pub fn metrics(&self) -> Option<&CallMetrics> {
        self.metrics.as_ref()
    }

    /// Returns the raw request and response the caller attached for debug
    /// capture, if it attached them.
    #[must_use]
    pub fn raw(&self) -> Option<&RawExchange> {
        self.raw.as_ref()
    }
}
