//! The [`ToolInfo`]: everything an Engine needs to know about a tool
//! except how to run it.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ids::ToolId;

/// A tool described as data.
///
/// A `ToolInfo` holds the tool's stable id, the description the model
/// reads, its parameter schema, whether its output is structured, and
/// whether a stop leaves its calls running.
///
/// The caller collects the `ToolInfo` values of every Pack it can serve
/// into a [`ToolCatalog`](super::ToolCatalog). The implementations stay with
/// the caller, keyed by [`ToolId`]. Every catalog tool is available to the
/// prompt's Lua, which calls one by id, and a section offers one to its
/// model with `tools.offer`. The Engine issues each tool call as an effect
/// that names the tool's id. The caller resolves that id to the
/// implementation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ToolInfo {
    /// The tool's stable identity. The catalog uses it as the key.
    pub id: ToolId,
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
}

impl ToolInfo {
    /// Builds a `ToolInfo` with plain-text output that a stop reaches.
    #[must_use]
    pub fn new(id: ToolId, description: impl Into<String>, parameters_schema: Value) -> ToolInfo {
        ToolInfo {
            id,
            description: description.into(),
            parameters_schema,
            structured_output: false,
            survives_stop: false,
        }
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
}
