//! The internal error as a Lua error value: the kind an author branches
//! on and the kind's fields.

use super::{Error, join_task_ids};
use crate::model::CompletionErrorKind;

/// The internal type's rendering into the Lua error table: the kind an author
/// branches on and the kind's fields. A failed model round renders as
/// `model_failure`, or `empty_model_reply` for an empty reply, and a round
/// with no model as `model_required`; other failures outside the prompt
/// (a failed tool freeze, configuration, input) render as `internal`;
/// every Lua-phase failure renders as `lua`; a store operation's own
/// failure renders as `store` with its `reason` and the structured
/// variant's fields.
impl runvil_lua::ErrorValue for Error {
    fn kind(&self) -> runvil_lua::ErrorKind {
        use runvil_lua::ErrorKind;
        match self {
            Error::Lua { .. }
            | Error::LuaCompile { .. }
            | Error::LuaQuota { .. }
            | Error::CallDepthExceeded { .. }
            | Error::Substitution(_) => ErrorKind::Lua,
            Error::ContextExhausted { .. } => ErrorKind::ContextExhausted,
            Error::Completion(error) if error.kind() == CompletionErrorKind::EmptyReply => {
                ErrorKind::EmptyModelReply
            }
            Error::Completion(_) => ErrorKind::ModelFailure,
            Error::ModelRequired { .. } => ErrorKind::ModelRequired,
            Error::Interrupted | Error::TaskCancelled { .. } => ErrorKind::Cancelled,
            Error::ToolLoopExhausted => ErrorKind::ToolLoopExhausted,
            Error::TasksLive { .. } => ErrorKind::TasksLive,
            Error::TaskNotOwned { .. } => ErrorKind::TaskNotOwned,
            Error::TaskConsumed { .. } => ErrorKind::TaskConsumed,
            Error::OutOfScopeToolCall { .. } => ErrorKind::OutOfScopeTool,
            Error::UnboundToolCall { .. } => ErrorKind::UnboundTool,
            Error::ModelOnlyToolCall { .. } => ErrorKind::ModelOnlyTool,
            Error::Tool { .. } => ErrorKind::Tool,
            Error::Store { .. } => ErrorKind::Store,
            Error::Parse(_)
            | Error::BindSchema { .. }
            | Error::ToolSets { .. }
            | Error::UnsupportedVersion(_)
            | Error::RequirementsUnmet { .. }
            | Error::Internal { .. }
            | Error::Determinism(_) => ErrorKind::Internal,
        }
    }

    fn fields(&self) -> Vec<(String, runvil_lua::ErrorField)> {
        use runvil_lua::ErrorField;
        match self {
            Error::ContextExhausted { reason } => {
                vec![("reason".to_owned(), ErrorField::String(reason.clone()))]
            }
            Error::Completion(error) if error.kind() == CompletionErrorKind::EmptyReply => error
                .finish_reason()
                .map(|reason| {
                    vec![(
                        "finish_reason".to_owned(),
                        ErrorField::String(reason.to_owned()),
                    )]
                })
                .unwrap_or_default(),
            Error::OutOfScopeToolCall { name, .. }
            | Error::UnboundToolCall { name, .. }
            | Error::ModelOnlyToolCall { name, .. } => {
                vec![("name".to_owned(), ErrorField::String(name.clone()))]
            }
            Error::TasksLive { tasks } => {
                vec![("tasks".to_owned(), ErrorField::String(join_task_ids(tasks)))]
            }
            Error::TaskNotOwned { task }
            | Error::TaskConsumed { task }
            | Error::TaskCancelled { task } => {
                vec![("task".to_owned(), ErrorField::String(task.to_string()))]
            }
            Error::Store { cause, .. } => runvil_lua::store_error_value_fields(cause),
            Error::Parse(_)
            | Error::Completion(_)
            | Error::Interrupted
            | Error::Lua { .. }
            | Error::LuaCompile { .. }
            | Error::BindSchema { .. }
            | Error::ToolSets { .. }
            | Error::Substitution(_)
            | Error::ToolLoopExhausted
            | Error::ModelRequired { .. }
            | Error::UnsupportedVersion(_)
            | Error::RequirementsUnmet { .. }
            | Error::Tool { .. }
            | Error::Internal { .. }
            | Error::LuaQuota { .. }
            | Error::CallDepthExceeded { .. }
            | Error::Determinism(_) => Vec::new(),
        }
    }
}
