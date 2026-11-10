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
use runvil::model::ModelRole;
use runvil::packs::PackRef;
use runvil::prompt::{ArgDecl, ArgsDecl, FileDecl, Frontmatter, RoleRequirements};

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
    /// The typed args declaration.
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

/// The typed args declaration.
#[derive(Debug, Serialize)]
struct ArgsDto {
    /// True when the declaration is the implicit default (no `args:` key).
    implicit: bool,
    /// The declared fields, sorted by name.
    fields: Vec<ArgDto>,
}

impl From<&ArgsDecl> for ArgsDto {
    fn from(decl: &ArgsDecl) -> Self {
        Self {
            implicit: decl.is_default(),
            fields: decl
                .iter()
                .map(|(name, arg)| ArgDto::new(name, arg))
                .collect(),
        }
    }
}

/// One declared arg.
#[derive(Debug, Serialize)]
struct ArgDto {
    /// The arg name.
    name: String,
    /// The declared type (`string`, `boolean`, `integer`, `number`).
    #[serde(rename = "type")]
    kind: String,
    /// Whether a call may omit the field.
    optional: bool,
    /// The declared default; `null` when absent.
    default: Option<serde_json::Value>,
    /// The human-readable description; `null` when absent.
    description: Option<String>,
}

impl ArgDto {
    /// Builds the DTO for the arg declared under `name`.
    fn new(name: &str, decl: &ArgDecl) -> Self {
        Self {
            name: name.to_owned(),
            kind: decl.kind().to_string(),
            optional: decl.is_optional(),
            // A declared default is validated against the declared type
            // at parse, so it is always a JSON-representable scalar.
            default: decl
                .default()
                .and_then(|value| serde_json::to_value(value).ok()),
            description: decl.description().map(str::to_owned),
        }
    }
}

/// The requirements a prompt declares for one model role.
#[derive(Debug, Serialize)]
struct ModelDto {
    /// The role name.
    label: String,
    /// The minimum context window in tokens; `null` when absent.
    min_context: Option<u32>,
    /// The thinking requirement; `null` when absent.
    thinking: Option<bool>,
}

impl ModelDto {
    /// Builds the DTO for the requirements declared for `role`.
    fn new(role: ModelRole, declared: RoleRequirements) -> Self {
        Self {
            label: role.as_str().to_owned(),
            min_context: declared.min_context().map(std::num::NonZeroU32::get),
            thinking: declared.thinking(),
        }
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
            args: ArgsDto::from(frontmatter.args()),
            models: ModelRole::ALL
                .into_iter()
                .filter_map(|role| {
                    frontmatter
                        .role_requirements(role)
                        .map(|declared| ModelDto::new(role, *declared))
                })
                .collect(),
        }
    }
}

/// Parses the posted prompt text and answers the contract DTO, or a
/// `422` envelope when the text is not a valid prompt.
async fn contract(Json(body): Json<ContractRequest>) -> Response {
    match Prompt::parse(&|_: &str| Ok(body.text.clone()), &body.name) {
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
