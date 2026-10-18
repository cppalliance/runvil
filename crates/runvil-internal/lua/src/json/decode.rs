//! `json.decode` and `json.decode_lines`: JSON text to Lua values.
//!
//! Both parse with `serde_json` into a tree on the Rust heap, outside the
//! Lua memory limit, then convert it with mlua's `to_value` under its
//! default options, which mark every list and turn every null into
//! `json.null`. A memory-limit failure while converting passes through
//! unchanged.

use mlua::{BorrowedBytes, Lua, LuaSerdeExt, Value};
use serde_json::Value as Json;

use super::refusal;

/// The most bytes either function takes.
const INPUT_LIMIT: usize = 8_388_608;

/// `json.decode(text)`: the one JSON document `text` holds, as Lua values.
pub(super) fn decode(lua: &Lua, text: Value) -> mlua::Result<Value> {
    let bytes = input(text, "json.decode")?;
    let document: Json =
        serde_json::from_slice(&bytes).map_err(|error| refusal(format!("json.decode: {error}")))?;
    lua.to_value(&document)
}

/// `json.decode_lines(text)`: every JSON value `text` holds, in order, as a
/// marked list. Any whitespace separates values, so several may share a
/// line and one may span lines, and an error's line and column count
/// across the whole text.
pub(super) fn decode_lines(lua: &Lua, text: Value) -> mlua::Result<Value> {
    let bytes = input(text, "json.decode_lines")?;
    let values = serde_json::Deserializer::from_slice(&bytes)
        .into_iter::<Json>()
        .collect::<Result<Vec<Json>, _>>()
        .map_err(|error| refusal(format!("json.decode_lines: {error}")))?;
    lua.to_value(&values)
}

/// The bytes of `text`, checked before any parsing: a string of at most
/// [`INPUT_LIMIT`] bytes.
fn input(text: Value, function: &str) -> mlua::Result<BorrowedBytes> {
    let text = match text {
        Value::String(text) => text,
        other => {
            return Err(refusal(format!(
                "{function} expects a string, got {}",
                other.type_name()
            )));
        }
    };
    let bytes = text.as_bytes();
    if bytes.len() > INPUT_LIMIT {
        return Err(refusal(format!(
            "{function}: input exceeds the {INPUT_LIMIT}-byte limit"
        )));
    }
    Ok(bytes)
}
