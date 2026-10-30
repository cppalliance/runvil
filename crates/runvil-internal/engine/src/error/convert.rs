//! Building the internal error: its constructors, the `From` bridges that
//! map the model-client, parser, and Lua crates' errors onto it variant
//! for variant, and the rebuild of a raised Lua error table into the
//! variant it stands in for.

use std::borrow::Cow;

use runvil_lua::Error as LuaError;
use runvil_parser::Error as ParserError;
use runvil_types::ids::TaskId;
use runvil_vfs::VfsError;

use super::{BoxedSource, Error};

impl Error {
    /// Builds a parse failure with a stable classification and no source span.
    pub(crate) fn parse(kind: crate::parser::ParseErrorKind, message: impl Into<String>) -> Error {
        Error::Parse(crate::parser::detail::parse_error(kind, message))
    }

    /// Stamps the prompt's frontmatter name onto a parse failure raised by
    /// `run` itself: the prompt is already parsed at that point, so the
    /// name is known and the location can name it. Any other variant passes
    /// through unchanged.
    pub(crate) fn with_prompt_name(self, name: &str) -> Error {
        match self {
            Error::Parse(error) => {
                Error::Parse(crate::parser::detail::with_prompt_name(error, name))
            }
            other => other,
        }
    }

    /// Builds an internal-invariant failure, capturing the Rust source
    /// position of the call site so [`crate::RunError::location`] can point
    /// at the broken invariant. The message is owned when it names a value.
    #[track_caller]
    pub(crate) fn internal(message: impl Into<Cow<'static, str>>) -> Error {
        let location = std::panic::Location::caller();
        Error::Internal {
            message: message.into(),
            file: location.file(),
            line: location.line(),
            cause: None,
        }
    }

    /// Builds an internal-invariant failure at a construction site the
    /// caller already captured, such as the one a Lua-crate internal
    /// failure brings.
    pub(crate) fn internal_at(
        message: impl Into<Cow<'static, str>>,
        location: &'static std::panic::Location<'static>,
    ) -> Error {
        Error::Internal {
            message: message.into(),
            file: location.file(),
            line: location.line(),
            cause: None,
        }
    }

    /// Builds an internal-invariant failure that keeps the failure which
    /// broke the invariant as its cause, capturing the call site as
    /// [`Error::internal`] does.
    #[track_caller]
    pub(crate) fn internal_with_cause(
        message: impl Into<Cow<'static, str>>,
        cause: impl Into<BoxedSource>,
    ) -> Error {
        let location = std::panic::Location::caller();
        Error::Internal {
            message: message.into(),
            file: location.file(),
            line: location.line(),
            cause: Some(cause.into()),
        }
    }

    /// Wraps a store failure that has no operation to render - a handle
    /// acquisition, the declared-store probe - as [`Error::Store`]: a run
    /// that declares no store is told so, and any other failure follows the
    /// fixed probe text.
    pub(crate) fn store(cause: VfsError) -> Error {
        Error::Store {
            message: crate::lua::store_probe_message(&cause),
            cause,
        }
    }

    /// Wraps one store operation's failure as [`Error::Store`], rendering
    /// the model-facing message with the operation for its wording.
    pub(crate) fn store_op(op: &runvil_lua::VfsOp, cause: VfsError) -> Error {
        Error::Store {
            message: crate::lua::store_error_message(op, &cause),
            cause,
        }
    }

    /// Builds an [`Error::Lua`] that keeps only `message`, with no
    /// originating `mlua` error.
    pub(crate) fn lua_message(message: impl Into<String>) -> Error {
        Error::Lua {
            message: message.into(),
            cause: None,
        }
    }

    /// Wraps an `mlua` failure as [`Error::Lua`], keeping it as the cause
    /// rather than flattening it to a string.
    #[cfg(test)]
    pub(super) fn lua(cause: mlua::Error) -> Error {
        Error::Lua {
            message: cause.to_string(),
            cause: Some(Box::new(cause)),
        }
    }
}

impl From<crate::subst::SubstitutionError> for Error {
    fn from(error: crate::subst::SubstitutionError) -> Error {
        Error::Substitution(Box::new(error))
    }
}

impl From<crate::model::CompletionError> for Error {
    fn from(error: crate::model::CompletionError) -> Error {
        Error::Completion(error)
    }
}

/// Maps a parse failure onto this internal type: a frontmatter or
/// structured failure stays a parse failure whole, and a Lua compile or
/// internal failure maps onto its own variant. The parser crate's error
/// type is not `#[non_exhaustive]` (the two crates version together), so
/// this match is total.
impl From<crate::parser::ParseError> for Error {
    fn from(error: crate::parser::ParseError) -> Self {
        match crate::parser::detail::parse_error_into_inner(error) {
            inner
            @ (ParserError::ParseFrontmatter { .. } | ParserError::ParseStructured { .. }) => {
                Error::Parse(crate::parser::detail::parse_error_from_inner(inner))
            }
            ParserError::Lua(lua) => Error::from(lua),
            ParserError::Internal(message) => Error::internal(message),
        }
    }
}

/// Maps the Lua crate's internal type onto this one variant for variant,
/// and rebuilds a raised error table through [`Error::from_raised`]. The
/// Lua crate's error type is not `#[non_exhaustive]` (the two crates
/// version together), so this match is total.
impl From<LuaError> for Error {
    fn from(error: LuaError) -> Error {
        match error {
            LuaError::Lua(message) => Error::lua_message(message),
            LuaError::LuaRuntime { message, cause } => Error::Lua {
                message,
                cause: Some(cause),
            },
            LuaError::LuaCompile {
                location,
                source_line,
                lua_source,
                message,
                cause,
            } => Error::LuaCompile {
                location,
                source_line,
                lua_source,
                message,
                cause,
            },
            LuaError::LuaQuota { resource } => Error::LuaQuota { resource },
            LuaError::ContextExhausted { reason } => Error::ContextExhausted { reason },
            LuaError::Interrupted => Error::Interrupted,
            LuaError::Tool { message, cause } => Error::Tool { message, cause },
            LuaError::Store { message, cause } => Error::Store { message, cause },
            LuaError::Internal { message, location } => Error::internal_at(message, location),
            LuaError::Raised(raised) => Error::from_raised(raised),
        }
    }
}

/// The `task` field of a raised task-error table, when it parses.
fn raised_task(raised: &runvil_lua::Raised) -> Option<TaskId> {
    raised
        .fields
        .get("task")
        .and_then(runvil_lua::ErrorField::as_str)
        .and_then(|task| task.parse().ok())
}

/// The field `name` of a raised error table, when it is a string.
fn raised_field<'a>(raised: &'a runvil_lua::Raised, name: &str) -> Option<&'a str> {
    raised
        .fields
        .get(name)
        .and_then(runvil_lua::ErrorField::as_str)
}

/// Rebuilds the structured [`runvil_vfs::VfsError`] a raised store
/// error table stands in for, from its `reason`, its fields, and its
/// message. The table was built from the same variant by
/// [`store_error_value_fields`](runvil_lua::store_error_value_fields), so the
/// reconstruction is exact where the fields round-trip; a malformed table
/// degrades to the closest variant rather than panicking.
fn raised_store_error(raised: &runvil_lua::Raised) -> VfsError {
    use runvil_vfs::PathReason;
    let path = raised_field(raised, "path").unwrap_or_default().to_owned();
    match raised_field(raised, "reason") {
        Some("not_found") => VfsError::NotFound { path },
        Some("anchor") => VfsError::Anchor {
            path,
            anchor: raised_field(raised, "anchor")
                .unwrap_or_default()
                .to_owned(),
            count: raised
                .fields
                .get("count")
                .and_then(runvil_lua::ErrorField::as_integer)
                .and_then(|count| usize::try_from(count).ok())
                .unwrap_or(0),
        },
        Some("invalid_path") => VfsError::InvalidPath {
            path,
            reason: raised_field(raised, "rule")
                .and_then(PathReason::from_tag)
                .unwrap_or(PathReason::Empty),
        },
        Some("invalid_range") => VfsError::InvalidRange {
            path,
            reason: "the bounds are invalid",
        },
        Some("not_utf8") => VfsError::NotUtf8 {
            path,
            valid_up_to: raised
                .fields
                .get("valid_up_to")
                .and_then(runvil_lua::ErrorField::as_integer)
                .and_then(|offset| usize::try_from(offset).ok())
                .unwrap_or(0),
        },
        Some("is_a_directory") => VfsError::IsADirectory { path },
        Some("not_a_directory") => VfsError::NotADirectory { path },
        Some("directory_not_empty") => VfsError::DirectoryNotEmpty { path },
        Some("already_exists") => VfsError::AlreadyExists { path },
        Some("scope_ended") => VfsError::ScopeEnded { path },
        Some("read_only") => VfsError::ReadOnly { path },
        Some("root_protected") => VfsError::RootProtected { path },
        Some("no_store") => VfsError::NoStore,
        // The variants below carry only `path` beside the message, so
        // their text fields degrade to empty, and an I/O failure's kind
        // to `Other` with no OS error code.
        Some("permission_denied") => VfsError::PermissionDenied {
            path,
            reason: String::new(),
        },
        Some("approval_required") => VfsError::ApprovalRequired {
            path,
            reason: String::new(),
        },
        Some("outside_root") => VfsError::OutsideRoot {
            path,
            detail: String::new(),
        },
        Some("cross_mount") => VfsError::CrossMount {
            path,
            detail: String::new(),
        },
        Some("io") => VfsError::Io {
            path,
            kind: std::io::ErrorKind::Other,
            raw_os_error: None,
            message: String::new(),
        },
        Some("unsupported") => VfsError::Unsupported {
            path,
            detail: String::new(),
        },
        Some("conflict") => VfsError::Conflict {
            path,
            detail: raised.message.clone(),
        },
        _ => VfsError::Backend {
            message: raised.message.clone(),
        },
    }
}

impl Error {
    /// Maps a structured error table that surfaced as a block's failure
    /// onto the variant its kind names, so a Lua-side raise classifies as
    /// the Rust-raised error it stands in for. A `task_not_owned` or
    /// `task_consumed` table rebuilds its variant from its `task` field, and
    /// every `cancelled` table maps to [`Error::Interrupted`]. A kind whose
    /// variant needs structure the table does not hold (the tool-scope
    /// errors, `tasks_live`, `model_failure`, `model_required`, `internal`),
    /// or a `context_exhausted` or task table missing its field, keeps its
    /// message as a Lua failure.
    fn from_raised(raised: runvil_lua::Raised) -> Error {
        match raised.kind {
            runvil_lua::ErrorKind::ToolLoopExhausted => Error::ToolLoopExhausted,
            runvil_lua::ErrorKind::ContextExhausted => match raised.exhaustion_reason() {
                Some(reason) => Error::ContextExhausted {
                    reason: reason.to_owned(),
                },
                None => Error::lua_message(raised.message),
            },
            runvil_lua::ErrorKind::EmptyModelReply => {
                let error = crate::model::CompletionError::new(
                    crate::model::CompletionErrorKind::EmptyReply,
                    raised.message,
                );
                Error::Completion(
                    match raised
                        .fields
                        .get("finish_reason")
                        .and_then(runvil_lua::ErrorField::as_str)
                    {
                        Some(reason) => error.with_finish_reason(reason),
                        None => error,
                    },
                )
            }
            runvil_lua::ErrorKind::Cancelled => Error::Interrupted,
            runvil_lua::ErrorKind::Tool => Error::Tool {
                message: raised.message.clone(),
                cause: Box::new(raised),
            },
            runvil_lua::ErrorKind::TaskNotOwned => match raised_task(&raised) {
                Some(task) => Error::TaskNotOwned { task },
                None => Error::lua_message(raised.message),
            },
            runvil_lua::ErrorKind::TaskConsumed => match raised_task(&raised) {
                Some(task) => Error::TaskConsumed { task },
                None => Error::lua_message(raised.message),
            },
            runvil_lua::ErrorKind::Store => Error::Store {
                message: raised.message.clone(),
                cause: raised_store_error(&raised),
            },
            runvil_lua::ErrorKind::OutOfScopeTool
            | runvil_lua::ErrorKind::UnboundTool
            | runvil_lua::ErrorKind::TasksLive
            | runvil_lua::ErrorKind::Lua
            | runvil_lua::ErrorKind::ModelFailure
            | runvil_lua::ErrorKind::ModelRequired
            | runvil_lua::ErrorKind::Internal => Error::lua_message(raised.message),
        }
    }
}
