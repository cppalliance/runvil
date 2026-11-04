//! The failures this crate raises that the Engine raises too, through the
//! same constructors, so each failure has one text.

use std::borrow::Cow;
use std::num::NonZeroU32;

use runvil_types::error::{Error, ErrorCode, FactKey, Location, failure, with_fact};
use runvil_types::ids::TaskId;

use super::QuotaResource;

#[cfg(test)]
#[path = "error-universal-tests.rs"]
mod tests;

/// A condition the prompt author caused and can fix in Lua, such as an
/// empty `messages` list or a heading that names no section, as a
/// `LuaRuntime` failure. The message is the specific failure as a noun
/// phrase, with no `lua error:` label, since a failed run classifies it as
/// a Lua failure.
#[must_use]
pub fn lua_runtime(message: impl Into<String>) -> Error {
    failure(ErrorCode::LuaRuntime, message)
}

/// The failure of a run that was cancelled, of a call whose effect the
/// caller dropped, or of a block that raised a caught `cancelled` error
/// value with no `task` again after another suspending call.
#[must_use]
pub fn interrupted() -> Error {
    failure(
        ErrorCode::Interrupted,
        "interrupted: the run was cancelled or this call was stopped",
    )
}

/// `compactors.fail` exhausted the model's context window, with `reason`:
/// `precheck` or `provider` after an overflow, else the caller's own. The
/// `Reason` fact is `reason`.
#[must_use]
pub fn context_exhausted(reason: &str) -> Error {
    let text = format!(
        "context exhausted: {}",
        crate::compactors::exhaustion_detail(reason)
    );
    with_fact(
        failure(ErrorCode::ContextExhausted, text),
        FactKey::Reason,
        reason,
    )
}

/// An exhausted Lua quota on `resource`, whose tag (`log_event` or
/// `log_byte`) is the `Reason` fact.
#[must_use]
pub fn lua_quota(resource: QuotaResource) -> Error {
    let tag = match resource {
        QuotaResource::LogEvent => "log_event",
        QuotaResource::LogByte => "log_byte",
    };
    with_fact(
        failure(
            ErrorCode::LuaQuota,
            format!("lua {resource} quota exceeded"),
        ),
        FactKey::Reason,
        tag,
    )
}

/// A task operation that named `task`, which the caller does not own. An
/// id that names no task at all is refused the same way, so a caller
/// learns nothing about tasks it never started.
#[must_use]
pub fn task_not_owned(task: &TaskId) -> Error {
    task_failure(
        ErrorCode::TaskNotOwned,
        format!("task `{task}` is not a task this chain owns"),
        task,
    )
}

/// A wait that named `task`, whose result was already delivered once.
#[must_use]
pub fn task_consumed(task: &TaskId) -> Error {
    task_failure(
        ErrorCode::TaskConsumed,
        format!("task `{task}` was already delivered: a task's result is taken by one wait"),
        task,
    )
}

/// The failure a wait delivers for `task`, which its owner cancelled
/// instead of letting it end on its own.
#[must_use]
pub fn task_cancelled(task: &TaskId) -> Error {
    task_failure(
        ErrorCode::TaskCancelled,
        format!("task `{task}` was cancelled"),
        task,
    )
}

/// A failure with `code` and `message` about `task`, its `Task` fact.
fn task_failure(code: ErrorCode, message: String, task: &TaskId) -> Error {
    with_fact(failure(code, message), FactKey::Task, task.to_string())
}

/// The tool-call loop ran its iteration cap without a final text reply.
#[must_use]
pub fn tool_loop_exhausted() -> Error {
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
) -> Error {
    let text = format!("internal invariant violated: {}", message.into());
    let mut error = failure(ErrorCode::Internal, text);
    error.at = NonZeroU32::new(site.line()).map(|line| Location::line(site.file(), line));
    error
}

/// Lua source at the prompt region `region`, which starts at prompt line
/// `source_line`, that did not compile, with the compiler's `diagnostic`.
/// The `Section` fact names the region; the compile path gives it its
/// location and prose.
pub(crate) fn lua_compile(region: &str, source_line: u32, diagnostic: &str) -> Error {
    with_fact(
        failure(
            ErrorCode::LuaCompile,
            format!("lua compilation error at {region} (line {source_line}): {diagnostic}"),
        ),
        FactKey::Section,
        region,
    )
}

/// A dispatched tool's own failure, `error`, under a message that names
/// the tool call and its model-safe prose; the prose, location, and facts
/// stay the tool's. A code outside the tool codes is `ToolOther`, so a
/// tool's failure always classifies as one.
pub(crate) fn tool(mut error: Error) -> Error {
    if !matches!(
        error.code,
        ErrorCode::ToolInvalidArguments
            | ErrorCode::ToolBackend
            | ErrorCode::ToolTransport
            | ErrorCode::ToolOther
    ) {
        error.code = ErrorCode::ToolOther;
    }
    error.message = format!("tool call failure: {}", error.prose);
    error
}
