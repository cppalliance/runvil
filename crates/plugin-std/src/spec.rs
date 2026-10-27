//! The six tool definitions this crate ships, each an embedded JSON file.
//!
//! A file holds the name the model calls the tool by, the description the
//! model reads, and the JSON Schema its arguments must match. The names,
//! descriptions, and schemas are Cursor's own, with the lines that
//! describe behavior this Pack does not have removed; `schemas/EDITS.md`
//! records each of those edits.

use std::sync::LazyLock;

use runvil_plugin::{ToolError, ToolErrorKind};
use serde::Deserialize;
use serde_json::Value;

/// One tool's definition: its id segment, and the file's own contents.
#[derive(Debug)]
pub(crate) struct Definition {
    /// The tool's last id segment, such as `read`, which its file's name
    /// gives.
    pub(crate) segment: &'static str,
    /// The name the model calls the tool by.
    pub(crate) wire_name: String,
    /// The description the model reads.
    pub(crate) description: String,
    /// The JSON-Schema `object` the tool's arguments must match.
    pub(crate) parameters: Value,
}

/// One tool's file, as the file itself holds it.
#[derive(Debug, Deserialize)]
struct File {
    /// The name the model calls the tool by.
    wire_name: String,
    /// The description the model reads.
    description: String,
    /// The JSON-Schema `object` the tool's arguments must match.
    parameters: Value,
}

/// Each tool's id segment beside the source of the file it reads.
const SOURCES: [(&str, &str); 6] = [
    ("read", include_str!("../schemas/read.json")),
    ("write", include_str!("../schemas/write.json")),
    ("replace", include_str!("../schemas/replace.json")),
    ("rm", include_str!("../schemas/rm.json")),
    ("glob", include_str!("../schemas/glob.json")),
    ("grep", include_str!("../schemas/grep.json")),
];

/// The parsed definitions, one per [`SOURCES`] entry in order.
static DEFINITIONS: LazyLock<Result<Vec<Definition>, ToolError>> = LazyLock::new(|| {
    SOURCES
        .iter()
        .map(|(segment, source)| {
            let file: File = serde_json::from_str(source).map_err(|e| {
                ToolError::new(
                    ToolErrorKind::Backend,
                    format!("std could not read the definition of its {segment} tool"),
                )
                .with_source(e)
            })?;
            Ok(Definition {
                segment,
                wire_name: file.wire_name,
                description: file.description,
                parameters: file.parameters,
            })
        })
        .collect()
});

/// Every definition, in [`SOURCES`] order.
///
/// # Errors
/// Returns a backend [`ToolError`] naming the tool whose file does not
/// parse. The files are embedded, so this can only happen after an edit.
pub(crate) fn all() -> Result<&'static [Definition], ToolError> {
    match &*DEFINITIONS {
        Ok(definitions) => Ok(definitions),
        Err(error) => Err(ToolError::new(error.kind(), error.to_string())),
    }
}

#[cfg(test)]
#[path = "spec-tests.rs"]
mod tests;
