//! A raised Lua error table, read back as a [`Raised`], as the universal
//! failure its kind names.

use runvil_types::error::{Error as Universal, ErrorCode, FactKey};
use runvil_types::ids::TaskId;
use serde_json::Value;

use super::{
    context_exhausted, interrupted, lua_runtime, task_cancelled, task_consumed, task_not_owned,
    tool_failure, tool_loop_exhausted, with_fact,
};
use crate::error_value::{ErrorField, ErrorKind, Raised};

/// The field `name` of a raised error table, when it is a string.
fn raised_field<'a>(raised: &'a Raised, name: &str) -> Option<&'a str> {
    raised.fields.get(name).and_then(ErrorField::as_str)
}

/// The `task` field of a raised task-error table, when it parses.
fn raised_task(raised: &Raised) -> Option<TaskId> {
    raised_field(raised, "task").and_then(|task| task.parse().ok())
}

/// Maps a structured error table that surfaced as a block's failure onto
/// the failure its kind names, so a Lua-side raise classifies as the
/// Rust-raised failure it stands in for, as
/// [`into_universal`](super::into_universal) documents.
pub(super) fn from_raised(raised: Raised) -> Universal {
    match raised.kind {
        ErrorKind::ToolLoopExhausted => tool_loop_exhausted(),
        ErrorKind::ContextExhausted => match raised.exhaustion_reason() {
            Some(reason) => context_exhausted(reason),
            None => lua_runtime(raised.message),
        },
        ErrorKind::EmptyModelReply => {
            let error = runvil_model_client::model::completion_error(
                ErrorCode::EmptyReply,
                raised.message.clone(),
            );
            match raised_field(&raised, "finish_reason") {
                Some(reason) => with_fact(error, FactKey::FinishReason, reason),
                None => error,
            }
        }
        ErrorKind::Cancelled => match raised_task(&raised) {
            Some(task) => task_cancelled(&task),
            None => interrupted(),
        },
        ErrorKind::Tool => tool_failure(&raised.message, None),
        ErrorKind::TaskNotOwned => match raised_task(&raised) {
            Some(task) => task_not_owned(&task),
            None => lua_runtime(raised.message),
        },
        ErrorKind::TaskConsumed => match raised_task(&raised) {
            Some(task) => task_consumed(&task),
            None => lua_runtime(raised.message),
        },
        ErrorKind::Store => raised_store_error(&raised),
        ErrorKind::OutOfScopeTool
        | ErrorKind::UnboundTool
        | ErrorKind::ModelOnlyTool
        | ErrorKind::TasksLive
        | ErrorKind::Lua
        | ErrorKind::ModelFailure
        | ErrorKind::ModelRequired
        | ErrorKind::Internal => lua_runtime(raised.message),
    }
}

/// Rebuilds the store failure a raised store table stands in for: the code
/// its `reason` tag names, the table's message, and the facts its fields
/// give, as [`store_error`](crate::store_error) would from the structured
/// failure the table was built from. The reconstruction is exact where the
/// fields round-trip; a malformed table degrades rather than panics: an
/// unknown tag is `Backend`, every code but `NoStore` and `Backend` keeps a
/// `path` (empty when the table has none), a missing anchor is empty and
/// its count 0, a missing UTF-8 offset is 0, and a rule that names no path
/// rule is `empty`.
fn raised_store_error(raised: &Raised) -> Universal {
    let store = ErrorCode::NotFound.number()..=ErrorCode::Backend.number();
    let code = raised_field(raised, "reason")
        .and_then(ErrorCode::from_tag)
        .filter(|code| store.contains(&code.number()))
        .unwrap_or(ErrorCode::Backend);
    let mut error = Universal::new(code, raised.message.clone(), code.prose());
    if matches!(code, ErrorCode::NoStore | ErrorCode::Backend) {
        return error;
    }
    let text = |name: &str| raised_field(raised, name).unwrap_or_default().to_owned();
    let integer = |name: &str| {
        raised
            .fields
            .get(name)
            .and_then(ErrorField::as_integer)
            .filter(|number| *number >= 0)
            .unwrap_or(0)
    };
    error.facts.insert(FactKey::Path, Value::from(text("path")));
    match code {
        ErrorCode::Anchor => {
            error
                .facts
                .insert(FactKey::Anchor, Value::from(text("anchor")));
            error
                .facts
                .insert(FactKey::Count, Value::from(integer("count")));
        }
        ErrorCode::InvalidPath => {
            let rule = raised_field(raised, "rule")
                .and_then(runvil_vfs::PathReason::from_tag)
                .unwrap_or(runvil_vfs::PathReason::Empty);
            error.facts.insert(FactKey::Rule, Value::from(rule.tag()));
        }
        ErrorCode::NotUtf8 => {
            error
                .facts
                .insert(FactKey::ValidUpTo, Value::from(integer("valid_up_to")));
        }
        _ => {}
    }
    error
}
