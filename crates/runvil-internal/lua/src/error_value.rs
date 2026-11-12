//! The structured error value every failure takes when it reaches Lua.
//!
//! A failure that crosses into author code - a shim's own argument error,
//! a Rust-raised error answered through the `(ok, result)` envelope, a
//! Engine function's own failure (`tools.offer`, `models.get`, a `sys` or `var`
//! guard) caught by `pcall`, or a shim raise such as
//! `make_tool_loop_exhausted_error` - is one Lua table `{ kind, code, message, ... }`
//! under a shared metatable whose `__tostring` returns `message`. The
//! message is the failure's prose, the text an author may hand a model,
//! which a table rendered from a universal error placed on a prompt line
//! puts after that line's `file:line`; a caller that branches reads
//! `kind`, `code`, the stable tag of the failure's [`ErrorCode`], and the
//! kind's own fields (such as `reason` for `make_context_exhausted_error` and
//! `finish_reason` for `empty_model_reply`; [`error_fields`] gives them
//! all).
//!
//! [`ErrorKind`] names the closed vocabulary of kinds; [`error_kind`] and
//! [`error_fields`] render a universal error into the shape;
//! [`normalized`] is what the shim's `pcall` and `xpcall` replacements
//! run a caught value through (so a Rust callback's failure, which mlua
//! raises as an opaque userdata, takes the same shape); and
//! [`raised_from`] reads a table back into a universal error when a
//! raise surfaces as a block coroutine's failure, so the failure survives
//! the boundary in both directions. A table rendered from a universal
//! error keeps that error beside it, which [`retained_error`] reads back
//! whole.

use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::str::FromStr;

use mlua::{AnyUserData, Function, Lua, Table, Value};

use runvil_types::error::{Error, ErrorCode, ErrorFact, coordinates};
use runvil_types::ids::TaskId;
use serde_json::Value as Json;

use crate::chunk_line::first_prompt_line;
use crate::error::{
    accepted_tags, author_line, authored_at, call_line, failure_text, make_internal_error,
    make_lua_runtime_error, prompt_file, root_cause, root_text,
};

/// The registry key of the shared error metatable, created on first use per
/// VM so every error table on that VM - Lua-built or Rust-built - shares
/// the same identity and the read-back can recognize it.
const METATABLE_REGISTRY: &str = "runvil.error_value.metatable";

/// The registry key of the VM's weak-keyed table from each error table
/// rendered from a universal error to that error, created on first use.
const RETAINED_REGISTRY: &str = "runvil.error_value.retained";

/// The closed vocabulary of failure kinds an author can branch on.
///
/// The tag is the `kind` string the Lua table holds; the set is fixed by
/// the protocol and a new failure classifies into one of these rather than
/// inventing a tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    /// The model tool loop ran its iteration cap without a final reply.
    ToolLoopExhausted,
    /// A request overflowed the model's context window and the selected
    /// compactor did not compact, or a caller raised it through
    /// `compactors.fail` with its own reason; includes `reason`.
    ContextExhausted,
    /// The model returned a turn with no product; includes `finish_reason`
    /// when the backend supplied one.
    EmptyModelReply,
    /// A model round or a catalog fetch failed for any reason other than an
    /// empty reply: transport, backend, decode, or configuration.
    ModelFailure,
    /// A model round found no selected model and no prompt-wide default.
    ModelRequired,
    /// The model named a tool outside the section's advertised scope;
    /// includes `name`.
    OutOfScopeTool,
    /// A script `tools.call` named an id that is no adhoc tool and no
    /// catalog tool in the run; includes `name`.
    UnboundTool,
    /// A script `tools.call` named a catalog tool that only a model may
    /// call, because a render shapes its arguments for the model; includes
    /// `name`.
    ModelOnlyTool,
    /// A dispatched tool's own failure.
    Tool,
    /// A task operation named a task the caller does not own; includes
    /// `task`.
    TaskNotOwned,
    /// A task's result was already delivered once; includes `task`.
    TaskConsumed,
    /// A section ended while tasks it owns were still live; includes
    /// `tasks`, the ids joined by `, `.
    TasksLive,
    /// The run was canceled, the caller dropped a call the run was waiting
    /// on, or a wait delivered a task its owner canceled (that value
    /// includes `task`).
    Canceled,
    /// A store operation's own failure; includes `reason` and the
    /// [`VfsError`](runvil_vfs::VfsError) variant's fields.
    Store,
    /// A Lua authoring or runtime failure: a compile error, a runtime error
    /// or memory refusal in author code, an Engine function's argument
    /// error, a failed substitution, the call-depth cap, or an exhausted
    /// Engine quota.
    Lua,
    /// An internal invariant was violated, or a failure outside the prompt the
    /// author cannot act on that has no kind of its own.
    Internal,
}

impl ErrorKind {
    /// Every kind, in declaration order.
    pub(crate) const ALL: &'static [ErrorKind] = &[
        ErrorKind::ToolLoopExhausted,
        ErrorKind::ContextExhausted,
        ErrorKind::EmptyModelReply,
        ErrorKind::ModelFailure,
        ErrorKind::ModelRequired,
        ErrorKind::OutOfScopeTool,
        ErrorKind::UnboundTool,
        ErrorKind::ModelOnlyTool,
        ErrorKind::Tool,
        ErrorKind::TaskNotOwned,
        ErrorKind::TaskConsumed,
        ErrorKind::TasksLive,
        ErrorKind::Canceled,
        ErrorKind::Store,
        ErrorKind::Lua,
        ErrorKind::Internal,
    ];

    /// Returns the `kind` string the Lua table holds.
    #[must_use]
    pub(crate) fn tag(self) -> &'static str {
        match self {
            ErrorKind::ToolLoopExhausted => "tool_loop_exhausted",
            ErrorKind::ContextExhausted => "context_exhausted",
            ErrorKind::EmptyModelReply => "empty_model_reply",
            ErrorKind::ModelFailure => "model_failure",
            ErrorKind::ModelRequired => "model_required",
            ErrorKind::OutOfScopeTool => "out_of_scope_tool",
            ErrorKind::UnboundTool => "unbound_tool",
            ErrorKind::ModelOnlyTool => "model_only_tool",
            ErrorKind::Tool => "tool",
            ErrorKind::TaskNotOwned => "task_not_owned",
            ErrorKind::TaskConsumed => "task_consumed",
            ErrorKind::TasksLive => "tasks_live",
            ErrorKind::Canceled => "canceled",
            ErrorKind::Store => "store",
            ErrorKind::Lua => "lua",
            ErrorKind::Internal => "internal",
        }
    }

    /// Parses a `kind` string; `None` for a tag outside the vocabulary.
    #[must_use]
    pub(crate) fn from_tag(tag: &str) -> Option<ErrorKind> {
        match tag {
            "tool_loop_exhausted" => Some(ErrorKind::ToolLoopExhausted),
            "context_exhausted" => Some(ErrorKind::ContextExhausted),
            "empty_model_reply" => Some(ErrorKind::EmptyModelReply),
            "model_failure" => Some(ErrorKind::ModelFailure),
            "model_required" => Some(ErrorKind::ModelRequired),
            "out_of_scope_tool" => Some(ErrorKind::OutOfScopeTool),
            "unbound_tool" => Some(ErrorKind::UnboundTool),
            "model_only_tool" => Some(ErrorKind::ModelOnlyTool),
            "tool" => Some(ErrorKind::Tool),
            "task_not_owned" => Some(ErrorKind::TaskNotOwned),
            "task_consumed" => Some(ErrorKind::TaskConsumed),
            "tasks_live" => Some(ErrorKind::TasksLive),
            "canceled" => Some(ErrorKind::Canceled),
            "store" => Some(ErrorKind::Store),
            "lua" => Some(ErrorKind::Lua),
            "internal" => Some(ErrorKind::Internal),
            _ => None,
        }
    }
}

impl std::fmt::Display for ErrorKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.tag())
    }
}

impl FromStr for ErrorKind {
    type Err = Error;

    /// Parses a kind from its exact tag, as `ErrorKind::from_tag` does. Other
    /// text is an [`InvalidValue`](ErrorCode::InvalidValue) error that names
    /// it and lists every tag.
    fn from_str(tag: &str) -> Result<ErrorKind, Self::Err> {
        ErrorKind::from_tag(tag).ok_or_else(|| {
            let text = format!(
                "invalid error kind `{tag}`: required one of {}",
                accepted_tags(ErrorKind::ALL.iter().map(|kind| kind.tag()))
            );
            let mut error = Error::new(ErrorCode::InvalidValue, text.clone(), text);
            error
                .facts
                .insert(ErrorFact::Name, serde_json::Value::from(tag));
            error
        })
    }
}

/// One field of an error table: a string or an integer, so a field like
/// a store anchor's `count` stays numeric in Lua instead of being
/// stringified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorField {
    /// A string-valued field.
    String(String),
    /// An integer-valued field (a store anchor's `count`, a UTF-8 failure's
    /// `valid_up_to`).
    Integer(i64),
}

impl ErrorField {
    /// The field's string value, when it is one.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            ErrorField::String(value) => Some(value),
            ErrorField::Integer(_) => None,
        }
    }
}

/// The kind a failure with `code` renders as. A model round's failure is
/// `model_failure`, or `empty_model_reply` for an empty reply, and a round
/// with no model is `make_model_required_error`; every Lua-phase failure, an exhausted
/// quota and the call-depth cap included, is `lua`; a store operation's own
/// failure is `store`; a canceled task's delivery is `canceled`, as any
/// other cancel is; and a failure outside the prompt the author cannot act
/// on (a parse failure, a failed tool freeze, a refused requirement, a
/// make_determinism_error violation, a stalled run, which ends the run
/// before any Lua could catch it, an internal fault) is `internal`.
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
        ErrorCode::Canceled | ErrorCode::TaskCanceled => ErrorKind::Canceled,
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
        | ErrorCode::UnsupportedVersion
        | ErrorCode::InvalidValue
        | ErrorCode::ToolSchemaInvalid
        | ErrorCode::ToolSetsInvalid
        | ErrorCode::Determinism
        | ErrorCode::RequirementsUnmet
        | ErrorCode::Internal
        | ErrorCode::Stalled
        | ErrorCode::RecorderFailed
        | ErrorCode::PluginLoadFailed
        | ErrorCode::NoOutput => ErrorKind::Internal,
    }
}

/// The one string field a failure with `code` holds and the fact behind
/// it, which both [`error_fields`] and [`field_facts`] read; `None` for a
/// code whose table holds no such field.
fn string_fact(code: ErrorCode) -> Option<(&'static str, ErrorFact)> {
    match code {
        ErrorCode::ContextExhausted => Some(("reason", ErrorFact::Reason)),
        ErrorCode::EmptyReply => Some(("finish_reason", ErrorFact::FinishReason)),
        ErrorCode::OutOfScopeTool | ErrorCode::UnboundTool | ErrorCode::ModelOnlyTool => {
            Some(("name", ErrorFact::Name))
        }
        ErrorCode::TaskNotOwned | ErrorCode::TaskConsumed | ErrorCode::TaskCanceled => {
            Some(("task", ErrorFact::Task))
        }
        _ => None,
    }
}

/// The fact `key` of `error` as the string field `name`, or nothing when
/// the fact is absent or not a string.
fn string_field(error: &Error, name: &str, key: ErrorFact) -> Vec<(String, ErrorField)> {
    error
        .facts
        .get(&key)
        .and_then(Json::as_str)
        .map(|value| vec![(name.to_owned(), ErrorField::String(value.to_owned()))])
        .unwrap_or_default()
}

/// The `Tasks` fact's ids as one string joined by `, `, the `tasks` field
/// of a `make_tasks_live_error` table.
fn joined_tasks(error: &Error) -> Vec<(String, ErrorField)> {
    let Some(Json::Array(tasks)) = error.facts.get(&ErrorFact::Tasks) else {
        return Vec::new();
    };
    let joined = tasks
        .iter()
        .filter_map(Json::as_str)
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
/// operation's refusal or a canceled task's delivery, and a store
/// failure's `reason` and fields. Every other code's table has no fields,
/// whatever facts it holds.
#[must_use]
pub fn error_fields(error: &Error) -> Vec<(String, ErrorField)> {
    match error.code {
        ErrorCode::TasksLive => joined_tasks(error),
        code if kind_of(code) == ErrorKind::Store => store_fact_fields(error),
        code => string_fact(code)
            .map(|(name, key)| string_field(error, name, key))
            .unwrap_or_default(),
    }
}

/// Returns the facts an error table's `fields` hold for a failure with
/// `code`, the inverse of [`error_fields`]: each field that code's table
/// gives, under its fact, with `tasks` split back into the ids it joins.
/// A field of another shape, or one the code's table never gives, holds
/// no fact.
fn field_facts(
    code: ErrorCode,
    fields: &BTreeMap<String, ErrorField>,
) -> BTreeMap<ErrorFact, Json> {
    match code {
        ErrorCode::TasksLive => fields
            .get("tasks")
            .and_then(ErrorField::as_str)
            .map(|joined| {
                let tasks: Vec<&str> = joined.split(", ").filter(|id| !id.is_empty()).collect();
                (ErrorFact::Tasks, Json::from(tasks))
            })
            .into_iter()
            .collect(),
        code if kind_of(code) == ErrorKind::Store => store_field_facts(fields),
        code => string_fact(code)
            .and_then(|(name, key)| {
                let value = fields.get(name)?.as_str()?;
                Some((key, Json::from(value)))
            })
            .into_iter()
            .collect(),
    }
}

/// Each store field's name in a Lua error table beside the fact that holds
/// it in a universal error, in the order a table lists them.
const STORE_FACTS: [(&str, ErrorFact); 5] = [
    ("path", ErrorFact::Path),
    ("valid_up_to", ErrorFact::ValidUpTo),
    ("rule", ErrorFact::Rule),
    ("anchor", ErrorFact::Anchor),
    ("count", ErrorFact::Count),
];

/// Returns the variant's own fields, without `reason`, in the shape
/// `make_store_error` documents.
fn store_error_fields(error: &runvil_vfs::VfsError) -> Vec<(String, ErrorField)> {
    use runvil_vfs::VfsError;
    let integer = |value: usize| ErrorField::Integer(i64::try_from(value).unwrap_or(i64::MAX));
    match error {
        VfsError::NotFound { path }
        | VfsError::AlreadyExists { path }
        | VfsError::NotADirectory { path }
        | VfsError::IsADirectory { path }
        | VfsError::DirectoryNotEmpty { path }
        | VfsError::InvalidRange { path, .. }
        | VfsError::PermissionDenied { path, .. }
        | VfsError::ApprovalRequired { path, .. }
        | VfsError::ScopeEnded { path }
        | VfsError::ReadOnly { path }
        | VfsError::OutsideRoot { path, .. }
        | VfsError::RootProtected { path }
        | VfsError::CrossMount { path, .. }
        | VfsError::Io { path, .. }
        | VfsError::Unsupported { path, .. }
        | VfsError::Conflict { path, .. } => {
            vec![("path".to_owned(), ErrorField::String(path.clone()))]
        }
        VfsError::NotUtf8 { path, valid_up_to } => vec![
            ("path".to_owned(), ErrorField::String(path.clone())),
            ("valid_up_to".to_owned(), integer(*valid_up_to)),
        ],
        VfsError::InvalidPath { path, reason } => vec![
            ("path".to_owned(), ErrorField::String(path.clone())),
            (
                "rule".to_owned(),
                ErrorField::String(reason.tag().to_owned()),
            ),
        ],
        VfsError::Anchor {
            path,
            anchor,
            count,
        } => vec![
            ("path".to_owned(), ErrorField::String(path.clone())),
            ("anchor".to_owned(), ErrorField::String(anchor.clone())),
            ("count".to_owned(), integer(*count)),
        ],
        // `NoStore` and `Backend` name no path, and `VfsError` is
        // `#[non_exhaustive]`: a variant added later reports no fields
        // until it is named here.
        _ => Vec::new(),
    }
}

/// Returns the full field set a store error value carries, `reason` first,
/// in the shape [`make_store_error`] documents.
#[cfg(any(test, feature = "test-support"))]
#[must_use]
pub fn store_error_value_fields(error: &runvil_vfs::VfsError) -> Vec<(String, ErrorField)> {
    let mut fields = vec![(
        "reason".to_owned(),
        ErrorField::String(error.tag().to_owned()),
    )];
    fields.extend(store_error_fields(error));
    fields
}

/// Returns the universal error a store failure becomes where it leaves the
/// VFS: the code named after the variant, the variant's fields as facts under
/// the names a Lua error table gives them, the failure's display text as the
/// message, and the code's default prose.
///
/// A store error value carries the `reason` tag, as `VfsError::tag` names
/// it, first, then the variant's own fields: the paths the failure names,
/// plus `anchor` and its integer `count` for an anchor error, `rule` for an
/// invalid path, or the integer `valid_up_to` for text that is not UTF-8.
/// The rule is the tag `PathReason::tag` returns.
///
/// Both store paths, the direct closures a Pack prelude's install calls
/// and the executor's effect path, build their failure here, so a store
/// error value has the same `kind`, `code`, `reason`, and fields on both;
/// only the direct closures put the store's message ahead of the default
/// prose.
#[must_use]
pub fn make_store_error(error: &runvil_vfs::VfsError) -> Error {
    let code = store_error_code(error);
    let mut converted = Error::new(code, error.to_string(), code.prose());
    for (name, field) in store_error_fields(error) {
        let Some(&(_, key)) = STORE_FACTS.iter().find(|(known, _)| *known == name) else {
            continue;
        };
        let value = match field {
            ErrorField::String(text) => Json::from(text),
            ErrorField::Integer(number) => Json::from(number),
        };
        converted.facts.insert(key, value);
    }
    converted
}

/// Returns the fields a store failure's universal error renders into its
/// Lua error table, the inverse of [`make_store_error`]: the code's tag as
/// `reason`, then each store fact under the field name
/// [`make_store_error`] documents for it, a string fact as a string and a
/// count or an offset as an integer.
fn store_fact_fields(error: &Error) -> Vec<(String, ErrorField)> {
    let mut fields = vec![(
        "reason".to_owned(),
        ErrorField::String(error.code.tag().to_owned()),
    )];
    for (name, key) in STORE_FACTS {
        let field = match error.facts.get(&key) {
            Some(Json::String(text)) => ErrorField::String(text.clone()),
            Some(value) => match value.as_i64() {
                Some(number) => ErrorField::Integer(number),
                None => continue,
            },
            None => continue,
        };
        fields.push((name.to_owned(), field));
    }
    fields
}

/// Returns the store facts a store failure's Lua error table `fields`
/// hold, the inverse of [`store_fact_fields`]: each store field under its
/// fact, a string field as a string and an integer field as a number.
/// `reason` is the code's tag, not a fact.
fn store_field_facts(fields: &BTreeMap<String, ErrorField>) -> BTreeMap<ErrorFact, Json> {
    STORE_FACTS
        .iter()
        .filter_map(|&(name, key)| {
            let value = match fields.get(name)? {
                ErrorField::String(text) => Json::from(text.as_str()),
                ErrorField::Integer(number) => Json::from(*number),
            };
            Some((key, value))
        })
        .collect()
}

/// The code named after the variant.
fn store_error_code(error: &runvil_vfs::VfsError) -> ErrorCode {
    use runvil_vfs::VfsError;
    match error {
        VfsError::NotFound { .. } => ErrorCode::NotFound,
        VfsError::AlreadyExists { .. } => ErrorCode::AlreadyExists,
        VfsError::NotADirectory { .. } => ErrorCode::NotADirectory,
        VfsError::IsADirectory { .. } => ErrorCode::IsADirectory,
        VfsError::DirectoryNotEmpty { .. } => ErrorCode::DirectoryNotEmpty,
        VfsError::NotUtf8 { .. } => ErrorCode::NotUtf8,
        VfsError::InvalidPath { .. } => ErrorCode::InvalidPath,
        VfsError::InvalidRange { .. } => ErrorCode::InvalidRange,
        VfsError::Anchor { .. } => ErrorCode::Anchor,
        VfsError::PermissionDenied { .. } => ErrorCode::PermissionDenied,
        VfsError::ApprovalRequired { .. } => ErrorCode::ApprovalRequired,
        VfsError::ScopeEnded { .. } => ErrorCode::ScopeEnded,
        VfsError::ReadOnly { .. } => ErrorCode::ReadOnly,
        VfsError::OutsideRoot { .. } => ErrorCode::OutsideRoot,
        VfsError::RootProtected { .. } => ErrorCode::RootProtected,
        VfsError::NoStore => ErrorCode::NoStore,
        VfsError::CrossMount { .. } => ErrorCode::CrossMount,
        VfsError::Io { .. } => ErrorCode::Io,
        VfsError::Unsupported { .. } => ErrorCode::Unsupported,
        VfsError::Conflict { .. } => ErrorCode::Conflict,
        // `VfsError` is `#[non_exhaustive]`: a variant added later is a
        // backend failure until it is named here.
        _ => ErrorCode::Backend,
    }
}

/// Returns the VM's shared error metatable, creating it on first use.
///
/// `__tostring` returns the table's `message`, so an author's `tostring`
/// (and mlua's own stringification when the table surfaces as a coroutine
/// failure) renders exactly the message text; `__concat` renders the same
/// way, so `'prefix: ' .. err` keeps working where the error was a string.
fn metatable(lua: &Lua) -> mlua::Result<Table> {
    if let Value::Table(existing) = lua.named_registry_value::<Value>(METATABLE_REGISTRY)? {
        return Ok(existing);
    }
    let metatable = lua.create_table()?;
    let tostring = lua.create_function(|_lua, table: Table| table.raw_get::<Value>("message"))?;
    metatable.raw_set("__tostring", tostring)?;
    let concat = lua
        .load("local tostring = tostring; return function(a, b) return tostring(a) .. tostring(b) end")
        .eval::<Function>()?;
    metatable.raw_set("__concat", concat)?;
    lua.set_named_registry_value(METATABLE_REGISTRY, &metatable)?;
    Ok(metatable)
}

/// Returns the VM's table from each error table rendered from a universal
/// error to that error, creating it on first use. Its keys are weak, so a
/// retained error lives exactly as long as its table.
fn retained(lua: &Lua) -> mlua::Result<Table> {
    if let Value::Table(existing) = lua.named_registry_value::<Value>(RETAINED_REGISTRY)? {
        return Ok(existing);
    }
    let retained = lua.create_table()?;
    let weak = lua.create_table()?;
    weak.raw_set("__mode", "k")?;
    retained.set_metatable(Some(weak))?;
    lua.set_named_registry_value(RETAINED_REGISTRY, &retained)?;
    Ok(retained)
}

/// Sets `kind` and `code` on `fields`, fills a missing `message` from the
/// kind, and attaches the shared metatable.
fn finish_table(lua: &Lua, kind: ErrorKind, code: ErrorCode, fields: Table) -> mlua::Result<Table> {
    fields.raw_set("kind", kind.tag())?;
    fields.raw_set("code", code.tag())?;
    if matches!(fields.raw_get::<Value>("message")?, Value::Nil) {
        fields.raw_set("message", kind.tag())?;
    }
    fields.set_metatable(Some(metatable(lua)?))?;
    Ok(fields)
}

/// Renders a universal error as the Lua error table: the `kind`
/// [`error_kind`] names, the error's code, its `message`, and the
/// [`error_fields`] beside them, under the shared metatable. The `message`
/// is the prose, after `{file}:{line}: ` when the error is located and not
/// internal, since an internal failure's location is in Rust. The table
/// keeps `error` beside it, for [`retained_error`].
///
/// # Errors
/// Returns an `mlua` error if the table cannot be created on `lua`.
pub(crate) fn error_table(lua: &Lua, error: &Error) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    let message = match &error.at {
        Some(at) if error.code != ErrorCode::Internal => {
            format!("{}: {}", coordinates(at), error.prose)
        }
        _ => error.prose.clone(),
    };
    table.raw_set("message", message)?;
    for (name, value) in error_fields(error) {
        let value = match value {
            ErrorField::String(value) => Value::String(lua.create_string(value)?),
            ErrorField::Integer(value) => Value::Integer(value),
        };
        table.raw_set(name, value)?;
    }
    let table = finish_table(lua, error_kind(error), error.code, table)?;
    retained(lua)?.raw_set(table.clone(), lua.create_any_userdata(error.clone())?)?;
    Ok(table)
}

/// Reads back the universal error a raised value was rendered from, when
/// it is an error table [`error_table`] built on this VM. Any other value
/// is `None`.
///
/// # Errors
/// Returns an `mlua` error if the retained error cannot be read.
pub(crate) fn retained_error(lua: &Lua, value: &Value) -> mlua::Result<Option<Error>> {
    let Value::Table(table) = value else {
        return Ok(None);
    };
    match retained(lua)?.raw_get::<Option<AnyUserData>>(table.clone())? {
        Some(error) => Ok(Some(error.borrow::<Error>()?.clone())),
        None => Ok(None),
    }
}

/// Builds the table the shim's `error_value(kind, { message = message })`
/// builds: `message` as it is, under the code `kind` gives.
///
/// # Errors
/// Returns an `mlua` error if the table cannot be created on `lua`.
pub(crate) fn message_table(lua: &Lua, kind: ErrorKind, message: &str) -> mlua::Result<Table> {
    let fields = lua.create_table()?;
    fields.raw_set("message", message)?;
    let code = table_code(kind, &fields);
    finish_table(lua, kind, code, fields)
}

/// Builds the `error_value(kind, fields)` chunk capture for the shim: the
/// Lua-side constructor of the same table shape, so a shim raise and a
/// Rust-raised error are indistinguishable to author code. The table's
/// code is the one its fields name, or else the one its kind gives. An
/// unknown kind is a shim bug and fails the call.
///
/// # Errors
/// Returns an `mlua` error if the function cannot be created.
pub(crate) fn install_error_value(lua: &Lua) -> mlua::Result<Function> {
    lua.create_function(|lua, (tag, fields): (String, Option<Table>)| {
        let kind = tag.parse::<ErrorKind>().map_err(|error| {
            mlua::Error::external(crate::error::make_internal_error(format!(
                "the shim raised {error}"
            )))
        })?;
        let fields = match fields {
            Some(fields) => fields,
            None => lua.create_table()?,
        };
        let code = table_code(kind, &fields);
        finish_table(lua, kind, code, fields)
    })
}

/// A Rust callback's failure as the universal error its table renders.
///
/// mlua raises a callback's `Err` into Lua as an opaque userdata whose
/// `tostring` is the error's display; author code cannot index it, so
/// `err.kind` fails at exactly the call sites that fail directly from
/// Rust. The classifier reads the universal error back out of the mlua
/// wrapper when the callback raised one, and otherwise sorts mlua's own
/// variants: an authoring or argument failure (a runtime or syntax error,
/// a bad argument, a value conversion, an external error of another type,
/// an exhausted memory quota) is `LuaRuntime`, with the root error's text
/// as its message and the author prose of what failed; anything else is
/// mlua's own machinery failing, which the author cannot act on, so it is
/// `Internal`, at its Rust line. Any other failure is placed on the line of
/// the failing call, which the traceback mlua took as the callback failed
/// names; when that names no author frame, on the line of the innermost
/// author frame running, or else on the call line. A universal failure the
/// prompt's Lua caused gets its author prose.
fn classified(lua: &Lua, error: &mlua::Error) -> Error {
    let (failure, what) = match error.downcast_ref::<Error>() {
        Some(typed) => (typed.clone(), typed.message.clone()),
        None => match root_cause(error) {
            root @ (mlua::Error::RuntimeError(_)
            | mlua::Error::SyntaxError { .. }
            | mlua::Error::MemoryError(_)
            | mlua::Error::BadArgument { .. }
            | mlua::Error::FromLuaConversionError { .. }
            | mlua::Error::SerializeError(_)
            | mlua::Error::DeserializeError(_)
            | mlua::Error::ExternalError(_)) => {
                (make_lua_runtime_error(root.to_string()), root_text(error))
            }
            root => return make_internal_error(root.to_string()),
        },
    };
    let line = traceback_line(error)
        .or_else(|| author_line(lua))
        .or_else(|| call_line(lua));
    authored_at(failure, &prompt_file(lua), line, failure_text(&what))
}

/// The prompt line of the innermost author frame in the traceback mlua
/// took as a callback in `error` failed: the innermost callback's that
/// names one. Read before [`root_cause`] drops the tracebacks.
fn traceback_line(error: &mlua::Error) -> Option<NonZeroU32> {
    match error {
        mlua::Error::CallbackError { traceback, cause } => {
            traceback_line(cause).or_else(|| first_prompt_line(traceback))
        }
        mlua::Error::WithContext { cause, .. } => traceback_line(cause),
        _ => None,
    }
}

/// Returns what `normalize_failure` returns for `value`: a Rust callback's
/// failure as the error table its classification names, anything else
/// unchanged, for the capture and the loop's compactor rule alike.
pub(crate) fn normalized(lua: &Lua, value: Value) -> mlua::Result<Value> {
    match value {
        Value::Error(error) => Ok(Value::Table(error_table(lua, &classified(lua, &error))?)),
        other => Ok(other),
    }
}

/// Whether `table` is an error table built on this VM, recognized by the
/// shared metatable, so an author's own table with a `kind` field is not
/// mistaken for one.
fn is_error_table(lua: &Lua, table: &Table) -> mlua::Result<bool> {
    let Some(attached) = table.metatable() else {
        return Ok(false);
    };
    Ok(attached.to_pointer() == metatable(lua)?.to_pointer())
}

/// The code a table of `kind` stands for when it names none, where
/// `field` reads one of its string fields: a `canceled` table with a
/// `task` field that parses is that task's cancellation and without one
/// a cancel; a `make_task_not_owned_error` or `make_task_consumed_error` table is its
/// task refusal only with such a field; a `make_context_exhausted_error` table is
/// context exhaustion only with a `reason`; a `store` table is the store
/// code its `reason` tag names, or `Backend`; and a kind whose failure
/// needs structure the table does not hold is `LuaRuntime`.
fn kind_code(kind: ErrorKind, field: impl Fn(&str) -> Option<String>) -> ErrorCode {
    let task = || field("task").is_some_and(|task| task.parse::<TaskId>().is_ok());
    match kind {
        ErrorKind::ToolLoopExhausted => ErrorCode::ToolLoopExhausted,
        ErrorKind::ContextExhausted if field("reason").is_some() => ErrorCode::ContextExhausted,
        ErrorKind::EmptyModelReply => ErrorCode::EmptyReply,
        ErrorKind::Canceled if task() => ErrorCode::TaskCanceled,
        ErrorKind::Canceled => ErrorCode::Canceled,
        ErrorKind::Tool => ErrorCode::ToolOther,
        ErrorKind::TaskNotOwned if task() => ErrorCode::TaskNotOwned,
        ErrorKind::TaskConsumed if task() => ErrorCode::TaskConsumed,
        ErrorKind::Store => {
            let store = ErrorCode::NotFound.number()..=ErrorCode::Backend.number();
            field("reason")
                .and_then(|reason| ErrorCode::from_tag(&reason))
                .filter(|code| store.contains(&code.number()))
                .unwrap_or(ErrorCode::Backend)
        }
        ErrorKind::ContextExhausted
        | ErrorKind::TaskNotOwned
        | ErrorKind::TaskConsumed
        | ErrorKind::OutOfScopeTool
        | ErrorKind::UnboundTool
        | ErrorKind::ModelOnlyTool
        | ErrorKind::TasksLive
        | ErrorKind::Lua
        | ErrorKind::ModelFailure
        | ErrorKind::ModelRequired
        | ErrorKind::Internal => ErrorCode::LuaRuntime,
    }
}

/// The code an error table of `kind` holding `fields` names: its `code`
/// field's, when that is a code's tag, and else the one its kind gives. A
/// field that cannot be read as a string names nothing.
fn table_code(kind: ErrorKind, fields: &Table) -> ErrorCode {
    let string = |name: &str| match fields.raw_get::<Value>(name) {
        Ok(Value::String(text)) => text.to_str().ok().map(|text| text.to_owned()),
        _ => None,
    };
    string("code")
        .and_then(|tag| ErrorCode::from_tag(&tag))
        .unwrap_or_else(|| kind_code(kind, string))
}

/// Reads a raised Lua value back as the universal error it stands for,
/// when it is an error table built on this VM. The code is the one
/// [`table_code`] names; the message and the prose are both the table's
/// `message`; and the facts are its fields under the fact names the
/// table's fields hold for that code. Any other value is `None`.
///
/// # Errors
/// Returns an `mlua` error if the table's fields cannot be read.
pub(crate) fn raised_from(lua: &Lua, value: &Value) -> mlua::Result<Option<Error>> {
    let Value::Table(table) = value else {
        return Ok(None);
    };
    if !is_error_table(lua, table)? {
        return Ok(None);
    }
    let Value::String(tag) = table.raw_get::<Value>("kind")? else {
        return Ok(None);
    };
    let Some(kind) = ErrorKind::from_tag(&tag.to_str()?) else {
        return Ok(None);
    };
    let message = match table.raw_get::<Value>("message")? {
        Value::String(message) => message.to_str()?.to_owned(),
        _ => kind.tag().to_owned(),
    };
    // `pairs` order is unspecified, but `fields` is a `BTreeMap`, so the
    // facts never depend on it.
    let mut fields = BTreeMap::new();
    for pair in table.pairs::<String, Value>() {
        let (name, value) = pair?;
        if matches!(name.as_str(), "kind" | "code" | "message") {
            continue;
        }
        let field = match value {
            Value::String(value) => ErrorField::String(value.to_str()?.to_owned()),
            Value::Integer(value) => ErrorField::Integer(value),
            _ => continue,
        };
        fields.insert(name, field);
    }
    let code = table_code(kind, table);
    let mut error = Error::new(code, message.clone(), message);
    error.facts = field_facts(code, &fields);
    Ok(Some(error))
}

#[cfg(test)]
#[path = "error_value-tests.rs"]
mod tests;
