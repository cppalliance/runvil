//! The public run-error surface: [`RunError`], its stable [`RunErrorKind`],
//! and the [`SourceLocation`] that says where a failure happened.

use std::fmt;
use std::num::NonZeroU32;
use std::ops::Range;

use crate::Error;

/// The stable category of a [`RunError`], for matching in code.
///
/// Each variant names the condition that failed the run. The enum is
/// `#[non_exhaustive]`, so a `match` on it needs a wildcard arm. That lets
/// new kinds be added while existing callers keep compiling.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunErrorKind {
    /// Parsing the prompt failed: invalid frontmatter YAML, a failure the
    /// parser classified as structural, or a Lua block that did not compile.
    Parse,
    /// The prompt declared a `runvil:` major version outside the set
    /// this build supports.
    Version,
    /// Binding a tool or model capability failed, or the capability was
    /// absent.
    Binding,
    /// A model completion failed at the transport, backend, or decode layer.
    Completion,
    /// A tool call failed or named an alias outside its scope, or the
    /// tool-call loop reached its iteration cap before a final reply.
    Tool,
    /// A run-scoped store operation failed, or the run lacks a store to
    /// operate on.
    Vfs,
    /// Two live executions in the run claimed the same store path.
    ///
    /// The run stops at once to keep the interleaving of their store access
    /// deterministic. Lua code cannot catch this failure.
    Determinism,
    /// A section's Lua phase failed to run or return a usable value, or the
    /// author's program broke where the Engine cannot fix it: a Pack prelude
    /// that did not load, a chain that ended with tasks still live, a
    /// task reached for by a chain that does not own it, a task result
    /// waited on twice, a wait delivering a cancelled task that surfaced
    /// uncaught, and a failed `{{ }}` substitution.
    Lua,
    /// A Lua quota ran out: the `log()` event budget or the `log()` byte
    /// budget. Also the `call`, `tasks.spawn`, or `tasks.fanout` arm that
    /// would have nested past the call-depth cap.
    Quota,
    /// `compactors.fail` raised context exhaustion: a request overflowed the
    /// model's context window and the selected compactor left it over the
    /// limit, or a section's Lua raised it with its own reason.
    ContextExhausted,
    /// An internal invariant failed, or the caller broke the `step` and
    /// `resume` contract: a `step` after `Done`, an answer of the wrong
    /// kind, an answer for an id the run never issued, or a second answer
    /// for one effect.
    Internal,
    /// The environment falls short of what the prompt needs: a missing or
    /// unavailable required Pack, a service the caller left out, a model
    /// requirement a bound model fails to meet (a context minimum or a hard
    /// keyword), a reserved Pack id that the run's tool catalog or Pack
    /// roster names, or an H1 block that failed the prompt's hard gate.
    RequirementsUnmet,
}

/// Where a run failed: a position in the prompt source or in the Rust
/// source.
///
/// The error's [`RunErrorKind`] says which of the two it is, and the
/// extension of `path` says so too. Use the kind to branch in code, the
/// error message to show a reader, and the location to navigate to the
/// fault. Only the Engine builds one.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SourceLocation {
    /// The prompt name or Rust source file where the failure occurred.
    ///
    /// For a prompt failure this is the prompt's frontmatter name when
    /// parsing got that far. Otherwise it is the placeholder `<prompt>`,
    /// which the caller should replace with its own label for the source. A
    /// frontmatter YAML failure always gets the placeholder, because it
    /// happens before the name is read. For an internal fault this is the
    /// Rust source file, as given by `file!()`.
    pub path: String,
    /// The 1-based line, when known.
    pub line: Option<u32>,
    /// The 1-based column, when known.
    pub column: Option<u32>,
    /// The byte span of the offending region, when known.
    ///
    /// Only parse failures can have a span, and a frontmatter YAML failure
    /// always has `None`. The offsets index the document body, which
    /// excludes the frontmatter and any leading BOM and has CRLF line endings
    /// normalized to LF. Use [`line`](SourceLocation::line) and
    /// [`column`](SourceLocation::column) to locate the failure in the
    /// original file.
    pub span: Option<Range<usize>>,
}

/// The error a failed prompt run reports.
///
/// A run that fails ends with [`Step::Done`](super::Step::Done), whose
/// result is [`RunResult::Failure`](super::RunResult::Failure) holding this
/// error. Use [`kind`](RunError::kind) to classify the failure in code. The
/// message is self-contained: when the failure keeps an originating error,
/// the message already holds that error's text, and
/// [`originating_error`](RunError::originating_error) returns the error
/// itself. A failure whose message does not hold its underlying error, such
/// as a model round's, keeps it behind [`std::error::Error::source`] instead.
#[derive(Debug)]
pub struct RunError {
    inner: Error,
}

impl RunError {
    /// Returns the stable classification of this failure.
    #[must_use]
    pub fn kind(&self) -> RunErrorKind {
        match &self.inner {
            Error::Parse(_) => RunErrorKind::Parse,
            Error::LuaQuota { .. } | Error::CallDepthExceeded { .. } => RunErrorKind::Quota,
            Error::ContextExhausted { .. } => RunErrorKind::ContextExhausted,
            // A leaked task, a task reached for by a chain that does not
            // own it, a result waited on twice, a wait's delivery of a
            // cancelled task surfacing uncaught, and a failed `{{ }}`
            // substitution are the author's program failing, as any Lua
            // fault is.
            Error::LuaCompile { .. }
            | Error::Lua { .. }
            | Error::TasksLive { .. }
            | Error::TaskNotOwned { .. }
            | Error::TaskConsumed { .. }
            | Error::TaskCancelled { .. }
            | Error::Substitution(_) => RunErrorKind::Lua,
            Error::UnsupportedVersion(_) => RunErrorKind::Version,
            Error::RequirementsUnmet { .. } => RunErrorKind::RequirementsUnmet,
            Error::Completion(_) => RunErrorKind::Completion,
            Error::ToolLoopExhausted
            | Error::OutOfScopeToolCall { .. }
            | Error::UnboundToolCall { .. }
            | Error::Tool { .. } => RunErrorKind::Tool,
            // A cancelled run ends as `RunResult::Cancelled`, so an
            // interruption that reaches a failure is an Engine fault.
            Error::Internal { .. } | Error::Interrupted => RunErrorKind::Internal,
            Error::Store { .. } => RunErrorKind::Vfs,
            Error::Determinism(_) => RunErrorKind::Determinism,
            Error::BindSchema { .. } | Error::ToolSets { .. } | Error::ModelRequired { .. } => {
                RunErrorKind::Binding
            }
        }
    }

    /// The Engine's own error behind the boundary one, for a test that
    /// matches on its variant.
    #[cfg(test)]
    pub(crate) fn inner(&self) -> &Error {
        &self.inner
    }

    /// Returns the error this failure originated from, when it keeps one: a
    /// Lua runtime failure keeps the `mlua` error, a Lua compile failure the
    /// compiler diagnostic, a schema binding failure the validation error, a
    /// tool failure the tool's own error, a store failure the store's
    /// structured error, an unmet requirement the failure it stands in for,
    /// an internal fault the failure that broke the invariant, and a parse
    /// failure the parser's cause. A model round's failure and a failed
    /// `{{ }}` substitution answer `None`: whatever cause each holds belongs
    /// to the error it wraps.
    ///
    /// The message already holds the cause's text, so
    /// [`std::error::Error::source`] does not return it again, and a printer
    /// that walks the source chain shows the text once.
    #[must_use]
    pub fn originating_error(&self) -> Option<&(dyn std::error::Error + Send + Sync + 'static)> {
        self.inner.originating_error()
    }

    /// Returns `true` when retrying the run may succeed (transient transport or
    /// backend failures).
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match &self.inner {
            Error::Completion(error) => error.is_retryable(),
            Error::Parse(_)
            | Error::Interrupted
            | Error::Lua { .. }
            | Error::LuaCompile { .. }
            | Error::BindSchema { .. }
            | Error::ToolSets { .. }
            | Error::Substitution(_)
            | Error::ToolLoopExhausted
            | Error::TasksLive { .. }
            | Error::TaskNotOwned { .. }
            | Error::TaskConsumed { .. }
            | Error::TaskCancelled { .. }
            | Error::OutOfScopeToolCall { .. }
            | Error::UnboundToolCall { .. }
            | Error::ModelRequired { .. }
            | Error::UnsupportedVersion(_)
            | Error::RequirementsUnmet { .. }
            | Error::Tool { .. }
            | Error::Internal { .. }
            | Error::LuaQuota { .. }
            | Error::CallDepthExceeded { .. }
            | Error::ContextExhausted { .. }
            | Error::Store { .. }
            | Error::Determinism(_) => false,
        }
    }

    /// Returns where the failure occurred, or `None` when it has no location.
    ///
    /// A failure of kind `Parse` returns its position in the prompt source.
    /// The path is the prompt's frontmatter name when parsing got that far,
    /// and the placeholder `<prompt>` otherwise. The line and column come
    /// from the YAML error for a frontmatter failure, or from the span for
    /// any other parse failure. A failure of kind `Internal` returns the Rust
    /// source file and line of the broken invariant. Failures of every other
    /// kind return `None`.
    #[must_use]
    pub fn location(&self) -> Option<SourceLocation> {
        match &self.inner {
            Error::Parse(error) => Some(SourceLocation {
                path: error.name().unwrap_or("<prompt>").to_owned(),
                line: error.line().map(NonZeroU32::get),
                column: error.column().map(NonZeroU32::get),
                span: error.span(),
            }),
            Error::Internal { file, line, .. } => Some(SourceLocation {
                path: (*file).to_owned(),
                line: Some(*line),
                column: None,
                span: None,
            }),
            Error::Completion(_)
            | Error::Interrupted
            | Error::Lua { .. }
            | Error::LuaCompile { .. }
            | Error::BindSchema { .. }
            | Error::ToolSets { .. }
            | Error::Substitution(_)
            | Error::ToolLoopExhausted
            | Error::TasksLive { .. }
            | Error::TaskNotOwned { .. }
            | Error::TaskConsumed { .. }
            | Error::TaskCancelled { .. }
            | Error::OutOfScopeToolCall { .. }
            | Error::UnboundToolCall { .. }
            | Error::ModelRequired { .. }
            | Error::UnsupportedVersion(_)
            | Error::RequirementsUnmet { .. }
            | Error::Tool { .. }
            | Error::LuaQuota { .. }
            | Error::CallDepthExceeded { .. }
            | Error::ContextExhausted { .. }
            | Error::Store { .. }
            | Error::Determinism(_) => None,
        }
    }
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.inner)
    }
}

impl std::error::Error for RunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        std::error::Error::source(&self.inner)
    }
}

impl From<Error> for RunError {
    fn from(inner: Error) -> Self {
        RunError { inner }
    }
}
