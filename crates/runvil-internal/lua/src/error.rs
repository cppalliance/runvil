//! The crate's internal error type.
//!
//! [`Error`] mirrors `runvil-engine`'s own internal error type: it
//! is never part of the documented API. The executor's public boundary
//! (`runvil_engine::RunError`) wraps and classifies `runvil-engine`'s
//! internal error type, which maps this one back variant-for-variant through
//! `From<runvil_lua::Error>`. The internal type is public only so
//! `runvil-engine` can perform that mapping verbatim; the facade does
//! not re-export it, and it is not marked `#[non_exhaustive]`, so the
//! mapping stays total.
//!
//! A variant that keeps an originating error renders that error's text in
//! its own `Display` and holds the error as a plain `cause` field behind
//! [`Error::cause`], never as a `#[source]`, so a printer that walks the
//! source chain shows the cause's text once.

use std::borrow::Cow;

use runvil_model_client::model::ModelSetError;

/// A type-erased owned error cause used by the internal error type.
type BoxedSource = Box<dyn std::error::Error + Send + Sync>;

/// A cloneable, shareable error cause.
///
/// Some compiled-program statics re-produce a typed [`Error`] on every
/// lookup, so a non-`Clone` dependency error cannot be moved into a fresh
/// [`Error`] each time. Wrapping it in a reference-counted [`SharedSource`]
/// keeps the typed cause and clones it cheaply per lookup instead of
/// flattening it to a string. Every embedded chunk's compiled-program static
/// is a caller, through `detail::shared_source_new`.
#[derive(Debug, Clone)]
pub(crate) struct SharedSource(pub(crate) std::sync::Arc<dyn std::error::Error + Send + Sync>);

impl std::fmt::Display for SharedSource {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, formatter)
    }
}

impl std::error::Error for SharedSource {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.source()
    }
}

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

/// The crate's internal error type: every failure a section VM's setup, Lua
/// code, or Engine calls can end in.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A condition the prompt author caused and can fix in Lua, with no
    /// originating `mlua` error to keep (for example a fanout collection
    /// member that is a function, or a refused message-list edit).
    ///
    /// Failures that *do* have an `mlua` cause use [`Error::LuaRuntime`].
    /// The message is the specific failure as a noun phrase; the public
    /// wrapper classifies this as a Lua failure, so no redundant
    /// `lua error:` type label is prepended.
    #[error("{0}")]
    Lua(String),

    /// A section's Lua phase failed at runtime or while bridging Engine
    /// values: the mapped prompt-location message, beside the originating
    /// `mlua` error kept as the cause.
    ///
    /// This is the cause-bearing counterpart to [`Error::Lua`]: it is built
    /// from a concrete `mlua::Error` (see `Error::lua` and
    /// `LuaProgram::map_runtime_error_among`), so the typed failure
    /// survives instead of being flattened to a string.
    #[error("{message}")]
    LuaRuntime {
        /// The mapped, location-tagged diagnostic (no redundant type label).
        message: String,
        /// The originating Lua error, whose text `message` already holds.
        cause: BoxedSource,
    },

    /// Lua source was not syntactically valid at its prompt location.
    ///
    /// Keeps the originating `mlua` compile error as the cause beside the
    /// location metadata, so the typed compiler diagnostic survives instead
    /// of being flattened into `message` alone.
    #[error("lua compilation error at {location} (line {source_line}): {message}")]
    LuaCompile {
        /// The prompt region supplied by the parser, such as a section prologue.
        location: String,
        /// 1-based line number in the prompt source where this Lua region starts.
        source_line: u32,
        /// The retained source that failed to compile.
        lua_source: String,
        /// The Lua 5.5 compiler diagnostic.
        message: String,
        /// The originating `mlua` compile error, whose text `message` holds.
        cause: BoxedSource,
    },

    /// An Engine quota on Lua (log events or log bytes) was exhausted. A
    /// stable typed error rather than a bare `Lua(String)` so the caller can
    /// distinguish quota exhaustion from an authoring error.
    #[error("lua {resource} quota exceeded")]
    LuaQuota {
        /// The exhausted resource.
        resource: QuotaResource,
    },

    /// `compactors.fail` exhausted the model's context: a request
    /// overflowed the context window (the pre-dispatch precheck or a
    /// provider rejection) and the selected compactor did not compact, or
    /// a caller raised it with its own reason. A stable typed error rather
    /// than a bare [`Error::Lua`] so the caller and `pcall` sites can
    /// distinguish context exhaustion from an authoring error.
    #[error("context exhausted: {}", crate::compactors::exhaustion_detail(reason))]
    ContextExhausted {
        /// `precheck` or `provider` after an overflow, else the caller's reason.
        reason: String,
    },

    /// The run was cancelled, or the caller dropped a call the run was
    /// waiting on.
    #[error("interrupted: the run was cancelled or this call was stopped")]
    Interrupted,

    /// A dispatched tool's own failure: the tool error's message, beside the
    /// tool's typed error kept as the cause rather than flattened.
    #[error("{message}")]
    Tool {
        /// The tool failure's rendered message.
        message: String,
        /// The tool's typed error, whose text `message` holds.
        cause: BoxedSource,
    },

    /// A store operation's own failure: the model-facing message rendered
    /// for the operation, beside the structured
    /// [`VfsError`](runvil_vfs::VfsError) it renders. The direct store
    /// closures raise this, so a failure caught by `pcall` classifies as
    /// kind `store` with the `reason` tag and the variant's fields.
    #[error("{message}")]
    Store {
        /// The model-facing message, from `store_error_message`.
        message: String,
        /// The structured failure the message renders.
        cause: runvil_vfs::VfsError,
    },

    /// Engine misuse that no prompt author can cause: a broken internal
    /// invariant, a lifecycle step taken out of order, or a poisoned lock.
    /// Surfaced as a concrete error rather than silently skipping work, so an
    /// impossible state cannot masquerade as a successful fall-through. The
    /// message is owned when it names a value, and `location` is the
    /// construction site.
    #[error("internal invariant violated: {message}")]
    Internal {
        /// The violated invariant, as a noun phrase.
        message: Cow<'static, str>,
        /// The Rust source position of the construction site.
        location: &'static std::panic::Location<'static>,
    },

    /// A structured error table raised in Lua surfaced as a block
    /// coroutine's failure with no retained typed error to substitute: the
    /// table's kind, message, and fields, kept rather than flattened to the
    /// message string. The executor maps it back onto its own error type by
    /// kind, so a shim raise classifies as the Rust-raised error it stands
    /// in for.
    #[error("{0}")]
    Raised(crate::error_value::Raised),
}

/// The typed value an Engine quota refusal raises through Lua.
///
/// The runtime-error boundary recognizes a refusal by downcasting to this
/// type and maps it to [`Error::LuaQuota`], so an author who raises the
/// same text fails as an ordinary runtime error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct QuotaExceeded(pub(crate) QuotaResource);

impl std::fmt::Display for QuotaExceeded {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self.0 {
            QuotaResource::LogEvent => "lua log event budget exceeded",
            QuotaResource::LogByte => "lua log cumulative byte budget exceeded",
        })
    }
}

impl std::error::Error for QuotaExceeded {}

impl Error {
    /// Wraps an `mlua` failure as [`Error::LuaRuntime`], keeping it as the
    /// cause rather than flattening it to a string.
    pub(crate) fn lua(source: mlua::Error) -> Error {
        Error::LuaRuntime {
            message: source.to_string(),
            cause: Box::new(source),
        }
    }

    /// Re-produces a typed [`Error`] from a [`SharedSource`] a
    /// compiled-program static captured once, cloning the `Arc` rather than
    /// flattening the cause to a string.
    pub(crate) fn shared(source: &SharedSource) -> Error {
        Error::LuaRuntime {
            message: source.to_string(),
            cause: Box::new(source.clone()),
        }
    }

    /// Wraps a tool failure as [`Error::Tool`], keeping the tool's own
    /// error as the cause rather than discarding it.
    pub(crate) fn tool(source: runvil_types::tools::ToolError) -> Error {
        Error::Tool {
            message: source.to_string(),
            cause: Box::new(source),
        }
    }

    /// Wraps a store operation's failure as [`Error::Store`], rendering
    /// the model-facing message with the operation for its wording.
    pub(crate) fn store(op: &crate::protocol::VfsOp, source: runvil_vfs::VfsError) -> Error {
        Error::Store {
            message: crate::engine_globals::store_error_message(op, &source),
            cause: source,
        }
    }

    /// Builds an [`Error::Internal`], capturing the call site as its
    /// construction site. The message is owned when it names a value.
    #[track_caller]
    pub(crate) fn internal(message: impl Into<Cow<'static, str>>) -> Error {
        Error::Internal {
            message: message.into(),
            location: std::panic::Location::caller(),
        }
    }

    /// Returns the originating error a cause-bearing variant keeps, whose
    /// text its `Display` already holds; `None` for every other variant.
    #[must_use]
    pub fn cause(&self) -> Option<&(dyn std::error::Error + Send + Sync + 'static)> {
        match self {
            Error::LuaRuntime { cause, .. }
            | Error::LuaCompile { cause, .. }
            | Error::Tool { cause, .. } => Some(cause.as_ref()),
            Error::Store { cause, .. } => Some(cause),
            Error::Lua(_)
            | Error::LuaQuota { .. }
            | Error::ContextExhausted { .. }
            | Error::Interrupted
            | Error::Internal { .. }
            | Error::Raised(_) => None,
        }
    }
}

/// An author error raised across the Lua boundary: an external
/// [`Error::Lua`], which `pcall` reports as kind `lua`.
pub(crate) fn author_refusal(message: String) -> mlua::Error {
    mlua::Error::external(Error::Lua(message))
}

/// Maps a poisoned model-set lock onto [`Error::Internal`], the mapping
/// the Engine receives through its `From<runvil_lua::Error>`: the lock is
/// the run's own mutex failing, which no prompt author can cause, not a
/// model failure.
impl From<ModelSetError> for Error {
    fn from(error: ModelSetError) -> Error {
        Error::internal(error.to_string())
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
