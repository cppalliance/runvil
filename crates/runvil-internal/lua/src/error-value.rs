//! The structured error value every failure takes when it reaches Lua.
//!
//! A failure that crosses into author code - a shim's own argument error,
//! a Rust-raised error answered through the `(ok, result)` envelope, a
//! Engine function's own failure (`tools.offer`, `models.get`, a `sys` or `var`
//! guard) caught by `pcall`, or a shim raise such as
//! `tool_loop_exhausted` - is one Lua table `{ kind, code, message, ... }`
//! under a shared metatable whose `__tostring` returns `message`. The
//! message is the failure's prose, the text an author may hand a model,
//! which a table rendered from a universal error placed on a prompt line
//! puts after that line's `file:line`; a caller that branches reads
//! `kind`, `code`, the stable tag of the failure's [`ErrorCode`], and the
//! kind's own fields (`reason` for `context_exhausted`, `finish_reason`
//! for `empty_model_reply`).
//!
//! [`ErrorKind`] names the closed vocabulary of kinds; [`error_kind`] and
//! [`error_fields`] render a universal error into the shape;
//! [`install_normalize_failure`] is the capture the shim's `pcall` and
//! `xpcall` replacements run a caught value through (so a Rust callback's
//! failure, which mlua raises as an opaque userdata, takes the same shape);
//! and [`raised_from`] reads a table back into a universal error when a
//! raise surfaces as a block coroutine's failure, so the failure survives
//! the boundary in both directions. A table rendered from a universal
//! error keeps that error beside it, which [`retained_error`] reads back
//! whole.

#[path = "error-value-classify.rs"]
mod classify;
#[path = "error-value-raised.rs"]
mod raised;
#[path = "error-value-store.rs"]
mod store;
#[path = "error-value-universal.rs"]
mod universal;

use std::str::FromStr;

use mlua::{AnyUserData, Function, Lua, Table, Value};

use runvil_types::error::{Error, ErrorCode, FactKey, coordinates};

use crate::error::accepted_tags;

pub(crate) use classify::{install_normalize_failure, normalized};
pub(crate) use raised::raised_from;
pub use store::{store_error, store_error_value_fields};
pub use universal::{error_fields, error_kind};

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
    let code = raised::table_code(kind, &fields);
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
            mlua::Error::external(crate::error::internal(format!("the shim raised {error}")))
        })?;
        let fields = match fields {
            Some(fields) => fields,
            None => lua.create_table()?,
        };
        let code = raised::table_code(kind, &fields);
        finish_table(lua, kind, code, fields)
    })
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

#[cfg(test)]
#[path = "error-value-tests.rs"]
mod tests;
