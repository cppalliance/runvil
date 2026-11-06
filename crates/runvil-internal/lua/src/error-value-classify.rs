//! The classifier a Rust callback's failure runs through on its way to
//! author code, and the `normalize_failure` capture built on it.

use std::num::NonZeroU32;

use mlua::{Lua, Value};
use runvil_types::error::Error;

use super::error_table;
use crate::chunk_line::first_prompt_line;
use crate::error::{
    author_line, authored_at, call_line, failure_text, internal, lua_runtime, prompt_file,
    root_cause, root_text,
};

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
            | mlua::Error::ExternalError(_)) => (lua_runtime(root.to_string()), root_text(error)),
            root => return internal(root.to_string()),
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
