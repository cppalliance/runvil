//! Where a failure crossing into a VM's Lua is placed, and its author
//! prose. The place is a line of the file the VM holds for the prompt its
//! Lua came from: the line of the innermost author frame running, or else
//! the call line, the author's line at the block's latest yield. Author
//! prose is the text the model reads for a failure the prompt's own Lua
//! caused: what failed, then the code's default prose.

use std::num::NonZeroU32;

use mlua::Lua;
use runvil_types::error::{Error, ErrorCode, place};

use crate::chunk_line::{STRING_CHUNK_OPENING, prompt_line};

/// The file label of a VM that was given none: the label a prompt that
/// came from no file has.
pub(crate) const UNNAMED_FILE: &str = "prompt";

/// Where a VM's failures are placed, held as the VM's app data.
struct PromptFile {
    /// The file the prompt the VM runs came from.
    file: String,
    /// The author's line at the running block's latest yield.
    call_line: Option<NonZeroU32>,
}

/// Records `file` as the file the prompt this VM runs came from, which
/// its failures are placed in.
pub(crate) fn set_prompt_file(lua: &Lua, file: &str) {
    if let Some(mut held) = lua.app_data_mut::<PromptFile>() {
        file.clone_into(&mut held.file);
        return;
    }
    lua.set_app_data(PromptFile {
        file: file.to_owned(),
        call_line: None,
    });
}

/// The file the prompt this VM runs came from, or `prompt` when the VM
/// was given none.
pub(crate) fn prompt_file(lua: &Lua) -> String {
    lua.app_data_ref::<PromptFile>()
        .map_or_else(|| UNNAMED_FILE.to_owned(), |held| held.file.clone())
}

/// Records `line` as this VM's call line, `None` included, so the line of
/// an earlier yield never outlives the one it belongs to.
pub(crate) fn set_call_line(lua: &Lua, line: Option<NonZeroU32>) {
    if let Some(mut held) = lua.app_data_mut::<PromptFile>() {
        held.call_line = line;
        return;
    }
    lua.set_app_data(PromptFile {
        file: UNNAMED_FILE.to_owned(),
        call_line: line,
    });
}

/// The author's line at the latest yield of the block this VM runs, which
/// holds while the block is suspended on that yield.
pub(crate) fn call_line(lua: &Lua) -> Option<NonZeroU32> {
    lua.app_data_ref::<PromptFile>()
        .and_then(|held| held.call_line)
}

/// The prompt line of the innermost frame on the running Lua stack whose
/// chunk is a prompt block, `line N`, skipping every other frame: a
/// shim's, a Pack prelude's, a Rust callback's. `None` when no such frame
/// runs, as when the caller renders an answer outside any Lua call.
pub(crate) fn author_line(lua: &Lua) -> Option<NonZeroU32> {
    (0..)
        .map_while(|level| {
            lua.inspect_stack(level, |frame| {
                let chunk_name = frame.source().source?;
                let line = u32::try_from(frame.current_line()?).ok()?;
                prompt_line(&chunk_name, line)
            })
        })
        .flatten()
        .next()
}

/// Whether a failure with `code` is one the prompt's Lua causes as it
/// runs, whose prose names what failed: a Lua failure, an exhausted quota,
/// the call-depth cap, a failed substitution, or a misused task. A direct
/// `store` closure gives its own failure that prose itself.
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

/// The author prose of a failure with `code`: `{what}. {default}`, where
/// `what` is what failed and `default` the code's prose.
pub(crate) fn author_prose(what: &str, code: ErrorCode) -> String {
    format!("{what}. {}", code.prose())
}

/// Places `error` on `line` of `file` as [`place`] does. When the prompt's
/// Lua caused the failure and its prose is still its code's default, the
/// prose becomes the author prose for `what`.
pub(crate) fn authored_at(error: Error, file: &str, line: Option<NonZeroU32>, what: &str) -> Error {
    let mut error = place(error, file, line);
    if is_authored(error.code) && error.prose == error.code.prose() {
        error.prose = author_prose(what, error.code);
    }
    error
}

/// `error` as it crosses into this VM's Lua: placed in the VM's file on
/// the line of the innermost author frame running, or else on the call
/// line, with the author prose of its message's own text when the
/// prompt's Lua caused it.
pub(crate) fn authored(lua: &Lua, error: Error) -> Error {
    let line = author_line(lua).or_else(|| call_line(lua));
    let what = failure_text(&error.message).to_owned();
    authored_at(error, &prompt_file(lua), line, &what)
}

/// The heading Lua puts on its own line between a failure's text and its
/// traceback.
pub(crate) const TRACEBACK_HEADING: &str = "\nstack traceback:";

/// The failure's own text in `message`: the text before the traceback,
/// without the `[string "..."]:N: ` positions Lua puts in front of it.
pub(crate) fn failure_text(message: &str) -> &str {
    let mut text = message
        .find(TRACEBACK_HEADING)
        .map_or(message, |at| &message[..at]);
    while let Some(rest) = text.strip_prefix(STRING_CHUNK_OPENING) {
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
