//! The `ListDir` tool: a bullet tree of a directory, listed breadth first.
//!
//! ListDir stats the directory the call names and walks it through the
//! call's [`Access`], so the run's mode and policy apply. The tree holds
//! at most 15,000 bytes of lines: when the next line would pass that, the
//! walk stops and the answer says so. Directories end in `/`, files and
//! directories share one byte-ordered list, entries named `.git` are
//! skipped, and a link is listed and never entered. The names are
//! untrusted output.

mod args;
mod render;
mod walk;

#[cfg(test)]
mod tests;

use runvil_plugin::{Access, Error, ErrorCode, FileType, ToolOutput, VfsError, make_tool_error};
use serde_json::Value;

use crate::refusal::{missing_root, not_a_directory, refusal};
use crate::spec::Definition;
use crate::text::BODY_CAP;

use self::args::Request;

/// Runs one `ListDir` call whose arguments already passed the shared
/// schema check.
///
/// # Errors
/// Returns an invalid-arguments [`Error`] naming the parameter for a
/// bad argument, and a message error for any other failure, worded for a
/// model to read.
pub(crate) fn call(
    access: &Access,
    definition: &Definition,
    args: &Value,
) -> Result<ToolOutput, Error> {
    let request = args::parse(definition, args)?;
    list_directory(access, &definition.wire_name, &request, BODY_CAP)
}

/// Lists the directory `request` names, with at most `cap` bytes of tree
/// lines. `tool` is the name the model called the tool by.
fn list_directory(
    access: &Access,
    tool: &str,
    request: &Request<'_>,
    cap: usize,
) -> Result<ToolOutput, Error> {
    let target = request.target;
    let message = |text: String| make_tool_error(ErrorCode::ToolOther, text);
    let failed = |error: VfsError| message(refusal(tool, target, &error));
    let stat = match access.stat(target) {
        Ok(stat) => stat,
        Err(VfsError::NotFound { .. }) => return Err(message(missing_root(target))),
        Err(error) => return Err(failed(error)),
    };
    check_directory(target, stat.file_type).map_err(message)?;
    let tree = walk::walk(target, &request.ignore, cap, |path| access.list(path))
        .map_err(|failure| message(refusal(tool, &failure.path, &failure.error)))?;
    Ok(ToolOutput::untrusted(render::render(&tree)))
}

/// Decides from the kind of node at `path` whether ListDir goes on: a
/// directory does, and so does a link, which is walked through, and
/// anything else gets the sentence that says it is not a directory.
fn check_directory(path: &str, file_type: FileType) -> Result<(), String> {
    match file_type {
        FileType::Directory | FileType::Symlink => Ok(()),
        _ => Err(not_a_directory(path)),
    }
}
