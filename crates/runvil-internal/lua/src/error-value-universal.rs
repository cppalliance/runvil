//! The universal error as a Lua error value: the kind an author branches
//! on, which its code names, and the kind's fields, which its facts hold,
//! in both directions.

use std::collections::BTreeMap;

use runvil_types::error::{Error, ErrorCode, FactKey};
use serde_json::Value;

use super::store::{store_fact_fields, store_field_facts};
use super::{ErrorField, ErrorKind};

/// The kind a failure with `code` renders as. A model round's failure is
/// `model_failure`, or `empty_model_reply` for an empty reply, and a round
/// with no model is `model_required`; every Lua-phase failure, an exhausted
/// quota and the call-depth cap included, is `lua`; a store operation's own
/// failure is `store`; a cancelled task's delivery is `cancelled`, as an
/// interruption is; and a failure outside the prompt the author cannot act
/// on (a parse failure, a failed tool freeze, a refused requirement, a
/// determinism violation, an internal fault) is `internal`.
fn kind_of(code: ErrorCode) -> ErrorKind {
    match code {
        ErrorCode::LuaRuntime
        | ErrorCode::LuaCompile
        | ErrorCode::LuaQuota
        | ErrorCode::CallDepthExceeded
        | ErrorCode::Substitution => ErrorKind::Lua,
        ErrorCode::ContextExhausted => ErrorKind::ContextExhausted,
        ErrorCode::EmptyReply => ErrorKind::EmptyModelReply,
        ErrorCode::ContextOverflow
        | ErrorCode::RateLimited
        | ErrorCode::QuotaExhausted
        | ErrorCode::Overloaded
        | ErrorCode::Refused
        | ErrorCode::Timeout
        | ErrorCode::Transport
        | ErrorCode::ServerError
        | ErrorCode::Rejected
        | ErrorCode::MalformedResponse
        | ErrorCode::Unavailable
        | ErrorCode::InvalidParts => ErrorKind::ModelFailure,
        ErrorCode::ModelRequired => ErrorKind::ModelRequired,
        ErrorCode::Interrupted | ErrorCode::TaskCancelled => ErrorKind::Cancelled,
        ErrorCode::ToolLoopExhausted => ErrorKind::ToolLoopExhausted,
        ErrorCode::TasksLive => ErrorKind::TasksLive,
        ErrorCode::TaskNotOwned => ErrorKind::TaskNotOwned,
        ErrorCode::TaskConsumed => ErrorKind::TaskConsumed,
        ErrorCode::OutOfScopeTool => ErrorKind::OutOfScopeTool,
        ErrorCode::UnboundTool => ErrorKind::UnboundTool,
        ErrorCode::ModelOnlyTool => ErrorKind::ModelOnlyTool,
        ErrorCode::ToolInvalidArguments
        | ErrorCode::ToolBackend
        | ErrorCode::ToolTransport
        | ErrorCode::ToolOther => ErrorKind::Tool,
        ErrorCode::NotFound
        | ErrorCode::AlreadyExists
        | ErrorCode::NotADirectory
        | ErrorCode::IsADirectory
        | ErrorCode::DirectoryNotEmpty
        | ErrorCode::NotUtf8
        | ErrorCode::InvalidPath
        | ErrorCode::InvalidRange
        | ErrorCode::Anchor
        | ErrorCode::PermissionDenied
        | ErrorCode::ApprovalRequired
        | ErrorCode::ScopeEnded
        | ErrorCode::ReadOnly
        | ErrorCode::OutsideRoot
        | ErrorCode::RootProtected
        | ErrorCode::NoStore
        | ErrorCode::CrossMount
        | ErrorCode::Io
        | ErrorCode::Unsupported
        | ErrorCode::Conflict
        | ErrorCode::Backend => ErrorKind::Store,
        ErrorCode::FrontmatterInvalid
        | ErrorCode::StructureInvalid
        | ErrorCode::FenceInvalid
        | ErrorCode::ListEmptyItem
        | ErrorCode::UnsupportedVersion
        | ErrorCode::InvalidValue
        | ErrorCode::ToolSchemaInvalid
        | ErrorCode::ToolSetsInvalid
        | ErrorCode::Determinism
        | ErrorCode::RequirementsUnmet
        | ErrorCode::Internal
        | ErrorCode::RecorderFailed
        | ErrorCode::PluginLoadFailed
        | ErrorCode::NoOutput => ErrorKind::Internal,
    }
}

/// The fact `key` of `error` as the string field `name`, or nothing when
/// the fact is absent or not a string.
fn string_field(error: &Error, name: &str, key: FactKey) -> Vec<(String, ErrorField)> {
    error
        .facts
        .get(&key)
        .and_then(Value::as_str)
        .map(|value| vec![(name.to_owned(), ErrorField::String(value.to_owned()))])
        .unwrap_or_default()
}

/// The `Tasks` fact's ids as one string joined by `, `, the `tasks` field
/// of a `tasks_live` table.
fn joined_tasks(error: &Error) -> Vec<(String, ErrorField)> {
    let Some(Value::Array(tasks)) = error.facts.get(&FactKey::Tasks) else {
        return Vec::new();
    };
    let joined = tasks
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    vec![("tasks".to_owned(), ErrorField::String(joined))]
}

/// Returns the kind a universal error's Lua error table names, which its
/// code classifies as.
#[must_use]
pub fn error_kind(error: &Error) -> ErrorKind {
    kind_of(error.code)
}

/// Returns the fields a universal error's Lua error table holds beside
/// `kind`, `code`, and `message`, from its facts: `reason` for context exhaustion,
/// `finish_reason` for an empty reply when the backend gave one, `name` for
/// a refused tool call, `tasks` for leaked tasks, `task` for a task
/// operation's refusal or a cancelled task's delivery, and a store
/// failure's `reason` and fields. Every other code's table has no fields,
/// whatever facts it holds.
#[must_use]
pub fn error_fields(error: &Error) -> Vec<(String, ErrorField)> {
    match error.code {
        ErrorCode::ContextExhausted => string_field(error, "reason", FactKey::Reason),
        ErrorCode::EmptyReply => string_field(error, "finish_reason", FactKey::FinishReason),
        ErrorCode::OutOfScopeTool | ErrorCode::UnboundTool | ErrorCode::ModelOnlyTool => {
            string_field(error, "name", FactKey::Name)
        }
        ErrorCode::TasksLive => joined_tasks(error),
        ErrorCode::TaskNotOwned | ErrorCode::TaskConsumed | ErrorCode::TaskCancelled => {
            string_field(error, "task", FactKey::Task)
        }
        code if kind_of(code) == ErrorKind::Store => store_fact_fields(error),
        _ => Vec::new(),
    }
}

/// Returns the facts an error table's `fields` hold for a failure with
/// `code`, the inverse of [`error_fields`]: each field that code's table
/// gives, under its fact, with `tasks` split back into the ids it joins.
/// A field of another shape, or one the code's table never gives, holds
/// no fact.
pub(super) fn field_facts(
    code: ErrorCode,
    fields: &BTreeMap<String, ErrorField>,
) -> BTreeMap<FactKey, Value> {
    let string = |name: &str, key: FactKey| {
        fields
            .get(name)
            .and_then(ErrorField::as_str)
            .map(|value| (key, Value::from(value)))
            .into_iter()
            .collect()
    };
    match code {
        ErrorCode::ContextExhausted => string("reason", FactKey::Reason),
        ErrorCode::EmptyReply => string("finish_reason", FactKey::FinishReason),
        ErrorCode::OutOfScopeTool | ErrorCode::UnboundTool | ErrorCode::ModelOnlyTool => {
            string("name", FactKey::Name)
        }
        ErrorCode::TasksLive => fields
            .get("tasks")
            .and_then(ErrorField::as_str)
            .map(|joined| {
                let tasks: Vec<&str> = joined.split(", ").filter(|id| !id.is_empty()).collect();
                (FactKey::Tasks, Value::from(tasks))
            })
            .into_iter()
            .collect(),
        ErrorCode::TaskNotOwned | ErrorCode::TaskConsumed | ErrorCode::TaskCancelled => {
            string("task", FactKey::Task)
        }
        code if kind_of(code) == ErrorKind::Store => store_field_facts(fields),
        _ => BTreeMap::new(),
    }
}

#[cfg(test)]
#[path = "error-value-universal-tests.rs"]
mod tests;
