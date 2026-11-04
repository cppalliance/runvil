//! The crate's failures, each the universal [`Error`].
//!
//! A site builds its failure where it raises it, with the constructors
//! here, which the Engine shares for the failures it raises itself, so
//! each failure has one text. A failure raised through Lua crosses the
//! `mlua` boundary as a typed value, `mlua::Error::external(error)`, and
//! the boundary that reads it back downcasts to [`Error`]. A failure that
//! crosses into Lua is placed on the author's line of the prompt's file,
//! which the VM holds, and one the prompt's own Lua caused gets author
//! prose that names what failed.

use std::borrow::Cow;

pub(crate) use runvil_types::error::Error;

#[path = "error-author.rs"]
mod author;
#[path = "error-universal.rs"]
mod universal;

#[cfg(any(test, feature = "test-support"))]
pub(crate) use author::UNNAMED_FILE;
pub(crate) use author::{
    author_line, author_prose, authored, authored_at, call_line, failure_text, prompt_file,
    set_call_line, set_prompt_file,
};
pub use universal::{
    context_exhausted, internal_at, interrupted, lua_quota, lua_runtime, task_cancelled,
    task_consumed, task_not_owned, tool_loop_exhausted,
};
pub(crate) use universal::{lua_compile, tool};

/// An Engine quota on Lua that a block can exhaust.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaResource {
    /// The per-VM `log()` event budget.
    LogEvent,
    /// The cumulative `log()` byte budget.
    LogByte,
}

impl std::fmt::Display for QuotaResource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            QuotaResource::LogEvent => "log event",
            QuotaResource::LogByte => "log byte",
        })
    }
}

/// The `LuaRuntime` failure an `mlua` failure is, with the `mlua` error's
/// text as its message.
#[expect(
    clippy::needless_pass_by_value,
    reason = "the adapter every VM call's `map_err` passes its error to"
)]
pub(crate) fn lua(source: mlua::Error) -> Error {
    lua_runtime(source.to_string())
}

/// An internal invariant failure, at the call site as its construction
/// site. The message is owned when it names a value.
#[must_use]
#[track_caller]
pub fn internal(message: impl Into<Cow<'static, str>>) -> Error {
    internal_at(message, std::panic::Location::caller())
}

/// An author error raised across the Lua boundary: a `LuaRuntime`
/// failure with `message`, which `pcall` reports as kind `lua`.
pub(crate) fn author_refusal(message: String) -> mlua::Error {
    mlua::Error::external(lua_runtime(message))
}

/// Strips `mlua`'s wrapping layers (the callback frame and any context)
/// down to the error a callback returned.
pub(crate) fn root_cause(error: &mlua::Error) -> &mlua::Error {
    match error {
        mlua::Error::CallbackError { cause, .. } | mlua::Error::WithContext { cause, .. } => {
            root_cause(cause)
        }
        other => other,
    }
}

/// The text of the failure at the root of `error`, without `mlua`'s label
/// for its kind: what Lua raised, or the message of the universal failure
/// a callback raised.
pub(crate) fn root_text(error: &mlua::Error) -> String {
    match root_cause(error) {
        mlua::Error::RuntimeError(message) | mlua::Error::SyntaxError { message, .. } => {
            message.clone()
        }
        other => other
            .downcast_ref::<Error>()
            .map_or_else(|| other.to_string(), |typed| typed.message.clone()),
    }
}

/// Crate-internal result alias over [`Error`].
pub(crate) type Result<T> = std::result::Result<T, Error>;

/// Renders `tags` as the accepted-value list a parse error names: each tag
/// in backticks, joined by commas.
pub(crate) fn accepted_tags(tags: impl IntoIterator<Item = &'static str>) -> String {
    tags.into_iter()
        .map(|tag| format!("`{tag}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
#[path = "error-tests.rs"]
mod tests;
