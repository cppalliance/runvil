//! The `/prompts/*` route handlers: parsing prompt text into the Run
//! window's contract DTO.
//!
//! The parse route is pure - no state, no disk, no clock: it turns the
//! posted markdown into the contract the Run window renders, or a `422`
//! envelope naming the parse failure kind and line. The parser's own
//! types are `Deserialize`-only, so the wire DTO is hand-written
//! `Serialize` structs built from [`Frontmatter`] accessors.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use serde::{Deserialize, Serialize};

use runvil::Prompt;
use runvil::packs::PackRef;
use runvil::prompt::{FileDecl, Frontmatter, ModelKeyword, ModelRole};
use serde_json::{Map, Value};

use crate::error::AppError;

/// The `/prompts/contract` route. The parse is pure, so the router
/// carries no state; the server mounts it under the default deadline and
/// its cross-site guard.
pub(crate) fn routes() -> axum::Router {
    axum::Router::new().route("/prompts/contract", post(contract))
}

/// The JSON body of `POST /prompts/contract`.
#[derive(Debug, Deserialize)]
struct ContractRequest {
    /// The prompt's display name (the file's, when it came from one).
    name: String,
    /// The prompt markdown to parse.
    text: String,
}

/// The Run-window contract: everything the panel renders, built from
/// the parsed frontmatter.
#[derive(Debug, Serialize)]
struct ContractResponse {
    /// The prompt's identifier.
    name: String,
    /// The one-line description shown in listings.
    description: String,
    /// The Engine major the `runvil:` key declares; `null` when absent.
    runvil: Option<u32>,
    /// The declared tool-loop cap; `null` for the runtime default.
    max_tool_iterations: Option<u32>,
    /// The declared input file; `null` when absent.
    input: Option<FileDto>,
    /// The declared output file; `null` when absent.
    output: Option<FileDto>,
    /// The declared Packs, in declaration order.
    uses: Vec<PackDto>,
    /// The input fields, derived from the `inputSchema:` key.
    args: ArgsDto,
    /// The declared model roles, sorted by label.
    models: Vec<ModelDto>,
}

/// A declared input or output file.
#[derive(Debug, Serialize)]
struct FileDto {
    /// The store-internal path.
    path: String,
    /// The human-readable purpose.
    description: String,
}

impl From<&FileDecl> for FileDto {
    fn from(decl: &FileDecl) -> Self {
        Self {
            path: decl.path().to_owned(),
            description: decl.description().to_owned(),
        }
    }
}

/// A declared Pack.
#[derive(Debug, Serialize)]
struct PackDto {
    /// The Pack id, such as `web`, which is `poop` for `acme/poop`.
    id: String,
}

impl From<&PackRef> for PackDto {
    fn from(entry: &PackRef) -> Self {
        Self {
            id: entry.id().to_string(),
        }
    }
}

/// The input fields the Run window renders, derived from the prompt's
/// input schema.
#[derive(Debug, Serialize)]
struct ArgsDto {
    /// True when the prompt has no input schema, so its input is the
    /// single prose field.
    implicit: bool,
    /// The fields, sorted by name.
    fields: Vec<ArgDto>,
}

impl ArgsDto {
    /// Derives the fields from `schema`: one per top-level property, or
    /// the single optional `prose` field when there is no schema.
    fn new(schema: Option<&Map<String, Value>>) -> Self {
        let Some(schema) = schema else {
            return Self {
                implicit: true,
                fields: vec![ArgDto {
                    name: "prose".to_owned(),
                    kind: "string",
                    optional: true,
                    default: None,
                    description: Some("Freeform input for this prompt".to_owned()),
                }],
            };
        };
        let required: Vec<&str> = schema
            .get("required")
            .and_then(Value::as_array)
            .map(|names| names.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let fields = schema
            .get("properties")
            .and_then(Value::as_object)
            .map(|properties| {
                properties
                    .iter()
                    .map(|(name, property)| {
                        ArgDto::new(name, property, required.contains(&name.as_str()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            implicit: false,
            fields,
        }
    }
}

/// One input field.
#[derive(Debug, Serialize)]
struct ArgDto {
    /// The property name.
    name: String,
    /// The property's scalar type (`string`, `boolean`, `integer`,
    /// `number`), or `json` for any other schema.
    #[serde(rename = "type")]
    kind: &'static str,
    /// Whether a call may omit the field, which holds when the schema's
    /// `required` does not list it.
    optional: bool,
    /// The property's `default`; `null` when absent.
    default: Option<Value>,
    /// The property's `description`; `null` when absent.
    description: Option<String>,
}

impl ArgDto {
    /// Builds the field for the property `name` with the schema `property`.
    fn new(name: &str, property: &Value, required: bool) -> Self {
        let kind = match property.get("type").and_then(Value::as_str) {
            Some("string") => "string",
            Some("boolean") => "boolean",
            Some("integer") => "integer",
            Some("number") => "number",
            _ => "json",
        };
        Self {
            name: name.to_owned(),
            kind,
            optional: !required,
            default: property.get("default").cloned(),
            description: property
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_owned),
        }
    }
}

/// One declared model role.
#[derive(Debug, Serialize)]
struct ModelDto {
    /// The prompt-local role label.
    label: String,
    /// The declared keywords (kebab-case wire vocabulary).
    keywords: Vec<&'static str>,
    /// The minimum context window in tokens; `null` when absent.
    min_context: Option<u32>,
    /// The role's prose description; `null` when absent.
    description: Option<String>,
}

impl ModelDto {
    /// Builds the DTO for the role declared under `label`.
    fn new(label: &str, role: &ModelRole) -> Self {
        Self {
            label: label.to_owned(),
            keywords: role.keywords().iter().copied().map(keyword_wire).collect(),
            min_context: role.min_context().map(std::num::NonZeroU32::get),
            description: role.description().map(str::to_owned),
        }
    }
}

/// The kebab-case wire form of a model keyword.
fn keyword_wire(keyword: ModelKeyword) -> &'static str {
    match keyword {
        ModelKeyword::Thinking => "thinking",
        ModelKeyword::NoThinking => "no-thinking",
        ModelKeyword::Frontier => "frontier",
        ModelKeyword::Fast => "fast",
        ModelKeyword::Small => "small",
        ModelKeyword::Creative => "creative",
        ModelKeyword::Chat => "chat",
        // Any other keyword has no wire form of its own.
        _ => "unknown",
    }
}

impl From<&Frontmatter> for ContractResponse {
    fn from(frontmatter: &Frontmatter) -> Self {
        Self {
            name: frontmatter.name().to_owned(),
            description: frontmatter.description().to_owned(),
            runvil: frontmatter.runvil(),
            max_tool_iterations: frontmatter
                .max_tool_iterations()
                .map(std::num::NonZeroU32::get),
            input: frontmatter.input().map(FileDto::from),
            output: frontmatter.output().map(FileDto::from),
            uses: frontmatter.uses().iter().map(PackDto::from).collect(),
            args: ArgsDto::new(frontmatter.input_schema()),
            models: frontmatter
                .models()
                .iter()
                .map(|(label, role)| ModelDto::new(label, role))
                .collect(),
        }
    }
}

/// Parses the posted prompt text and answers the contract DTO, or a
/// `422` envelope when the text is not a valid prompt.
async fn contract(Json(body): Json<ContractRequest>) -> Response {
    // The contract needs the tree alone; the parse-time events are not
    // this route's to log.
    match Prompt::parse(&body.text, &body.name).0 {
        Ok(prompt) => (
            StatusCode::OK,
            Json(ContractResponse::from(prompt.frontmatter())),
        )
            .into_response(),
        Err(error) => AppError::prompt_parse(&error).into_response(),
    }
}

#[cfg(test)]
#[path = "prompts-tests.rs"]
mod tests;
