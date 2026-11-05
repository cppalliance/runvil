//! A raised error table read back as the universal error it stands for:
//! its code, its message as both message and prose, and the facts its
//! fields hold.

use std::collections::BTreeMap;

use mlua::{Lua, Table, Value};
use runvil_types::error::{Error, ErrorCode};
use runvil_types::ids::TaskId;

use super::universal::field_facts;
use super::{ErrorField, ErrorKind, is_error_table};

/// The code a table of `kind` stands for when it names none, where
/// `field` reads one of its string fields: a `canceled` table with a
/// `task` field that parses is that task's cancellation and without one
/// a cancel; a `task_not_owned` or `task_consumed` table is its
/// task refusal only with such a field; a `context_exhausted` table is
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
pub(super) fn table_code(kind: ErrorKind, fields: &Table) -> ErrorCode {
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
#[path = "error-value-raised-tests.rs"]
mod tests;
