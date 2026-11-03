//! The `Grep` tool: a regular-expression search over the files under a
//! path.
//!
//! Grep walks the tree through the call's [`Access`], so the run's mode
//! and policy apply, and reads each file whole, never more than 16 MiB of
//! it. Files come in path order, the order Glob uses. Output stops at
//! 15,000 bytes, and every call runs from the start, so a call with an
//! `offset` pages through the same result. An entry that cannot be
//! searched is skipped and counted, never a failure. The text of the
//! files is returned as untrusted output.

mod args;
mod find;
mod found;
mod matching;
mod pause;
mod render;
mod source;
mod types;
mod window;

#[cfg(test)]
mod tests;

use runvil_plugin::{Access, Error, ErrorCode, ToolOutput, tool_error};
use serde_json::Value;

use crate::spec::Definition;

use self::args::Request;
use self::found::Limits;
use self::source::Source;

/// Runs one `Grep` call whose arguments already passed the shared schema
/// check.
///
/// # Errors
/// Returns an invalid-arguments [`Error`] naming the parameter for a
/// bad argument, and a message error for a root that cannot be searched,
/// worded for a model to read.
pub(crate) async fn call(
    access: &Access,
    definition: &Definition,
    args: &Value,
) -> Result<ToolOutput, Error> {
    let request = args::parse(definition, args)?;
    search(access, &definition.wire_name, &request, &Limits::default()).await
}

/// Searches `source` for what `request` asks, under `limits`. `tool` is
/// the name the model called the tool by.
async fn search<S: Source>(
    source: &S,
    tool: &str,
    request: &Request<'_>,
    limits: &Limits,
) -> Result<ToolOutput, Error> {
    let found = find::find(source, tool, request, limits)
        .await
        .map_err(|text| tool_error(ErrorCode::ToolOther, text))?;
    Ok(render::render(&found, request, limits))
}
