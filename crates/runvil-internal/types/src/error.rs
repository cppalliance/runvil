//! The universal error: one [`Error`] record that can describe any Engine
//! failure, by a stable [`ErrorCode`], text for people and for the model,
//! an optional [`Location`], and values named by [`FactKey`].

use std::collections::BTreeMap;
use std::fmt;
use std::num::NonZeroU32;

#[path = "error-prose.rs"]
mod prose;
#[path = "error-tag.rs"]
mod tag;
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
    pub at: Option<Location>,
    /// The values the failure names, such as a path or a task id.
    pub facts: BTreeMap<FactKey, serde_json::Value>,
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
        failure(
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
        ErrorCode::FrontmatterInvalid
        | ErrorCode::StructureInvalid
        | ErrorCode::FenceInvalid
        | ErrorCode::ListEmptyItem => {
            let parts: Vec<String> = [
                error
                    .facts
                    .get(&FactKey::Name)
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
pub struct Location {
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

impl Location {
    /// Builds a range, or `None` when the end comes before the start.
    #[must_use]
    pub fn new(
        file: impl Into<String>,
        start_line: NonZeroU32,
        start_column: NonZeroU32,
        end_line: NonZeroU32,
        end_column: NonZeroU32,
    ) -> Option<Location> {
        ((end_line, end_column) >= (start_line, start_column)).then(|| Location {
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
    pub fn line(file: impl Into<String>, line: NonZeroU32) -> Location {
        Location {
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
/// completes, and the Lua crate's `internal_at` writes Rust locations.
///
/// An [`ErrorCode::Internal`] error comes back unchanged, so it keeps its
/// Rust location. Any other error that has a location keeps it, so the
/// first placement wins; only an empty file in it becomes `file`, since the
/// parser locates a failure before it knows the file. Otherwise a known
/// `line` becomes [`Location::line`] in `file`, and with no `line` the
/// error comes back unchanged.
#[must_use]
pub fn place(mut error: Error, file: &str, line: Option<NonZeroU32>) -> Error {
    if error.code == ErrorCode::Internal {
        return error;
    }
    match &mut error.at {
        Some(at) if at.file.is_empty() => file.clone_into(&mut at.file),
        Some(_) => {}
        None => error.at = line.map(|line| Location::line(file, line)),
    }
    error
}

/// Builds a failure with `code`, `message`, and the code's default prose.
#[must_use]
pub fn failure(code: ErrorCode, message: impl Into<String>) -> Error {
    Error::new(code, message, code.prose())
}

/// Gives `error` the fact `key` with `value`.
#[must_use]
pub fn with_fact(mut error: Error, key: FactKey, value: impl Into<serde_json::Value>) -> Error {
    error.facts.insert(key, value.into());
    error
}

/// Formats `at` as `{file}:{line}`: its file and start line, with no
/// column. This is the only code that formats a location as `file:line`.
#[must_use]
pub fn coordinates(at: &Location) -> String {
    format!("{}:{}", at.file, at.start_line)
}

/// The name of a fact. Each mirrors a field name in today's Lua error tables,
/// apart from `Limit`, `Section`, `Chain`, and `Run`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FactKey {
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
    /// A list section has an empty item.
    ListEmptyItem = 104,
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
}
