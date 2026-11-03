//! The default text the model reads for each code.

use super::ErrorCode;

impl ErrorCode {
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
            ErrorCode::ListEmptyItem => {
                "A list section has an empty item. Fill it in or remove it."
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
            ErrorCode::TaskCancelled => {
                "The task was cancelled before it finished, so it has no result. Handle the \
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
            ErrorCode::Interrupted => "The operation was cancelled.",
            ErrorCode::Internal => "An internal error occurred. Retrying will not help.",
            ErrorCode::RecorderFailed => "The run could not be recorded, so it stopped.",
            ErrorCode::PluginLoadFailed => {
                "A plugin could not be loaded, so its tools are not available."
            }
            ErrorCode::NoOutput => "The run produced no output text.",
        }
    }
}
