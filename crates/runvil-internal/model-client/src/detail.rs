//! Operations on the wire types that only the Engine performs.
//!
//! The `runvil` facade never re-exports this module, so only Engine
//! crates reach it. Each function reaches a field or a construction step
//! the facade-visible type keeps private. [`completion_into_result`] is the
//! one that splits a completion into the result the tool loop consumes and
//! the metadata the round reports.

use serde_json::Value;

use crate::client::{
    Completion, CompletionResult, Message, Role, ToolCall, ToolSchema, ToolSchemaError,
};

/// Constructs a message from parts a caller has already validated.
///
/// `content` is the raw wire content value - a string for a plain message
/// or an `OpenAI` content-parts array for a multimodal one - and serializes
/// into the request verbatim. For each chat round the Engine sends, its Lua
/// layer has already validated each author-built record once, as the
/// message list added it, and hands the validated parts here.
#[must_use]
pub fn message_from_validated_parts(
    role: Role,
    content: Value,
    tool_call_id: Option<String>,
    tool_calls: Option<Vec<Value>>,
) -> Message {
    Message {
        role,
        content,
        tool_call_id,
        tool_calls,
    }
}

/// Returns a message's raw content value (a string or a content-parts
/// array), for the executor's pre-dispatch size estimate, which needs the
/// parts that [`Message::content`] flattens away.
#[must_use]
pub fn message_content_value(message: &Message) -> &Value {
    &message.content
}

/// Returns the raw `tool_calls` array an assistant turn holds, for the
/// executor's pre-dispatch size estimate.
#[must_use]
pub fn message_raw_tool_calls(message: &Message) -> Option<&[Value]> {
    message.tool_calls.as_deref()
}

/// Builds a tool schema, validating the wire name and that the parameters
/// are a JSON object.
///
/// The raw [`serde_json::Value`] schema enters here only from the
/// executor's internal tool contract, so the raw JSON never appears in a
/// facade-visible constructor.
///
/// # Errors
/// Returns [`ToolSchemaError::InvalidName`] when `name` does not match
/// `[A-Za-z][A-Za-z0-9_-]{0,63}`, and
/// [`ToolSchemaError::NonObjectSchema`] when `parameters` is not a JSON
/// object, so a tool can never be advertised to the model with an unusable
/// name or a non-object JSON Schema.
pub fn tool_schema_new(
    name: impl Into<String>,
    description: impl Into<String>,
    parameters: Value,
) -> Result<ToolSchema, ToolSchemaError> {
    let name = name.into();
    let bytes = name.as_bytes();
    let valid = (1..=64).contains(&bytes.len())
        && bytes[0].is_ascii_alphabetic()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'));
    if !valid {
        return Err(ToolSchemaError::InvalidName {
            name,
            reason: "expected [A-Za-z][A-Za-z0-9_-]{0,63}",
        });
    }
    if !parameters.is_object() {
        return Err(ToolSchemaError::NonObjectSchema { name });
    }
    Ok(ToolSchema {
        name,
        description: description.into(),
        parameters,
    })
}

/// Returns a tool call's parsed arguments as the raw wire JSON the
/// dispatcher hands a tool.
///
/// The call's arguments are stored as a map, and the dispatcher's
/// [`ToolCallEvent`] field is already a [`Value`], so the value is built
/// here and moved into the event rather than cloned at the call site.
///
/// [`ToolCallEvent`]: runvil_types::metrics::ToolCallEvent
#[must_use]
pub fn tool_call_arguments(call: &ToolCall) -> Value {
    Value::Object(call.arguments.clone())
}

/// Takes the text or tool-call outcome out of a completion.
#[must_use]
pub fn completion_into_result(completion: Completion) -> CompletionResult {
    completion.result
}
