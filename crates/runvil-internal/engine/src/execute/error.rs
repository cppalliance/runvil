//! The stable [`RunErrorKind`] of a failed run, and the kind and message
//! the Engine's own error ends a run with.

use runvil_types::error::FactKey;

use crate::Error;

use super::RunOutcome;

/// The stable category of a failed run's [`RunOutcome::Failed`], for matching
/// in code.
///
/// Each variant names the condition that failed the run. The enum is
/// `#[non_exhaustive]`, so a `match` on it needs a wildcard arm. That lets
/// new kinds be added while existing callers keep compiling.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunErrorKind {
    /// Parsing the prompt failed: invalid frontmatter YAML, a failure the
    /// parser classified as structural, or a Lua block that did not compile.
    /// The run's first step reports it too, before any section runs, for a
    /// prompt that declares no `runvil:` version or a major version this
    /// build does not support.
    Parse,
    /// Building the tools a section offers its model failed: a tool's
    /// model-facing schema did not build, or the section's frozen sets, or
    /// the render answer they are merged from, broke a rule of the merge.
    Binding,
    /// A model completion failed at the transport, backend, or decode layer.
    Completion,
    /// A tool call failed or named an alias outside its scope, or the
    /// tool-call loop reached its iteration cap before a final reply.
    Tool,
    /// A run-scoped store operation failed, or the run lacks a store to
    /// operate on.
    ///
    /// It also covers two live executions in the run that claim the same
    /// store path. The run then stops at once to keep the interleaving of
    /// their store access deterministic, and Lua code cannot catch the
    /// failure.
    Vfs,
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
    /// The environment falls short of what the prompt needs. The run's
    /// first step reports it, before any section runs, for a declared Pack
    /// that the run's roster lacks or lists as unavailable, a model
    /// requirement that the model filling a role fails to meet (a context
    /// minimum or a thinking requirement), a non-empty role map with no
    /// `default` entry, and a reserved Pack id that the run's tool catalog
    /// or Pack roster names. An H1 block that failed the prompt's hard gate
    /// reports it too, and so does a `models.infer` or `models.loop` round
    /// that finds no current model because the caller supplied none.
    RequirementsUnmet,
}

impl RunErrorKind {
    /// Returns the stable tag for this kind: the variant's name, such as
    /// `Parse` or `RequirementsUnmet`.
    ///
    /// [`RunErrorKind::from_tag`] parses the same word back, so the two
    /// round-trip.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            RunErrorKind::Parse => "Parse",
            RunErrorKind::Binding => "Binding",
            RunErrorKind::Completion => "Completion",
            RunErrorKind::Tool => "Tool",
            RunErrorKind::Vfs => "Vfs",
            RunErrorKind::Lua => "Lua",
            RunErrorKind::Quota => "Quota",
            RunErrorKind::ContextExhausted => "ContextExhausted",
            RunErrorKind::Internal => "Internal",
            RunErrorKind::RequirementsUnmet => "RequirementsUnmet",
        }
    }

    /// Parses a tag from [`RunErrorKind::as_str`] back into its kind.
    ///
    /// Two tags that older builds wrote also parse, so a stored run keeps
    /// its meaning: `Version` reads as [`RunErrorKind::Parse`] and
    /// `Determinism` as [`RunErrorKind::Vfs`]. Returns `None` for any
    /// other string.
    #[must_use]
    pub fn from_tag(tag: &str) -> Option<RunErrorKind> {
        match tag {
            "Parse" | "Version" => Some(RunErrorKind::Parse),
            "Binding" => Some(RunErrorKind::Binding),
            "Completion" => Some(RunErrorKind::Completion),
            "Tool" => Some(RunErrorKind::Tool),
            "Vfs" | "Determinism" => Some(RunErrorKind::Vfs),
            "Lua" => Some(RunErrorKind::Lua),
            "Quota" => Some(RunErrorKind::Quota),
            "ContextExhausted" => Some(RunErrorKind::ContextExhausted),
            "Internal" => Some(RunErrorKind::Internal),
            "RequirementsUnmet" => Some(RunErrorKind::RequirementsUnmet),
            _ => None,
        }
    }
}

impl Error {
    /// Returns the stable classification of this failure.
    pub(crate) fn kind(&self) -> RunErrorKind {
        match self {
            Error::Parse(_) | Error::UnsupportedVersion(_) => RunErrorKind::Parse,
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
            Error::RequirementsUnmet { .. } | Error::ModelRequired { .. } => {
                RunErrorKind::RequirementsUnmet
            }
            Error::Completion(_) => RunErrorKind::Completion,
            Error::ToolLoopExhausted
            | Error::OutOfScopeToolCall { .. }
            | Error::UnboundToolCall { .. }
            | Error::ModelOnlyToolCall { .. }
            | Error::Tool { .. } => RunErrorKind::Tool,
            // A cancelled run ends as `RunOutcome::Cancelled`, so an
            // interruption that reaches a failure is an Engine fault.
            Error::Internal { .. } | Error::Interrupted => RunErrorKind::Internal,
            Error::Store { .. } | Error::Determinism(_) => RunErrorKind::Vfs,
            Error::BindSchema { .. } | Error::ToolSets { .. } => RunErrorKind::Binding,
        }
    }

    /// Returns the self-contained text a failed run reports: the error's
    /// own text, then each cause down its `source` chain whose text the
    /// message does not hold yet, after `: `, then where the failure
    /// happened.
    ///
    /// A cause-bearing variant renders its cause in its own text and keeps
    /// no `source`, so the chain adds only what a transparent variant's
    /// error keeps there. A parse failure ends with its prompt name, line,
    /// and column, each when known, as `(prompt {name}, line {line},
    /// column {column})`, and an internal fault with the Rust source file
    /// and line of the broken invariant, as `(at {file}:{line})`.
    pub(crate) fn message(&self) -> String {
        let mut rendered = self.to_string();
        let mut cause = std::error::Error::source(self);
        while let Some(current) = cause {
            let text = current.to_string();
            if !text.is_empty() && !rendered.contains(&text) {
                rendered.push_str(": ");
                rendered.push_str(&text);
            }
            cause = current.source();
        }
        if let Some(location) = self.location() {
            rendered.push_str(" (");
            rendered.push_str(&location);
            rendered.push(')');
        }
        rendered
    }

    /// Where the failure happened, for the end of its message, or `None`
    /// when the failure has no location or the parser found none of its
    /// parts.
    fn location(&self) -> Option<String> {
        match self {
            Error::Parse(error) => {
                let parts: Vec<String> = [
                    error
                        .facts
                        .get(&FactKey::Name)
                        .and_then(serde_json::Value::as_str)
                        .map(|name| format!("prompt {name}")),
                    error
                        .at
                        .as_ref()
                        .map(|at| format!("line {}", at.start_line)),
                    error
                        .at
                        .as_ref()
                        .map(|at| format!("column {}", at.start_column)),
                ]
                .into_iter()
                .flatten()
                .collect();
                (!parts.is_empty()).then(|| parts.join(", "))
            }
            Error::Internal { file, line, .. } => Some(format!("at {file}:{line}")),
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
            | Error::ModelOnlyToolCall { .. }
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

    /// Returns the failed outcome this error ends a run with.
    pub(crate) fn to_outcome(&self) -> RunOutcome {
        RunOutcome::Failed {
            kind: self.kind(),
            message: self.message(),
        }
    }
}

#[cfg(test)]
#[path = "error-tests.rs"]
mod tests;
