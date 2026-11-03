//! Author prose: the text the model reads for a failure the prompt's own
//! Lua caused, which names the prompt's file, and the line when it is
//! known, before what failed and the code's default prose. The file is the
//! label the VM holds for the prompt its Lua came from.

use std::num::NonZeroU32;

use mlua::Lua;
use runvil_types::error::{Error, ErrorCode, Location};

/// The file label of a VM that was given none: the label a prompt that
/// came from no file has.
pub(crate) const UNNAMED_FILE: &str = "prompt";

/// The file the prompt a VM runs came from, held as the VM's app data.
struct PromptFile(String);

/// Records `file` as the file the prompt this VM runs came from, which
/// the author prose of its failures names.
pub(crate) fn set_prompt_file(lua: &Lua, file: &str) {
    lua.set_app_data(PromptFile(file.to_owned()));
}

/// The file the prompt this VM runs came from, or `prompt` when the VM
/// was given none.
pub(crate) fn prompt_file(lua: &Lua) -> String {
    lua.app_data_ref::<PromptFile>()
        .map_or_else(|| UNNAMED_FILE.to_owned(), |file| file.0.clone())
}

/// Whether a failure with `code` is one the prompt's Lua causes as it
/// runs, whose prose names where: a Lua failure, an exhausted quota, the
/// call-depth cap, a failed substitution, or a misused task. A store
/// failure is one only when a direct `store` closure raised it, which
/// gives it its prose itself.
fn is_authored(code: ErrorCode) -> bool {
    matches!(
        code,
        ErrorCode::LuaRuntime
            | ErrorCode::LuaCompile
            | ErrorCode::LuaQuota
            | ErrorCode::CallDepthExceeded
            | ErrorCode::Substitution
            | ErrorCode::TasksLive
            | ErrorCode::TaskNotOwned
            | ErrorCode::TaskConsumed
    )
}

/// The prose of a failure with `code` in `file`: `{file}:{line}: {what}.
/// {default}` when the line is known, and `{file}: {what}. {default}`
/// otherwise, where `what` is what failed and `default` the code's prose.
fn author_prose(file: &str, line: Option<NonZeroU32>, what: &str, code: ErrorCode) -> String {
    match line {
        Some(line) => format!("{file}:{line}: {what}. {}", code.prose()),
        None => format!("{file}: {what}. {}", code.prose()),
    }
}

/// Gives `error` the author prose for `what` in `file`, and, when `line`
/// is known, the location of that line of `file`.
pub(crate) fn authored_at(
    mut error: Error,
    file: &str,
    line: Option<NonZeroU32>,
    what: &str,
) -> Error {
    error.at = line.map(|line| Location::line(file, line));
    error.prose = author_prose(file, line, what, error.code);
    error
}

/// `error` as it crosses into this VM's Lua: a failure the prompt's Lua
/// caused that has no location yet gets the author prose of its message's
/// own text in the VM's file; a located one already has its prose, and
/// every other failure keeps its own.
pub(crate) fn authored(lua: &Lua, error: Error) -> Error {
    if error.at.is_some() || !is_authored(error.code) {
        return error;
    }
    let what = failure_text(&error.message).to_owned();
    authored_at(error, &prompt_file(lua), None, &what)
}

/// The failure's own text in `message`: the text before the traceback,
/// without the `[string "..."]:N: ` positions Lua puts in front of it.
pub(crate) fn failure_text(message: &str) -> &str {
    let mut text = message
        .find("\nstack traceback:")
        .map_or(message, |at| &message[..at]);
    while let Some(rest) = text.strip_prefix("[string \"") {
        let Some(close) = rest.find("\"]:") else {
            break;
        };
        let after = &rest[close + 3..];
        let digits = after
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(after.len());
        match after[digits..].strip_prefix(": ") {
            Some(tail) if digits > 0 => text = tail,
            _ => break,
        }
    }
    text.trim_end()
}

#[cfg(test)]
#[path = "error-author-tests.rs"]
mod tests;
