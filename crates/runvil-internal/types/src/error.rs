//! The universal error: one [`Error`] record that can describe any Engine
//! failure, by a stable [`ErrorCode`], text for people and for the model,
//! an optional [`SourceLoc`], and values named by [`ErrorFact`].

use std::collections::BTreeMap;
use std::fmt;
use std::num::NonZeroU32;

#[cfg(test)]
#[path = "error-tests.rs"]
mod tests;

/// One failure: what it is, the text for people and for the model, where it
/// happened, and the values it names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    /// Which failure this is.
    pub code: ErrorCode,
    /// The full diagnostic for logs and people, causes included.
    pub message: String,
    /// The text the model reads: what failed, and what to do instead or that
    /// it can never work.
    pub prose: String,
    /// Where it happened, when known.
    pub at: Option<SourceLoc>,
    /// The values the failure names, such as a path or a task id.
    pub facts: BTreeMap<ErrorFact, serde_json::Value>,
}

impl Error {
    /// Builds an error with no location and no facts.
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<String>, prose: impl Into<String>) -> Error {
        Error {
            code,
            message: message.into(),
            prose: prose.into(),
            at: None,
            facts: BTreeMap::new(),
        }
    }

    /// The failure of a canceled run, of a call whose effect the caller
    /// dropped, or of a block that re-raised a caught `canceled` error value
    /// with no `task` after another suspending call.
    #[must_use]
    pub fn canceled() -> Error {
        make_failure_error(
            ErrorCode::Canceled,
            "canceled: the run was canceled or this call was stopped",
        )
    }

    /// Whether this error is a cancel rather than a failure: its code is
    /// [`ErrorCode::Canceled`]. A run that ends with an error ends canceled
    /// exactly when this holds.
    #[must_use]
    pub fn is_canceled(&self) -> bool {
        self.code == ErrorCode::Canceled
    }

    /// Appends `cause`, and each cause under it, to the message after `: `,
    /// skipping any text the message already holds. A cause whose text
    /// starts with the message's end and then `: ` adds only what follows.
    /// The prose stays as it was.
    #[must_use]
    pub fn with_cause(mut self, cause: &(dyn std::error::Error + 'static)) -> Error {
        let mut next = Some(cause);
        while let Some(current) = next {
            let text = current.to_string();
            if !text.is_empty() && !self.message.contains(&text) {
                let repeated = text
                    .rmatch_indices(": ")
                    .map(|(at, _)| at)
                    .find(|&at| at > 0 && self.message.ends_with(&text[..at]));
                if let Some(at) = repeated {
                    self.message.push_str(&text[at..]);
                } else {
                    self.message.push_str(": ");
                    self.message.push_str(&text);
                }
            }
            next = current.source();
        }
        self
    }

    /// Returns the self-contained text of the failure: the message, with
    /// its causes, then where the failure happened.
    ///
    /// A structure, fence, or list failure, the parser's own, ends with its
    /// prompt name, line, and column, each when known, as
    /// `(prompt {name}, line {line}, column {column})`. A frontmatter failure
    /// ends with its line and column alone, each when known, as
    /// `(line {line}, column {column})`: it predates the prompt's name, and
    /// its `Name` fact, when it has one, is a refused Pack's id. Any other
    /// failure with a location ends with it as `(at {file}:{line})`: its
    /// prompt file and line, or for an internal fault the Rust source file
    /// and line of the broken invariant.
    #[must_use]
    pub fn located_message(&self) -> String {
        let mut rendered = self.message.clone();
        if let Some(location) = location(self) {
            rendered.push_str(" (");
            rendered.push_str(&location);
            rendered.push(')');
        }
        rendered
    }
}

/// Where the failure happened, for the end of its located message, or
/// `None` when the failure has no location or the parser found none of its
/// parts.
fn location(error: &Error) -> Option<String> {
    match error.code {
        ErrorCode::FrontmatterInvalid | ErrorCode::StructureInvalid | ErrorCode::FenceInvalid => {
            let parts: Vec<String> = [
                error
                    .facts
                    .get(&ErrorFact::Name)
                    .filter(|_| error.code != ErrorCode::FrontmatterInvalid)
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
        _ => error
            .at
            .as_ref()
            .map(|at| format!("at {}", coordinates(at))),
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

/// A range in a file, from the start position up to but not including the
/// end. Lines and columns are 1-based, and columns count characters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceLoc {
    /// The file. Its extension tells its kind, such as `.md` for a prompt or
    /// `.rs` for Rust.
    pub file: String,
    /// The line the range starts on.
    pub start_line: NonZeroU32,
    /// The column the range starts at.
    pub start_column: NonZeroU32,
    /// The line of the end position.
    pub end_line: NonZeroU32,
    /// The column of the end position, which the range excludes.
    pub end_column: NonZeroU32,
}

impl SourceLoc {
    /// Builds a range, or `None` when the end comes before the start.
    #[must_use]
    pub fn new(
        file: impl Into<String>,
        start_line: NonZeroU32,
        start_column: NonZeroU32,
        end_line: NonZeroU32,
        end_column: NonZeroU32,
    ) -> Option<SourceLoc> {
        ((end_line, end_column) >= (start_line, start_column)).then(|| SourceLoc {
            file: file.into(),
            start_line,
            start_column,
            end_line,
            end_column,
        })
    }

    /// Builds the range covering all of `line`, from column 1 up to column 1
    /// of the next line. On line `u32::MAX` the range is empty.
    #[must_use]
    pub fn line(file: impl Into<String>, line: NonZeroU32) -> SourceLoc {
        SourceLoc {
            file: file.into(),
            start_line: line,
            start_column: NonZeroU32::MIN,
            end_line: line.saturating_add(1),
            end_column: NonZeroU32::MIN,
        }
    }
}

/// Places `error` in the prompt `file`. This is the only code that gives a
/// location its prompt file or turns a known line into a prompt location:
/// the parser's span constructors build spans with no file, which this
/// completes, and the Lua crate's `make_internal_at_error` writes Rust locations.
///
/// An [`ErrorCode::Internal`] error comes back unchanged, so it keeps its
/// Rust location. Any other error that has a location keeps it, so the
/// first placement wins; only an empty file in it becomes `file`, since the
/// parser locates a failure before it knows the file. Otherwise a known
/// `line` becomes [`SourceLoc::line`] in `file`, and with no `line` the
/// error comes back unchanged.
#[must_use]
pub fn place(mut error: Error, file: &str, line: Option<NonZeroU32>) -> Error {
    if error.code == ErrorCode::Internal {
        return error;
    }
    match &mut error.at {
        Some(at) if at.file.is_empty() => file.clone_into(&mut at.file),
        Some(_) => {}
        None => error.at = line.map(|line| SourceLoc::line(file, line)),
    }
    error
}

/// Builds a failure with `code`, `message`, and the code's default prose.
#[must_use]
pub fn make_failure_error(code: ErrorCode, message: impl Into<String>) -> Error {
    Error::new(code, message, code.prose())
}

/// Gives `error` the fact `key` with `value`.
#[must_use]
pub fn with_fact(mut error: Error, key: ErrorFact, value: impl Into<serde_json::Value>) -> Error {
    error.facts.insert(key, value.into());
    error
}

/// Formats `at` as `{file}:{line}`: its file and start line, with no
/// column. This is the only code that formats a location as `file:line`.
#[must_use]
pub fn coordinates(at: &SourceLoc) -> String {
    format!("{}:{}", at.file, at.start_line)
}

/// The name of a fact. Each mirrors a field name in today's Lua error tables,
/// apart from `Limit`, `Section`, `Chain`, and `Run`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ErrorFact {
    /// A file or store path, or a glob pattern, as a string.
    Path,
    /// The name the failure is about, such as a tool alias, a Pack id, or
    /// rejected input, as a string.
    Name,
    /// A task id, as a dotted string.
    Task,
    /// Task ids, as an array of dotted strings.
    Tasks,
    /// An edit's anchor text, as a string.
    Anchor,
    /// A count, such as anchor matches or prompt tokens, as an integer.
    Count,
    /// The limit that was hit, such as a context window or a depth cap, as an
    /// integer.
    Limit,
    /// The tag of the rule that was broken, as a string.
    Rule,
    /// The byte length of the valid UTF-8 prefix, as an integer.
    ValidUpTo,
    /// The tag of a sub-reason within the code, as a string.
    Reason,
    /// The model backend's finish reason, as a string.
    FinishReason,
    /// The section or prompt region the failure is in, as a string.
    Section,
    /// The chain the failure happened in, as a dotted string.
    Chain,
    /// The run id a recorder issued, as an integer.
    Run,
}

/// A stable failure code. Numbers group by family in hundreds: 1xx parse,
/// 2xx Lua, 3xx tasks, 4xx model, 5xx tools, 6xx store, 7xx run, 8xx
/// caller. A number is never reused. Store and model codes are named after
/// the store and model failure kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum ErrorCode {
    /// The frontmatter is missing, unclosed, not valid YAML, or breaks its
    /// schema.
    FrontmatterInvalid = 101,
    /// The document structure is invalid, such as a missing H1 or a
    /// duplicate sibling section.
    StructureInvalid = 102,
    /// A `lua` fence is unclosed or breaks the strict fence rule.
    FenceInvalid = 103,
    /// The prompt declares a `runvil:` major this build does not support.
    UnsupportedVersion = 105,
    /// A value is missing, or failed to parse or validate, such as an id, a
    /// tag, a temperature, or a setting.
    InvalidValue = 106,

    /// Lua failed at runtime, or a call from Lua was refused.
    LuaRuntime = 201,
    /// A Lua region did not compile.
    LuaCompile = 202,
    /// A Lua quota ran out, such as the `log()` event or byte budget.
    LuaQuota = 203,
    /// A `call`, `tasks.spawn`, or `tasks.fanout` arm passed the call-depth
    /// cap.
    CallDepthExceeded = 204,
    /// A `{{ }}` prose substitution failed.
    Substitution = 205,

    /// A chain ended with tasks still live.
    TasksLive = 301,
    /// A task operation named a task the caller does not own.
    TaskNotOwned = 302,
    /// A wait named a task whose result was already delivered.
    TaskConsumed = 303,
    /// A wait delivered a task its owner canceled.
    TaskCanceled = 304,

    /// The request exceeds the model's context window.
    ContextOverflow = 401,
    /// The model backend is limiting the request rate.
    RateLimited = 402,
    /// The billing or usage quota is spent.
    QuotaExhausted = 403,
    /// The model backend is temporarily at capacity.
    Overloaded = 404,
    /// The provider declined the content on policy grounds.
    Refused = 405,
    /// The wait for a reply or the next chunk timed out.
    Timeout = 406,
    /// The connection to the model backend failed or the stream broke.
    Transport = 407,
    /// The model backend reported a fault of its own.
    ServerError = 408,
    /// The model backend refused the request for any other reason.
    Rejected = 409,
    /// The reply was malformed or over the size limit.
    MalformedResponse = 410,
    /// The model's reply had no text and no tool calls.
    EmptyReply = 411,
    /// Model access is turned off or not configured.
    Unavailable = 412,
    /// A tool call or completion was built from invalid parts.
    InvalidParts = 413,
    /// The context window overflowed and the compactor did not compact.
    ContextExhausted = 414,
    /// A model round found no current model.
    ModelRequired = 415,

    /// The tool rejected the arguments it was given.
    ToolInvalidArguments = 501,
    /// The tool's backend refused or failed the request.
    ToolBackend = 502,
    /// The tool call failed at the transport layer.
    ToolTransport = 503,
    /// The tool failed for any other reason.
    ToolOther = 504,
    /// The model called a tool outside the section's offered sets.
    OutOfScopeTool = 505,
    /// A script `tools.call` named no tool in the run.
    UnboundTool = 506,
    /// A script `tools.call` named a tool only a model may call.
    ModelOnlyTool = 507,
    /// The tool-call loop reached its cap without a final reply.
    ToolLoopExhausted = 508,
    /// A tool's model-facing schema did not build.
    ToolSchemaInvalid = 509,
    /// A section's tool sets broke a rule of the merge that builds them.
    ToolSetsInvalid = 510,

    /// The path does not exist.
    NotFound = 601,
    /// The path already exists where creation required absence.
    AlreadyExists = 602,
    /// A directory operation named a non-directory.
    NotADirectory = 603,
    /// A file operation named a directory.
    IsADirectory = 604,
    /// A non-recursive removal named a directory that has entries.
    DirectoryNotEmpty = 605,
    /// Bytes that are not UTF-8 appeared where text was required.
    NotUtf8 = 606,
    /// The path or glob pattern is malformed or escapes the namespace root.
    InvalidPath = 607,
    /// A line range was rejected.
    InvalidRange = 608,
    /// An edit's anchor text was empty, absent, or found more than once.
    Anchor = 609,
    /// The policy denied the operation.
    PermissionDenied = 610,
    /// The policy asked for an approval this layer never grants.
    ApprovalRequired = 611,
    /// The run or tool call that owned the access has ended.
    ScopeEnded = 612,
    /// The mount or backend that serves the path is read-only.
    ReadOnly = 613,
    /// The path resolves outside its backend's root.
    OutsideRoot = 614,
    /// The operation would remove or replace the mounted root.
    RootProtected = 615,
    /// The run declares no store.
    NoStore = 616,
    /// A two-path operation named paths that different mounts serve.
    CrossMount = 617,
    /// An operating-system I/O call failed.
    Io = 618,
    /// The backend does not implement the operation.
    Unsupported = 619,
    /// The operation conflicts with another access's claim on the path.
    Conflict = 620,
    /// The backend failed for any other reason.
    Backend = 621,
    /// Two live executions claimed the same store path.
    Determinism = 622,

    /// The environment falls short of what the prompt needs.
    RequirementsUnmet = 701,
    /// The run was canceled, or the call it waited on was dropped.
    Canceled = 702,
    /// An internal invariant broke; nothing the prompt can fix.
    Internal = 703,
    /// No chain can move and the main chain is not
    /// waiting on its inbox: the run deadlocked. Wire
    /// name `stalled`; names the tasks waiting on their
    /// inboxes. A run code, beside `Internal`.
    Stalled = 704,

    /// The run recorder failed to store a write.
    RecorderFailed = 801,
    /// A Plugin entry point did not open or load.
    PluginLoadFailed = 802,
    /// The run report has no output text: the run did not complete, or the
    /// prompt declares no `output:` file.
    NoOutput = 803,
}

impl ErrorCode {
    /// Returns the code's number.
    #[must_use]
    pub const fn number(self) -> u16 {
        self as u16
    }

    /// Returns the code's stable tag: its name in snake_case, such as
    /// `not_found` or `rate_limited`. No two codes share a tag. Store and
    /// model codes are named after the store and model failure kinds.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            ErrorCode::FrontmatterInvalid => "frontmatter_invalid",
            ErrorCode::StructureInvalid => "structure_invalid",
            ErrorCode::FenceInvalid => "fence_invalid",
            ErrorCode::UnsupportedVersion => "unsupported_version",
            ErrorCode::InvalidValue => "invalid_value",
            ErrorCode::LuaRuntime => "lua_runtime",
            ErrorCode::LuaCompile => "lua_compile",
            ErrorCode::LuaQuota => "lua_quota",
            ErrorCode::CallDepthExceeded => "call_depth_exceeded",
            ErrorCode::Substitution => "substitution",
            ErrorCode::TasksLive => "tasks_live",
            ErrorCode::TaskNotOwned => "task_not_owned",
            ErrorCode::TaskConsumed => "task_consumed",
            ErrorCode::TaskCanceled => "task_canceled",
            ErrorCode::ContextOverflow => "context_overflow",
            ErrorCode::RateLimited => "rate_limited",
            ErrorCode::QuotaExhausted => "quota_exhausted",
            ErrorCode::Overloaded => "overloaded",
            ErrorCode::Refused => "refused",
            ErrorCode::Timeout => "timeout",
            ErrorCode::Transport => "transport",
            ErrorCode::ServerError => "server_error",
            ErrorCode::Rejected => "rejected",
            ErrorCode::MalformedResponse => "malformed_response",
            ErrorCode::EmptyReply => "empty_reply",
            ErrorCode::Unavailable => "unavailable",
            ErrorCode::InvalidParts => "invalid_parts",
            ErrorCode::ContextExhausted => "context_exhausted",
            ErrorCode::ModelRequired => "model_required",
            ErrorCode::ToolInvalidArguments => "tool_invalid_arguments",
            ErrorCode::ToolBackend => "tool_backend",
            ErrorCode::ToolTransport => "tool_transport",
            ErrorCode::ToolOther => "tool_other",
            ErrorCode::OutOfScopeTool => "out_of_scope_tool",
            ErrorCode::UnboundTool => "unbound_tool",
            ErrorCode::ModelOnlyTool => "model_only_tool",
            ErrorCode::ToolLoopExhausted => "tool_loop_exhausted",
            ErrorCode::ToolSchemaInvalid => "tool_schema_invalid",
            ErrorCode::ToolSetsInvalid => "tool_sets_invalid",
            ErrorCode::NotFound => "not_found",
            ErrorCode::AlreadyExists => "already_exists",
            ErrorCode::NotADirectory => "not_a_directory",
            ErrorCode::IsADirectory => "is_a_directory",
            ErrorCode::DirectoryNotEmpty => "directory_not_empty",
            ErrorCode::NotUtf8 => "not_utf8",
            ErrorCode::InvalidPath => "invalid_path",
            ErrorCode::InvalidRange => "invalid_range",
            ErrorCode::Anchor => "anchor",
            ErrorCode::PermissionDenied => "permission_denied",
            ErrorCode::ApprovalRequired => "approval_required",
            ErrorCode::ScopeEnded => "scope_ended",
            ErrorCode::ReadOnly => "read_only",
            ErrorCode::OutsideRoot => "outside_root",
            ErrorCode::RootProtected => "root_protected",
            ErrorCode::NoStore => "no_store",
            ErrorCode::CrossMount => "cross_mount",
            ErrorCode::Io => "io",
            ErrorCode::Unsupported => "unsupported",
            ErrorCode::Conflict => "conflict",
            ErrorCode::Backend => "backend",
            ErrorCode::Determinism => "determinism",
            ErrorCode::RequirementsUnmet => "requirements_unmet",
            ErrorCode::Canceled => "canceled",
            ErrorCode::Internal => "internal",
            ErrorCode::Stalled => "stalled",
            ErrorCode::RecorderFailed => "recorder_failed",
            ErrorCode::PluginLoadFailed => "plugin_load_failed",
            ErrorCode::NoOutput => "no_output",
        }
    }

    /// Parses a tag from [`ErrorCode::tag`] back into its code, so the two
    /// round-trip. Returns `None` for any other string.
    #[must_use]
    pub fn from_tag(tag: &str) -> Option<ErrorCode> {
        Some(match tag {
            "frontmatter_invalid" => ErrorCode::FrontmatterInvalid,
            "structure_invalid" => ErrorCode::StructureInvalid,
            "fence_invalid" => ErrorCode::FenceInvalid,
            "unsupported_version" => ErrorCode::UnsupportedVersion,
            "invalid_value" => ErrorCode::InvalidValue,
            "lua_runtime" => ErrorCode::LuaRuntime,
            "lua_compile" => ErrorCode::LuaCompile,
            "lua_quota" => ErrorCode::LuaQuota,
            "call_depth_exceeded" => ErrorCode::CallDepthExceeded,
            "substitution" => ErrorCode::Substitution,
            "tasks_live" => ErrorCode::TasksLive,
            "task_not_owned" => ErrorCode::TaskNotOwned,
            "task_consumed" => ErrorCode::TaskConsumed,
            "task_canceled" => ErrorCode::TaskCanceled,
            "context_overflow" => ErrorCode::ContextOverflow,
            "rate_limited" => ErrorCode::RateLimited,
            "quota_exhausted" => ErrorCode::QuotaExhausted,
            "overloaded" => ErrorCode::Overloaded,
            "refused" => ErrorCode::Refused,
            "timeout" => ErrorCode::Timeout,
            "transport" => ErrorCode::Transport,
            "server_error" => ErrorCode::ServerError,
            "rejected" => ErrorCode::Rejected,
            "malformed_response" => ErrorCode::MalformedResponse,
            "empty_reply" => ErrorCode::EmptyReply,
            "unavailable" => ErrorCode::Unavailable,
            "invalid_parts" => ErrorCode::InvalidParts,
            "context_exhausted" => ErrorCode::ContextExhausted,
            "model_required" => ErrorCode::ModelRequired,
            "tool_invalid_arguments" => ErrorCode::ToolInvalidArguments,
            "tool_backend" => ErrorCode::ToolBackend,
            "tool_transport" => ErrorCode::ToolTransport,
            "tool_other" => ErrorCode::ToolOther,
            "out_of_scope_tool" => ErrorCode::OutOfScopeTool,
            "unbound_tool" => ErrorCode::UnboundTool,
            "model_only_tool" => ErrorCode::ModelOnlyTool,
            "tool_loop_exhausted" => ErrorCode::ToolLoopExhausted,
            "tool_schema_invalid" => ErrorCode::ToolSchemaInvalid,
            "tool_sets_invalid" => ErrorCode::ToolSetsInvalid,
            "not_found" => ErrorCode::NotFound,
            "already_exists" => ErrorCode::AlreadyExists,
            "not_a_directory" => ErrorCode::NotADirectory,
            "is_a_directory" => ErrorCode::IsADirectory,
            "directory_not_empty" => ErrorCode::DirectoryNotEmpty,
            "not_utf8" => ErrorCode::NotUtf8,
            "invalid_path" => ErrorCode::InvalidPath,
            "invalid_range" => ErrorCode::InvalidRange,
            "anchor" => ErrorCode::Anchor,
            "permission_denied" => ErrorCode::PermissionDenied,
            "approval_required" => ErrorCode::ApprovalRequired,
            "scope_ended" => ErrorCode::ScopeEnded,
            "read_only" => ErrorCode::ReadOnly,
            "outside_root" => ErrorCode::OutsideRoot,
            "root_protected" => ErrorCode::RootProtected,
            "no_store" => ErrorCode::NoStore,
            "cross_mount" => ErrorCode::CrossMount,
            "io" => ErrorCode::Io,
            "unsupported" => ErrorCode::Unsupported,
            "conflict" => ErrorCode::Conflict,
            "backend" => ErrorCode::Backend,
            "determinism" => ErrorCode::Determinism,
            "requirements_unmet" => ErrorCode::RequirementsUnmet,
            "canceled" => ErrorCode::Canceled,
            "internal" => ErrorCode::Internal,
            "stalled" => ErrorCode::Stalled,
            "recorder_failed" => ErrorCode::RecorderFailed,
            "plugin_load_failed" => ErrorCode::PluginLoadFailed,
            "no_output" => ErrorCode::NoOutput,
            _ => return None,
        })
    }

    /// Returns the default text the model reads for this code: what went
    /// wrong and, when the model can act on it, what to do instead. It never
    /// names a file, a line, or an internal cause. A site with specifics
    /// writes its own. The text for a failure in the prompt itself speaks to
    /// a model fixing the prompt, and its site puts what failed in front,
    /// after the location only for a parse failure.
    #[must_use]
    #[expect(
        clippy::too_many_lines,
        reason = "one arm per code holds its default text"
    )]
    pub const fn prose(self) -> &'static str {
        match self {
            ErrorCode::FrontmatterInvalid => {
                "The frontmatter is invalid. Fix the YAML between the `---` lines so it matches the \
                 frontmatter schema."
            }
            ErrorCode::StructureInvalid => {
                "The prompt's headings are invalid. Give the prompt exactly one H1 title, and give \
                 every section a non-empty name that is unique among its siblings and a parent \
                 heading."
            }
            ErrorCode::FenceInvalid => {
                "A Lua code fence is malformed. Open it with exactly ```lua and close it with \
                 exactly ```."
            }
            ErrorCode::UnsupportedVersion => {
                "The prompt declares a `runvil:` version this build does not support. Declare a \
                 version it supports."
            }
            ErrorCode::InvalidValue => {
                "A value was missing or not valid. Check it against what the operation expects and \
                 try again with a valid one."
            }
            ErrorCode::LuaRuntime => {
                "The Lua code failed at runtime. Fix the code at this location."
            }
            ErrorCode::LuaCompile => {
                "The Lua code does not compile. Fix the syntax at this location."
            }
            ErrorCode::LuaQuota => "The section logged more than its budget allows. Log less.",
            ErrorCode::CallDepthExceeded => {
                "Calls nest deeper than the cap allows. Nest `call`, `tasks.spawn`, and \
                 `tasks.fanout` less deeply."
            }
            ErrorCode::Substitution => {
                "A placeholder in the section's prose could not be filled. Fix the placeholder, or \
                 set the value it reads before the prose is read."
            }
            ErrorCode::TasksLive => {
                "The chain ended with tasks still running. Wait on or cancel every task the chain \
                 spawns before it ends."
            }
            ErrorCode::TaskNotOwned => {
                "The task is not one this chain owns. Wait on, inspect, or cancel only the tasks \
                 this chain spawned."
            }
            ErrorCode::TaskConsumed => {
                "The task's result was already delivered. Wait on each task only once."
            }
            ErrorCode::TaskCanceled => {
                "The task was canceled before it finished, so it has no result. Handle the \
                 cancellation where the task is waited on."
            }
            ErrorCode::ContextOverflow => {
                "The request is larger than the model's context window. Send less text and try again."
            }
            ErrorCode::RateLimited => {
                "The model backend is limiting the request rate. Wait, then try again."
            }
            ErrorCode::QuotaExhausted => {
                "The model backend's usage quota is spent, so retrying will not work until it is \
                 restored."
            }
            ErrorCode::Overloaded => "The model backend is overloaded. Wait, then try again.",
            ErrorCode::Refused => {
                "The model backend refused the request on content policy grounds. Retrying the same \
                 request will not work; change it instead."
            }
            ErrorCode::Timeout => "The model backend did not answer in time. Try again.",
            ErrorCode::Transport => "The connection to the model backend failed. Try again.",
            ErrorCode::ServerError => "The model backend reported a fault of its own. Try again.",
            ErrorCode::Rejected => {
                "The model backend rejected the request. Retrying the same request will not work; \
                 change it instead."
            }
            ErrorCode::MalformedResponse => {
                "The model backend sent a reply that could not be understood. Try again."
            }
            ErrorCode::EmptyReply => {
                "The reply had no text and no tool calls. Reply with text or call a tool."
            }
            ErrorCode::Unavailable => {
                "Model access is turned off or not configured, so model calls will not work."
            }
            ErrorCode::InvalidParts => {
                "The tool call or completion was built from invalid parts, so it was not sent."
            }
            ErrorCode::ContextExhausted => {
                "The conversation no longer fits the model's context window and could not be \
                 shortened. Finish with what you have."
            }
            ErrorCode::ModelRequired => {
                "The round has no model. Select a model before `models.infer` or `models.loop`, or \
                 bind a default model."
            }
            ErrorCode::ToolInvalidArguments => {
                "The tool rejected its arguments. Check the tool's parameters and call it again with \
                 valid arguments."
            }
            ErrorCode::ToolBackend => {
                "The tool's backend failed or refused the request. Try again, or use a different tool."
            }
            ErrorCode::ToolTransport => "The tool could not reach its backend. Try again.",
            ErrorCode::ToolOther => "The tool failed.",
            ErrorCode::OutOfScopeTool => {
                "That tool is not offered here. Call only the tools you were offered."
            }
            ErrorCode::UnboundTool => {
                "No tool in this run has that id. Call a tool id from the run's catalog."
            }
            ErrorCode::ModelOnlyTool => {
                "Only a model may call that tool, so a script call to it will never work. Offer it \
                 with `tools.offer` and let the model call it."
            }
            ErrorCode::ToolLoopExhausted => {
                "The tool-call limit was reached before a final reply. Stop calling tools and reply \
                 with what you have."
            }
            ErrorCode::ToolSchemaInvalid => {
                "A tool's model-facing schema did not build. Fix the tool's name or its parameter \
                 schema."
            }
            ErrorCode::ToolSetsInvalid => {
                "The section's tool sets break a rule of the merge that builds them. Fix the offers, \
                 or the render that answered them."
            }
            ErrorCode::NotFound => {
                "That path does not exist. Check the path, or list its directory to find the right \
                 one."
            }
            ErrorCode::AlreadyExists => {
                "That path already exists. Use a different path, or remove the existing one first."
            }
            ErrorCode::NotADirectory => {
                "That path is not a directory. Use a directory path for this operation."
            }
            ErrorCode::IsADirectory => {
                "That path is a directory. Use a file path for this operation."
            }
            ErrorCode::DirectoryNotEmpty => {
                "That directory is not empty. Remove it recursively, or empty it first."
            }
            ErrorCode::NotUtf8 => "That file is not UTF-8 text, so it cannot be read as text.",
            ErrorCode::InvalidPath => {
                "That path is not valid. Use a relative path with no `.` or `..` segments, no \
                 backslashes, and no empty segments."
            }
            ErrorCode::InvalidRange => {
                "That line range is not valid. Use a range within the file's lines."
            }
            ErrorCode::Anchor => {
                "The text to replace was empty, missing, or found more than once. Use text that \
                 appears exactly once in the file."
            }
            ErrorCode::PermissionDenied => {
                "The policy does not allow that operation, so it will never work here."
            }
            ErrorCode::ApprovalRequired => {
                "That operation needs an approval that cannot be given here, so it will never work."
            }
            ErrorCode::ScopeEnded => {
                "The access for that operation has ended, so it will never work again."
            }
            ErrorCode::ReadOnly => {
                "That path is read-only, so writing to it will never work. Write to a different path."
            }
            ErrorCode::OutsideRoot => {
                "That path leads outside the allowed directory, so it will never work."
            }
            ErrorCode::RootProtected => "The root directory cannot be removed or replaced.",
            ErrorCode::NoStore => "This run has no store, so file operations will not work.",
            ErrorCode::CrossMount => {
                "Those paths are on different mounts, so this will never work. Use paths on the same \
                 mount."
            }
            ErrorCode::Io => "The file operation failed. Try again.",
            ErrorCode::Unsupported => "That operation is not supported for this path.",
            ErrorCode::Conflict => {
                "Another operation is using that path at the same time. Try again after it finishes."
            }
            ErrorCode::Backend => "The file operation failed.",
            ErrorCode::Determinism => {
                "Two concurrent operations used the same store path, so the run stopped."
            }
            ErrorCode::RequirementsUnmet => {
                "The environment does not provide what the prompt needs. Supply the missing Packs or \
                 models, or change what the prompt declares."
            }
            ErrorCode::Canceled => "The operation was canceled.",
            ErrorCode::Internal => "An internal error occurred. Retrying will not help.",
            ErrorCode::Stalled => {
                "No chain can move, so the run stopped. Send each task that waits on its inbox the \
                 message it waits for, or stop waiting on that task."
            }
            ErrorCode::RecorderFailed => "The run could not be recorded, so it stopped.",
            ErrorCode::PluginLoadFailed => {
                "A plugin could not be loaded, so its tools are not available."
            }
            ErrorCode::NoOutput => "The run produced no output text.",
        }
    }
}
