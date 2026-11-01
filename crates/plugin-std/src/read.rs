//! The `Read` tool: a window of a text file's lines, numbered.
//!
//! Read opens the path the call names through the call's [`Access`], so
//! the run's mode and policy apply. It refuses a directory, a special
//! file, a file over 16 MiB, and binary data, and it never truncates a
//! file: a read that does not reach the last line says which offset
//! continues it. File text is returned as untrusted output.

mod args;
mod render;
mod window;

#[cfg(test)]
mod tests;

use runvil_plugin::{Access, FileType, ToolError, ToolOutput, VfsError};
use serde_json::Value;

use crate::check::plural;
use crate::refusal::{neither_file_nor_directory, refusal};
use crate::spec::Definition;
use crate::text::{MAX_FILE_BYTES, decode, sniff_binary, split_lines};

use self::args::Request;

/// The most bytes of numbered lines one read returns: each line's prefix,
/// its text, and its newline count, and the notes and the trailer do not.
pub(crate) const BYTE_CAP: usize = 100_000;

/// Runs one `Read` call whose arguments already passed the shared schema
/// check.
///
/// # Errors
/// Returns an invalid-arguments [`ToolError`] naming the parameter for a
/// bad argument, and a message error for any other failure, worded for a
/// model to read.
pub(crate) fn call(
    access: &Access,
    definition: &Definition,
    args: &Value,
) -> Result<ToolOutput, ToolError> {
    let request = args::parse(definition, args)?;
    read_file(access, &definition.wire_name, &request, BYTE_CAP)
}

/// Reads the file `request` names, in the window it asks for, with at
/// most `cap` bytes of numbered lines. `tool` is the name the model
/// called the tool by.
fn read_file(
    access: &Access,
    tool: &str,
    request: &Request<'_>,
    cap: usize,
) -> Result<ToolOutput, ToolError> {
    let path = request.path;
    let failed = |error: VfsError| ToolError::message(refusal(tool, path, &error));
    let stat = access.stat(path).map_err(failed)?;
    check_type(path, stat.file_type).map_err(ToolError::message)?;
    let bytes = access
        .read_range(path, 0, MAX_FILE_BYTES + 1)
        .map_err(failed)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_FILE_BYTES {
        return Err(ToolError::message(format!(
            "File is too large to read: {path} is over the limit of {MAX_FILE_BYTES} bytes."
        )));
    }
    if sniff_binary(&bytes) {
        return Err(ToolError::message(format!(
            "File content is binary data, not text: {path}"
        )));
    }
    let decoded = decode(&bytes);
    let lines = split_lines(&decoded.text);
    if lines.is_empty() {
        return Ok(ToolOutput::untrusted("File is empty."));
    }
    let Some(window) = window::window(lines.len(), request.offset, request.limit) else {
        return Err(ToolError::message(format!(
            "Offset {} is beyond the end of {path}, which has {}.",
            request.offset.unwrap_or_default(),
            plural(lines.len(), "line")
        )));
    };
    let notes = render::notes(decoded.utf16, decoded.invalid);
    Ok(ToolOutput::untrusted(render::render(
        &lines, &window, &notes, cap,
    )))
}

/// Decides from the kind of node at `path` whether Read goes on: a file
/// or a link does, and anything else gets the sentence that says why not.
fn check_type(path: &str, file_type: FileType) -> Result<(), String> {
    match file_type {
        FileType::File | FileType::Symlink => Ok(()),
        FileType::Directory => Err(format!("Path is a directory, not a file: {path}")),
        // A pipe, a socket, or a device, and any kind added later: none
        // of them is text Read can window.
        _ => Err(neither_file_nor_directory(path)),
    }
}
