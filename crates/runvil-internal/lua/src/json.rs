//! The `json` Engine global: a plain table, like `string`, holding
//! `json.null`, `json.array`, `json.decode`, `json.decode_lines`,
//! `json.encode`, and `json.encode_lines`.
//!
//! `json.null` is mlua's null sentinel, a light userdata with a null
//! pointer, and every decoded JSON null is that value. A marked list is a
//! table whose metatable is mlua's array metatable: every path that
//! converts Lua to JSON through mlua's `from_value` encodes it as a JSON
//! array, so `json.array()` sends `[]` where a plain empty table sends `{}`.
//! Null and list detection rely on the values themselves, never on fields
//! of the `json` table, so reassigning a field cannot break them.

mod decode;
mod encode;
mod keys;

pub(crate) use encode::to_json;

use mlua::{Lua, LuaSerdeExt, Table, Value};

use crate::error::{Error, Result};

/// Installs the `json` table as a global.
///
/// # Errors
/// Returns [`Error::LuaRuntime`] if the table, its functions, or the
/// global cannot be created.
pub(crate) fn install(lua: &Lua) -> Result<()> {
    let json = lua.create_table().map_err(Error::lua)?;
    json.raw_set("null", lua.null()).map_err(Error::lua)?;
    json.raw_set("array", lua.create_function(array).map_err(Error::lua)?)
        .map_err(Error::lua)?;
    json.raw_set(
        "decode",
        lua.create_function(decode::decode).map_err(Error::lua)?,
    )
    .map_err(Error::lua)?;
    json.raw_set(
        "decode_lines",
        lua.create_function(decode::decode_lines)
            .map_err(Error::lua)?,
    )
    .map_err(Error::lua)?;
    json.raw_set(
        "encode",
        lua.create_function(encode::encode).map_err(Error::lua)?,
    )
    .map_err(Error::lua)?;
    json.raw_set(
        "encode_lines",
        lua.create_function(encode::encode_lines)
            .map_err(Error::lua)?,
    )
    .map_err(Error::lua)?;
    lua.globals().raw_set("json", json).map_err(Error::lua)
}

/// `json.array(t)`: marks `t` as a list in place and returns it, or returns
/// a new empty marked list when called with no argument or nil. A table
/// already marked comes back unchanged; one with any other metatable is
/// refused, so a guarded or sealed table keeps its own.
fn array(lua: &Lua, value: Value) -> mlua::Result<Table> {
    let marker = lua.array_metatable();
    match value {
        Value::Nil => {
            let list = lua.create_table()?;
            list.set_metatable(Some(marker))?;
            Ok(list)
        }
        Value::Table(table) => match table.metatable() {
            None => {
                table.set_metatable(Some(marker))?;
                Ok(table)
            }
            Some(metatable) if metatable == marker => Ok(table),
            Some(_) => Err(refusal(
                "json.array: the table already has a metatable".to_owned(),
            )),
        },
        other => Err(refusal(format!(
            "json.array expects a table or no argument, got {}",
            other.type_name()
        ))),
    }
}

/// Raises `message` as an author error, which `pcall` reports as kind
/// `lua`.
fn refusal(message: String) -> mlua::Error {
    mlua::Error::external(Error::Lua(message))
}
