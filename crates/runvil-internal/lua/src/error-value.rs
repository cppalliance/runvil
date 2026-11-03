//! The structured error value every failure takes when it reaches Lua.
//!
//! A failure that crosses into author code - a shim's own argument error,
//! a Rust-raised error answered through the `(ok, result)` envelope, a
//! Engine function's own failure (`tools.offer`, `models.get`, a `sys` or `var`
//! guard) caught by `pcall`, or a shim raise such as
//! `tool_loop_exhausted` - is one Lua table `{ kind, message, ... }` under
//! a shared metatable whose `__tostring` returns `message`. A `pcall`
//! caller that prints the error sees exactly the text it saw before; a
//! caller that branches reads `kind` and the kind's own fields (`reason`
//! for `context_exhausted`, `finish_reason` for `empty_model_reply`).
//!
//! [`ErrorKind`] names the closed vocabulary of kinds; [`error_kind`],
//! [`error_table_message`], and [`error_fields`] render a universal error
//! into the shape, and [`Error::to_raised`] this crate's own;
//! [`install_normalize_failure`] is the capture the shim's `pcall` and
//! `xpcall` replacements run a caught value through (so a Rust callback's
//! failure, which mlua raises as an opaque userdata, takes the same shape),
//! and [`Raised`] is the table read back into Rust when a shim raise
//! surfaces as a block coroutine's failure, so the kind survives the
//! boundary in both directions.

#[path = "error-value-classify.rs"]
mod classify;
#[path = "error-value-store.rs"]
mod store;
#[path = "error-value-universal.rs"]
mod universal;

use std::collections::BTreeMap;
use std::str::FromStr;

use mlua::{Function, Lua, Table, Value};

use runvil_types::error::{ErrorCode, FactKey};

use crate::error::{Error, accepted_tags};

pub(crate) use classify::{install_normalize_failure, normalized};
pub use store::{store_error, store_error_value_fields};
pub use universal::{error_fields, error_kind, error_table_message};

/// The registry key of the shared error metatable, created on first use per
/// VM so every error table on that VM - Lua-built or Rust-built - shares
/// the same identity and the read-back can recognize it.
const METATABLE_REGISTRY: &str = "runvil.error_value.metatable";

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
    /// The model named a tool outside the section's advertised scope.
    OutOfScopeTool,
    /// A script `tools.call` named an alias with no binding in the run.
    UnboundTool,
    /// A script `tools.call` named a catalog tool that only a model may
    /// call, because a render shapes its arguments for the model.
    ModelOnlyTool,
    /// A dispatched tool's own failure.
    Tool,
    /// A task operation named a task the caller does not own.
    TaskNotOwned,
    /// A task's result was already delivered once.
    TaskConsumed,
    /// A section ended while tasks it owns were still live.
    TasksLive,
    /// The run was cancelled, the caller dropped a call the run was waiting
    /// on, or a wait delivered a task its owner cancelled (that value
    /// includes `task`).
    Cancelled,
    /// A store operation's own failure; includes `reason` and the
    /// [`VfsError`](runvil_vfs::VfsError) variant's fields.
    Store,
    /// A Lua authoring or runtime failure: a compile error, a runtime error
    /// in author code, a shim's argument error, or an exhausted Engine quota.
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
        ErrorKind::Cancelled,
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
            ErrorKind::Cancelled => "cancelled",
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
            "cancelled" => Some(ErrorKind::Cancelled),
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
    type Err = runvil_types::error::Error;

    /// Parses a kind from its exact tag, as `ErrorKind::from_tag` does. Other
    /// text is an [`InvalidValue`](ErrorCode::InvalidValue) error that names
    /// it and lists every tag.
    fn from_str(tag: &str) -> Result<ErrorKind, Self::Err> {
        ErrorKind::from_tag(tag).ok_or_else(|| {
            let text = format!(
                "invalid error kind `{tag}`: required one of {}",
                accepted_tags(ErrorKind::ALL.iter().map(|kind| kind.tag()))
            );
            let mut error =
                runvil_types::error::Error::new(ErrorCode::InvalidValue, text.clone(), text);
            error
                .facts
                .insert(FactKey::Name, serde_json::Value::from(tag));
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

    /// The field's integer value, when it is one.
    #[must_use]
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            ErrorField::String(_) => None,
            ErrorField::Integer(value) => Some(*value),
        }
    }
}

impl Error {
    /// This crate's own error as the error table it renders into, for a
    /// failure that reaches Lua without leaving the crate: the kind an
    /// author branches on, the display text as the message, and the kind's
    /// own fields. A raised table renders as it was read back.
    pub(crate) fn to_raised(&self) -> Raised {
        let (kind, fields) = match self {
            Error::Lua(_)
            | Error::LuaRuntime { .. }
            | Error::LuaCompile { .. }
            | Error::LuaQuota { .. } => (ErrorKind::Lua, BTreeMap::new()),
            Error::ContextExhausted { reason } => (
                ErrorKind::ContextExhausted,
                BTreeMap::from([("reason".to_owned(), ErrorField::String(reason.clone()))]),
            ),
            Error::Interrupted => (ErrorKind::Cancelled, BTreeMap::new()),
            Error::Tool { .. } => (ErrorKind::Tool, BTreeMap::new()),
            Error::Store { cause, .. } => (
                ErrorKind::Store,
                store_error_value_fields(cause).into_iter().collect(),
            ),
            Error::Internal { .. } => (ErrorKind::Internal, BTreeMap::new()),
            Error::Raised(raised) => return raised.clone(),
        };
        Raised {
            kind,
            message: self.to_string(),
            fields,
        }
    }
}

/// A structured error table read back into Rust: the kind, the message
/// `tostring` rendered, and the kind's fields.
///
/// This is the shape a block coroutine's failure takes when the raised
/// value was an error table (built by the shim's `raise` or by
/// `error_table`) and no retained typed error was substituted for it -
/// the case for a Lua-side raise. [`into_universal`](crate::into_universal)
/// maps it onto the failure its kind names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Raised {
    /// The kind the table named.
    pub kind: ErrorKind,
    /// The message `tostring` renders.
    pub message: String,
    /// The kind's own fields (`reason`, `finish_reason`, ...), string- or
    /// integer-valued (a store anchor's `count`, a UTF-8 failure's
    /// `valid_up_to`).
    pub fields: BTreeMap<String, ErrorField>,
}

impl Raised {
    /// The reason a `context_exhausted` table held, when it holds a
    /// string one.
    #[must_use]
    pub fn exhaustion_reason(&self) -> Option<&str> {
        self.fields.get("reason").and_then(ErrorField::as_str)
    }
}

impl std::fmt::Display for Raised {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for Raised {}

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

/// Sets `kind` on `fields`, fills a missing `message` from the kind, and
/// attaches the shared metatable.
fn finish_table(lua: &Lua, kind: ErrorKind, fields: Table) -> mlua::Result<Table> {
    fields.raw_set("kind", kind.tag())?;
    if matches!(fields.raw_get::<Value>("message")?, Value::Nil) {
        fields.raw_set("message", kind.tag())?;
    }
    fields.set_metatable(Some(metatable(lua)?))?;
    Ok(fields)
}

/// Builds the error table of `kind` holding `message` and `fields`, under
/// the shared metatable.
fn table_of(
    lua: &Lua,
    kind: ErrorKind,
    message: &str,
    fields: impl IntoIterator<Item = (String, ErrorField)>,
) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    table.raw_set("message", message)?;
    for (name, value) in fields {
        let value = match value {
            ErrorField::String(value) => Value::String(lua.create_string(value)?),
            ErrorField::Integer(value) => Value::Integer(value),
        };
        table.raw_set(name, value)?;
    }
    finish_table(lua, kind, table)
}

/// Renders a universal error as the Lua error table: the `kind`
/// [`error_kind`] names, the `message` [`error_table_message`] gives, and
/// the [`error_fields`] beside them, under the shared metatable.
///
/// # Errors
/// Returns an `mlua` error if the table cannot be created on `lua`.
pub(crate) fn error_table(lua: &Lua, error: &runvil_types::error::Error) -> mlua::Result<Table> {
    table_of(
        lua,
        error_kind(error),
        &error_table_message(error),
        error_fields(error),
    )
}

/// Renders `raised` as the Lua error table: its kind, its message, and its
/// fields, under the shared metatable. This crate's own error renders
/// through it, by [`Error::to_raised`].
///
/// # Errors
/// Returns an `mlua` error if the table cannot be created on `lua`.
pub(crate) fn raised_table(lua: &Lua, raised: &Raised) -> mlua::Result<Table> {
    table_of(lua, raised.kind, &raised.message, raised.fields.clone())
}

/// Builds the `error_value(kind, fields)` chunk capture for the shim: the
/// Lua-side constructor of the same table shape, so a shim raise and a
/// Rust-raised error are indistinguishable to author code. An unknown kind
/// is a shim bug and fails the call.
///
/// # Errors
/// Returns an `mlua` error if the function cannot be created.
pub(crate) fn install_error_value(lua: &Lua) -> mlua::Result<Function> {
    lua.create_function(|lua, (tag, fields): (String, Option<Table>)| {
        let kind = tag.parse::<ErrorKind>().map_err(|error| {
            mlua::Error::external(Error::internal(format!("the shim raised {error}")))
        })?;
        let fields = match fields {
            Some(fields) => fields,
            None => lua.create_table()?,
        };
        finish_table(lua, kind, fields)
    })
}

/// Reads a raised Lua value back as a [`Raised`] when it is an error table
/// built on this VM (recognized by the shared metatable, so an author's own
/// table with a `kind` field is not mistaken for one). Any other value is
/// `None`.
///
/// # Errors
/// Returns an `mlua` error if the table's fields cannot be read.
pub(crate) fn raised_from(lua: &Lua, value: &Value) -> mlua::Result<Option<Raised>> {
    let Value::Table(table) = value else {
        return Ok(None);
    };
    let Some(attached) = table.metatable() else {
        return Ok(None);
    };
    if attached.to_pointer() != metatable(lua)?.to_pointer() {
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
    // raised value's field order never depends on it.
    let mut fields = BTreeMap::new();
    for pair in table.pairs::<String, Value>() {
        let (name, value) = pair?;
        if name == "kind" || name == "message" {
            continue;
        }
        let field = match value {
            Value::String(value) => ErrorField::String(value.to_str()?.to_owned()),
            Value::Integer(value) => ErrorField::Integer(value),
            _ => continue,
        };
        fields.insert(name, field);
    }
    Ok(Some(Raised {
        kind,
        message,
        fields,
    }))
}

#[cfg(test)]
#[path = "error-value-tests.rs"]
mod tests;
