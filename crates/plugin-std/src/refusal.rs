//! The sentence a model reads when a file operation fails.
//!
//! Every file tool turns a [`VfsError`] into one of these, so the same
//! failure reads the same in each tool. The path in a sentence is always
//! the path as the model wrote it, never the canonical path the error
//! holds, so a relative `notes/a.md` is not echoed back as `/notes/a.md`.

use runvil_plugin::VfsError;

/// The model-readable sentence for `error`, from the tool called `tool`
/// (the name the model uses) on `path` (as the model wrote it).
///
/// A sentence names no tool other than `tool`.
pub(crate) fn refusal(tool: &str, path: &str, error: &VfsError) -> String {
    match error {
        VfsError::NotFound { .. } => format!("File not found: {path}"),
        VfsError::IsADirectory { .. } => format!("Path is a directory, not a file: {path}"),
        VfsError::NotADirectory { .. } => not_a_directory(path),
        VfsError::Conflict { .. } => format!(
            "{path} was changed, or is being changed, by another call in this round, so it \
             cannot be read in this round. Call {tool} again in your next message."
        ),
        VfsError::Io { kind, message, .. } => format!("I/O failure on {path} ({kind}): {message}"),
        VfsError::PermissionDenied { reason, .. } | VfsError::ApprovalRequired { reason, .. } => {
            reason.clone()
        }
        other => without_label(&other.to_string()),
    }
}

/// `text` without its leading label: the text after the first `": "`, or
/// the whole text when it has none.
fn without_label(text: &str) -> String {
    text.split_once(": ")
        .map_or(text, |(_, rest)| rest)
        .to_owned()
}

/// The sentence for a root the model named that does not exist.
///
/// A tool that starts from a directory words its own missing root, because
/// the `NotFound` sentence above says a file was not found.
pub(crate) fn missing_root(path: &str) -> String {
    format!("Path does not exist: {path}")
}

/// The sentence for a path that names something other than a directory
/// where a directory was needed.
pub(crate) fn not_a_directory(path: &str) -> String {
    format!("Path is not a directory: {path}")
}

/// The sentence for a path that is neither a file nor a directory, such
/// as a pipe or a device.
pub(crate) fn neither_file_nor_directory(path: &str) -> String {
    format!("Path is neither a file nor a directory: {path}")
}

#[cfg(test)]
mod tests;
