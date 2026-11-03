//! The universal error this crate's own error becomes where it leaves the
//! crate, and the failures that conversion builds. The Engine raises the
//! same failures itself through these constructors, so each failure has
//! one text, and where this crate's error has a variant for it, that text
//! is the variant's own display.

use std::borrow::Cow;
use std::num::NonZeroU32;

use runvil_types::error::{Error as Universal, ErrorCode, FactKey, Location};
use runvil_types::ids::TaskId;
use serde_json::Value;

use super::{Error, QuotaResource};
use crate::error_value::store_error;

#[path = "error-universal-raised.rs"]
mod raised;
#[cfg(test)]
#[path = "error-universal-tests.rs"]
mod tests;

/// Builds a failure with `code`, `message`, and the code's default prose.
fn failure(code: ErrorCode, message: impl Into<String>) -> Universal {
    Universal::new(code, message, code.prose())
}

/// Gives `error` the fact `key` with `value`.
fn with_fact(mut error: Universal, key: FactKey, value: impl Into<Value>) -> Universal {
    error.facts.insert(key, value.into());
    error
}

/// A condition the prompt author caused and can fix in Lua, such as an
/// empty `messages` list or a heading that names no section, as a
/// `LuaRuntime` failure. The message is the specific failure as a noun
/// phrase, with no `lua error:` label, since a failed run classifies it as
/// a Lua failure.
#[must_use]
pub fn lua_runtime(message: impl Into<String>) -> Universal {
    failure(ErrorCode::LuaRuntime, message)
}

/// The failure of a run that was cancelled, of a call whose effect the
/// caller dropped, or of a block that raised a caught `cancelled` error
/// value with no `task` again after another suspending call.
#[must_use]
pub fn interrupted() -> Universal {
    failure(ErrorCode::Interrupted, Error::Interrupted.to_string())
}

/// `compactors.fail` exhausted the model's context window, with `reason`:
/// `precheck` or `provider` after an overflow, else the caller's own. The
/// `Reason` fact is `reason`.
#[must_use]
pub fn context_exhausted(reason: &str) -> Universal {
    let text = Error::ContextExhausted {
        reason: reason.to_owned(),
    }
    .to_string();
    with_fact(
        failure(ErrorCode::ContextExhausted, text),
        FactKey::Reason,
        reason,
    )
}

/// An exhausted Lua quota on `resource`, whose tag (`log_event` or
/// `log_byte`) is the `Reason` fact.
#[must_use]
pub fn lua_quota(resource: QuotaResource) -> Universal {
    let tag = match resource {
        QuotaResource::LogEvent => "log_event",
        QuotaResource::LogByte => "log_byte",
    };
    with_fact(
        failure(
            ErrorCode::LuaQuota,
            Error::LuaQuota { resource }.to_string(),
        ),
        FactKey::Reason,
        tag,
    )
}

/// A task operation that named `task`, which the caller does not own. An
/// id that names no task at all is refused the same way, so a caller
/// learns nothing about tasks it never started.
#[must_use]
pub fn task_not_owned(task: &TaskId) -> Universal {
    task_failure(
        ErrorCode::TaskNotOwned,
        format!("task `{task}` is not a task this chain owns"),
        task,
    )
}

/// A wait that named `task`, whose result was already delivered once.
#[must_use]
pub fn task_consumed(task: &TaskId) -> Universal {
    task_failure(
        ErrorCode::TaskConsumed,
        format!("task `{task}` was already delivered: a task's result is taken by one wait"),
        task,
    )
}

/// The failure a wait delivers for `task`, which its owner cancelled
/// instead of letting it end on its own.
#[must_use]
pub fn task_cancelled(task: &TaskId) -> Universal {
    task_failure(
        ErrorCode::TaskCancelled,
        format!("task `{task}` was cancelled"),
        task,
    )
}

/// A failure with `code` and `message` about `task`, its `Task` fact.
fn task_failure(code: ErrorCode, message: String, task: &TaskId) -> Universal {
    with_fact(failure(code, message), FactKey::Task, task.to_string())
}

/// The tool-call loop ran its iteration cap without a final text reply.
#[must_use]
pub fn tool_loop_exhausted() -> Universal {
    failure(
        ErrorCode::ToolLoopExhausted,
        "tool-call loop did not converge",
    )
}

/// An internal invariant failure constructed at `site`: a state the
/// surrounding code has already guaranteed cannot occur, surfaced rather
/// than skipped. Its location is that Rust source position, which a failed
/// run's message ends with. The message is owned when it names a value.
#[must_use]
pub fn internal_at(
    message: impl Into<Cow<'static, str>>,
    site: &'static std::panic::Location<'static>,
) -> Universal {
    let text = Error::Internal {
        message: message.into(),
        location: site,
    }
    .to_string();
    let mut error = failure(ErrorCode::Internal, text);
    error.at = NonZeroU32::new(site.line()).map(|line| Location::line(site.file(), line));
    error
}

/// Returns the universal error `error` becomes where it leaves this crate,
/// variant for variant: an authoring or runtime failure is `LuaRuntime`, a
/// compile failure `LuaCompile` at its region's line, a quota `LuaQuota`,
/// context exhaustion `ContextExhausted`, a cancellation `Interrupted`, a
/// tool's failure the tool's own error under a message that names the
/// call, a store failure the store's code and facts under its own message,
/// and an internal fault `Internal` at its construction site. A raised
/// error table becomes the failure its kind names: a `task_not_owned`,
/// `task_consumed`, or `cancelled` table with a `task` field rebuilds its
/// task failure, and a `cancelled` table without one is an interruption; a
/// `store` table rebuilds the code its `reason` tag names; and a kind whose
/// failure needs structure the table does not hold keeps its message as a
/// `LuaRuntime` failure. Each keeps the text the variant displays.
#[must_use]
pub fn into_universal(error: Error) -> Universal {
    match error {
        Error::LuaCompile {
            ref location,
            source_line,
            ..
        } => lua_compile(location, source_line, error.to_string()),
        Error::Lua(message) | Error::LuaRuntime { message, .. } => lua_runtime(message),
        Error::LuaQuota { resource } => lua_quota(resource),
        Error::ContextExhausted { reason } => context_exhausted(&reason),
        Error::Interrupted => interrupted(),
        Error::Tool { message, cause } => tool_failure(&message, cause.downcast_ref::<Universal>()),
        Error::Store { message, cause } => {
            let mut converted = store_error(&cause);
            converted.message = message;
            converted
        }
        Error::Internal { message, location } => internal_at(message, location),
        Error::Raised(raised) => raised::from_raised(raised),
    }
}

/// Lua source at the prompt region `region`, which starts at prompt line
/// `source_line`, that did not compile, with `message` as its text. The
/// location is that line of the region, and the `Section` fact names the
/// region.
fn lua_compile(region: &str, source_line: u32, message: String) -> Universal {
    let mut error = failure(ErrorCode::LuaCompile, message);
    error.at = NonZeroU32::new(source_line).map(|line| Location::line(region, line));
    with_fact(error, FactKey::Section, region)
}

/// A dispatched tool's own failure, whose model-safe text is `message`:
/// the tool's universal error, `tool`, with its prose, location, and
/// facts, under a message that names the tool call. A code outside the
/// tool codes, and a failure that holds no universal error, is
/// `ToolOther`, so a tool's failure always classifies as one.
fn tool_failure(message: &str, tool: Option<&Universal>) -> Universal {
    let mut error = tool
        .cloned()
        .unwrap_or_else(|| Universal::new(ErrorCode::ToolOther, message, message));
    if !matches!(
        error.code,
        ErrorCode::ToolInvalidArguments
            | ErrorCode::ToolBackend
            | ErrorCode::ToolTransport
            | ErrorCode::ToolOther
    ) {
        error.code = ErrorCode::ToolOther;
    }
    error.message = format!("tool call failure: {message}");
    error
}
