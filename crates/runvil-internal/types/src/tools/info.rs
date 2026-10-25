//! The [`ToolInfo`]: everything an Engine needs to know about a tool
//! except how to run it.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ids::ToolId;

/// A tool described as data.
///
/// A `ToolInfo` holds the tool's stable id, the name the model calls it by
/// when one is set, the description the model reads, its parameter schema,
/// whether its output is structured, whether a stop leaves its calls
/// running, and whether its output lists tools.
///
/// The caller collects the `ToolInfo` values of every Pack it can serve
/// into a [`ToolCatalog`](super::ToolCatalog). The implementations stay with
/// the caller, keyed by [`ToolId`]. Every catalog tool is available to the
/// prompt's Lua, which calls one by id, and a section offers one to its
/// model with `tools.offer`. The Engine issues each tool call as an effect
/// that names the tool's id. The caller resolves that id to the
/// implementation.
///
/// A catalog tool's wire name, the name the model calls it by, is its
/// `wire_name` when set, which a Pack may set in `Pack::tools()` as well as
/// in a render (`Pack::render`), or else its id with `/` and `.` turned
/// into `_`, such as `web_fetch` for `web/fetch`. A catalog tool's
/// `wire_name` stays its wire name wherever a render passes it through. A
/// tool that appears only in a render's answer is model-only, which Lua
/// cannot see or call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ToolInfo {
    /// The tool's stable identity. The catalog uses it as the key.
    pub id: ToolId,
    /// The name the model calls the tool by, or `None` for the default
    /// derived from the id. A `ToolInfo` serialized without the field
    /// reads as `None`, and a `None` is left out when serializing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wire_name: Option<String>,
    /// The one-sentence description the model reads.
    pub description: String,
    /// The JSON-Schema `object` the tool's arguments must match.
    pub parameters_schema: Value,
    /// Whether the tool's output text is a single JSON value. When it is, a
    /// script that calls the tool receives the output as data. Otherwise the
    /// script receives it as a string.
    pub structured_output: bool,
    /// Whether a stop leaves the tool's calls in flight. A stop cancels
    /// every other call in flight and lets the run go on; the run's end
    /// and the early end of the task that made the call still end this
    /// one. A question to the operator sets it, so the question
    /// stays open. A `ToolInfo` serialized without the field reads as
    /// `false`.
    #[serde(default)]
    pub survives_stop: bool,
    /// Whether the tool's output lists tools, such as the tools of the
    /// servers a Pack reaches. A script that calls such a tool receives
    /// its untrusted output as plain text, so the script can read the
    /// listing, while a model's call still receives the untrusted
    /// envelope. A `ToolInfo` serialized without the field reads as
    /// `false`.
    #[serde(default)]
    pub describes_tools: bool,
}

impl ToolInfo {
    /// Builds a `ToolInfo` under the default wire name, with plain-text
    /// output that a stop reaches and that lists no tools.
    #[must_use]
    pub fn new(id: ToolId, description: impl Into<String>, parameters_schema: Value) -> ToolInfo {
        ToolInfo {
            id,
            wire_name: None,
            description: description.into(),
            parameters_schema,
            structured_output: false,
            survives_stop: false,
            describes_tools: false,
        }
    }

    /// Sets the name the model calls the tool by.
    #[must_use]
    pub fn wire_name(mut self, wire_name: impl Into<String>) -> ToolInfo {
        self.wire_name = Some(wire_name.into());
        self
    }

    /// Sets whether the tool's output is structured JSON (`true`) or plain
    /// text (`false`).
    #[must_use]
    pub fn structured(mut self, structured: bool) -> ToolInfo {
        self.structured_output = structured;
        self
    }

    /// Sets whether a stop leaves the tool's calls in flight (`true`) or
    /// drops them (`false`).
    #[must_use]
    pub fn survives_stop(mut self, survives: bool) -> ToolInfo {
        self.survives_stop = survives;
        self
    }

    /// Sets whether the tool's output lists tools (`true`) or not
    /// (`false`).
    #[must_use]
    pub fn describes_tools(mut self, describes: bool) -> ToolInfo {
        self.describes_tools = describes;
        self
    }
}
