//! The classifier a Rust callback's failure runs through on its way to
//! author code, and the `normalize_failure` capture built on it.

use mlua::{Function, Lua, Value};
use runvil_types::error::{Error, ErrorCode};

use super::error_table;
use crate::error::{
    authored, authored_at, failure_text, lua_runtime, prompt_file, root_cause, root_text,
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
/// `Internal`. A universal failure the prompt's Lua caused gets its author
/// prose.
fn classified(lua: &Lua, error: &mlua::Error) -> Error {
    if let Some(typed) = error.downcast_ref::<Error>() {
        return authored(lua, typed.clone());
    }
    let root = root_cause(error);
    match root {
        mlua::Error::RuntimeError(_)
        | mlua::Error::SyntaxError { .. }
        | mlua::Error::MemoryError(_)
        | mlua::Error::BadArgument { .. }
        | mlua::Error::FromLuaConversionError { .. }
        | mlua::Error::SerializeError(_)
        | mlua::Error::DeserializeError(_)
        | mlua::Error::ExternalError(_) => {
            let what = root_text(error);
            authored_at(
                lua_runtime(root.to_string()),
                &prompt_file(lua),
                None,
                failure_text(&what),
            )
        }
        _ => Error::new(
            ErrorCode::Internal,
            root.to_string(),
            ErrorCode::Internal.prose(),
        ),
    }
}

/// Builds the `normalize_failure(value)` chunk capture for the shim's
/// `pcall` and `xpcall` replacements: a Rust callback's failure (mlua's
/// wrapped error) becomes the error table its classification names; any
/// other caught value - a string, an author's own table, an error table
/// already built - passes through unchanged.
///
/// # Errors
/// Returns an `mlua` error if the function cannot be created.
pub(crate) fn install_normalize_failure(lua: &Lua) -> mlua::Result<Function> {
    lua.create_function(normalized)
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
