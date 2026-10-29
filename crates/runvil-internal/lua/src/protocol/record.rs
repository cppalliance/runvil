//! The message-record vocabulary a chat request is built from: one
//! validated record, its role, its content, the tool calls it holds, and
//! the metadata the projection never sends, plus the per-record
//! validation the list's builders run as they add a record.

use runvil_model_client::client::Role;

use super::non_empty::NonEmpty;

/// The most bytes a record's `meta` may take as compact JSON.
pub(crate) const MAX_RECORD_META_BYTES: usize = 8 * 1024;

/// One content part of a multimodal message: visible text or an image URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentPart {
    /// Visible text.
    Text(String),
    /// An image URL; the record parse does not restrict its scheme.
    ImageUrl(String),
}

/// A message record's content: plain visible text, or a content-parts
/// array.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageContent {
    /// Plain visible text.
    Text(String),
    /// A multimodal content-parts array.
    Parts(NonEmpty<ContentPart>),
}

/// One normalized tool call an assistant message holds: the
/// provider-neutral `{id, name, arguments}` record every later component
/// consumes. The record stays neutral; the projection's `wire_message`
/// renders it as the OpenAI function-call wire shape at dispatch time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallRecord {
    /// The call identifier tool results correlate against.
    pub id: String,
    /// The wire name of the tool the model asked for.
    pub name: String,
    /// The call arguments; always an object, normalized to `{}` when the
    /// record had none.
    pub arguments: serde_json::Value,
}

/// A message record's role. A tool result holds the call ID it answers,
/// which only a tool result has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordRole {
    /// A system instruction.
    System,
    /// The user's turn.
    User,
    /// The model's turn.
    Assistant,
    /// A tool result.
    Tool {
        /// The ID of the call the result answers.
        tool_call_id: String,
    },
}

impl RecordRole {
    /// The wire role.
    #[must_use]
    pub(crate) fn kind(&self) -> Role {
        match self {
            RecordRole::System => Role::System,
            RecordRole::User => Role::User,
            RecordRole::Assistant => Role::Assistant,
            RecordRole::Tool { .. } => Role::Tool,
        }
    }

    /// The call ID a tool result answers; `None` for every other role.
    #[must_use]
    pub(crate) fn tool_call_id(&self) -> Option<&str> {
        match self {
            RecordRole::Tool { tool_call_id } => Some(tool_call_id),
            RecordRole::System | RecordRole::User | RecordRole::Assistant => None,
        }
    }
}

/// One validated message record: the plain-message contract every later
/// component (projection, `models.loop`, the message builders) consumes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageRecord {
    /// The message role, with a tool result's call ID.
    pub role: RecordRole,
    /// The visible content.
    pub content: MessageContent,
    /// The normalized tool calls the record holds. Only an assistant
    /// record may send them: the record parse accepts them on any role,
    /// and the projection refuses them on every other role.
    pub tool_calls: Vec<ToolCallRecord>,
    /// Metadata the projection never sends; `None` when absent or JSON null.
    pub meta: Option<serde_json::Value>,
}

impl MessageRecord {
    /// The record's JSON form: `role`, `content`, and `tool_calls`,
    /// `tool_call_id`, and `meta` when present, in the shapes the record
    /// parse accepts, so the parse turns it back into an equal record.
    #[must_use]
    pub(crate) fn to_json(&self) -> serde_json::Value {
        let mut entry = serde_json::Map::new();
        entry.insert("role".to_owned(), self.role.kind().as_str().into());
        entry.insert("content".to_owned(), self.content.to_json());
        if let Some(calls) = self.tool_calls_json() {
            entry.insert("tool_calls".to_owned(), calls);
        }
        if let Some(id) = self.role.tool_call_id() {
            entry.insert("tool_call_id".to_owned(), id.into());
        }
        if let Some(meta) = &self.meta {
            entry.insert("meta".to_owned(), meta.clone());
        }
        serde_json::Value::Object(entry)
    }

    /// The `tool_calls` field's JSON form: an array of `{id, name,
    /// arguments}` objects, or `None` when the record holds no calls.
    pub(crate) fn tool_calls_json(&self) -> Option<serde_json::Value> {
        if self.tool_calls.is_empty() {
            return None;
        }
        let calls = self.tool_calls.iter().map(|call| {
            serde_json::json!({
                "id": call.id,
                "name": call.name,
                "arguments": call.arguments,
            })
        });
        Some(calls.collect())
    }
}

impl MessageContent {
    /// The `content` field's JSON form: the text, or an array of `text`
    /// and `image_url` parts.
    pub(crate) fn to_json(&self) -> serde_json::Value {
        match self {
            MessageContent::Text(text) => text.as_str().into(),
            MessageContent::Parts(parts) => parts
                .iter()
                .map(|part| match part {
                    ContentPart::Text(text) => serde_json::json!({ "type": "text", "text": text }),
                    ContentPart::ImageUrl(url) => {
                        serde_json::json!({ "type": "image_url", "image_url": { "url": url } })
                    }
                })
                .collect(),
        }
    }
}

/// The message roles the chat protocol accepts, comma-separated.
fn chat_roles() -> String {
    Role::ALL
        .iter()
        .map(|role| role.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The content-part types the chat protocol accepts (the Multimodal
/// contract: text parts and image-URL parts).
const CHAT_PART_TYPES: [&str; 2] = ["text", "image_url"];

/// Validates one record in its JSON form, as a list builder adds it.
/// `index` is the 1-based position the record takes, named in the
/// error.
///
/// The rules: a known role; `content` a string or a non-empty
/// content-parts array with known part types and payloads; a
/// `tool_call_id` a string, required on tool records and refused on every
/// other; a present `tool_calls` an array of normalized
/// `{id, name, arguments}` records; a `meta` of any JSON value, none when
/// null, at most [`MAX_RECORD_META_BYTES`] as compact JSON. Fields beyond
/// the five a record holds (`role`, `content`, `tool_call_id`,
/// `tool_calls`, `meta`) are dropped.
/// Cross-record checks - unique call IDs, complete call-result pairing,
/// provider-required alternation - belong to the per-dispatch projection
/// ([`crate::projection`]).
pub(crate) fn parse_record(
    index: usize,
    entry: &serde_json::Value,
) -> std::result::Result<MessageRecord, String> {
    let serde_json::Value::Object(entry) = entry else {
        return Err(format!("messages[{index}] must be a message table"));
    };
    let role = match entry.get("role") {
        Some(serde_json::Value::String(role)) => match role.parse::<Role>() {
            Ok(role) => role,
            Err(_) => {
                return Err(format!(
                    "messages[{index}] role {role:?} is unknown; known roles: {}",
                    chat_roles()
                ));
            }
        },
        _ => {
            return Err(format!(
                "messages[{index}] role must be a string, one of: {}",
                chat_roles()
            ));
        }
    };
    let content = match entry.get("content") {
        Some(serde_json::Value::String(text)) => MessageContent::Text(text.clone()),
        Some(serde_json::Value::Array(parts)) => {
            match NonEmpty::from_vec(parse_content_parts(index, parts)?) {
                Some(parts) => MessageContent::Parts(parts),
                None => return Err(content_refusal(index)),
            }
        }
        _ => return Err(content_refusal(index)),
    };
    let tool_call_id = match entry.get("tool_call_id") {
        None => None,
        Some(serde_json::Value::String(id)) => Some(id.clone()),
        Some(_) => {
            return Err(format!("messages[{index}] tool_call_id must be a string"));
        }
    };
    let role = match (role, tool_call_id) {
        (Role::Tool, Some(tool_call_id)) => RecordRole::Tool { tool_call_id },
        (Role::Tool, None) => {
            return Err(format!(
                "messages[{index}] is a tool message and must set a string tool_call_id"
            ));
        }
        (_, Some(_)) => {
            return Err(format!(
                "messages[{index}] sets a tool_call_id but is not a tool message"
            ));
        }
        (Role::System, None) => RecordRole::System,
        (Role::User, None) => RecordRole::User,
        (Role::Assistant, None) => RecordRole::Assistant,
        (other, None) => {
            return Err(format!(
                "messages[{index}] role {:?} is unknown; known roles: {}",
                other.as_str(),
                chat_roles()
            ));
        }
    };
    let tool_calls = match entry.get("tool_calls") {
        None => Vec::new(),
        Some(serde_json::Value::Array(calls)) => calls
            .iter()
            .enumerate()
            .map(|(position, call)| parse_tool_call_record(index, position + 1, call))
            .collect::<std::result::Result<Vec<_>, _>>()?,
        Some(_) => {
            return Err(format!("messages[{index}] tool_calls must be an array"));
        }
    };
    let meta = match entry.get("meta") {
        None | Some(serde_json::Value::Null) => None,
        Some(meta) => {
            let size = meta.to_string().len();
            if size > MAX_RECORD_META_BYTES {
                return Err(format!(
                    "messages[{index}] meta is {size} bytes as JSON; the limit is \
                     {MAX_RECORD_META_BYTES} bytes"
                ));
            }
            Some(meta.clone())
        }
    };
    Ok(MessageRecord {
        role,
        content,
        tool_calls,
        meta,
    })
}

/// The refusal for a `content` that is neither a string nor a non-empty
/// content-parts array.
fn content_refusal(index: usize) -> String {
    format!("messages[{index}] content must be a string or a non-empty array of content parts")
}

/// Parses one message's content-parts array: each part is a table whose
/// `type` names a known part kind, with that kind's required payload.
fn parse_content_parts(
    index: usize,
    parts: &[serde_json::Value],
) -> std::result::Result<Vec<ContentPart>, String> {
    parts
        .iter()
        .enumerate()
        .map(|(position, part)| parse_content_part(index, position + 1, part))
        .collect()
}

/// Parses one content part into its typed variant: a `text` part has a
/// string `text` field; an `image_url` part has an `image_url` table
/// with a string `url` field.
fn parse_content_part(
    index: usize,
    part_index: usize,
    part: &serde_json::Value,
) -> std::result::Result<ContentPart, String> {
    let malformed = || {
        format!(
            "messages[{index}] content part {part_index} must be a table \
             with a string type field"
        )
    };
    let serde_json::Value::Object(part) = part else {
        return Err(malformed());
    };
    let kind = match part.get("type") {
        Some(serde_json::Value::String(kind)) => kind.as_str(),
        _ => return Err(malformed()),
    };
    match kind {
        "text" => match part.get("text") {
            Some(serde_json::Value::String(text)) => Ok(ContentPart::Text(text.clone())),
            _ => Err(format!(
                "messages[{index}] content part {part_index} is a text part \
                 and must set a string text field"
            )),
        },
        "image_url" => {
            let Some(serde_json::Value::Object(image)) = part.get("image_url") else {
                return Err(image_url_refusal(index, part_index));
            };
            match image.get("url") {
                Some(serde_json::Value::String(url)) => Ok(ContentPart::ImageUrl(url.clone())),
                _ => Err(image_url_refusal(index, part_index)),
            }
        }
        unknown => Err(format!(
            "messages[{index}] content part {part_index} has unknown type \
             {unknown:?}; known types: {}",
            CHAT_PART_TYPES.join(", ")
        )),
    }
}

/// The refusal for an `image_url` part without an `image_url` table
/// holding a string `url`.
fn image_url_refusal(index: usize, part_index: usize) -> String {
    format!(
        "messages[{index}] content part {part_index} is an image_url part and must set an \
         image_url table with a string url field"
    )
}

/// Parses one tool call into its normalized record: a string `id`, a
/// string `name`, and an `arguments` object that normalizes to `{}` when
/// absent.
fn parse_tool_call_record(
    index: usize,
    call_index: usize,
    call: &serde_json::Value,
) -> std::result::Result<ToolCallRecord, String> {
    let serde_json::Value::Object(call) = call else {
        return Err(format!(
            "messages[{index}] tool_calls[{call_index}] must be a table"
        ));
    };
    let id = match call.get("id") {
        Some(serde_json::Value::String(id)) => id.clone(),
        _ => {
            return Err(format!(
                "messages[{index}] tool_calls[{call_index}] must set a string id"
            ));
        }
    };
    let name = match call.get("name") {
        Some(serde_json::Value::String(name)) => name.clone(),
        _ => {
            return Err(format!(
                "messages[{index}] tool_calls[{call_index}] must set a string name"
            ));
        }
    };
    let arguments = match call.get("arguments") {
        None | Some(serde_json::Value::Null) => serde_json::Value::Object(serde_json::Map::new()),
        Some(arguments @ serde_json::Value::Object(_)) => arguments.clone(),
        Some(_) => {
            return Err(format!(
                "messages[{index}] tool_calls[{call_index}] arguments must be a table"
            ));
        }
    };
    Ok(ToolCallRecord {
        id,
        name,
        arguments,
    })
}
