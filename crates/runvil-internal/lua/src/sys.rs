//! The sealed `sys` table that sandboxed author code reads.

use std::borrow::Cow;

use mlua::{Lua, LuaSerdeExt, Value};
use serde_json::Value as Json;

use crate::error::{Error, Result};
use crate::proxy::{key_text, read_only_proxy};

/// Builds a sealed Lua `sys` table from runtime metadata.
///
/// A [`read_only_proxy`] over the JSON object: a read of an absent field
/// raises, and a present `null` reads as nil.
pub(crate) fn seal_sys(lua: &Lua, sys: &Json) -> Result<mlua::Table> {
    let data = match sys {
        Json::Object(map) => map.clone(),
        other => {
            return Err(Error::Internal(Cow::Owned(format!(
                "sys must be a table, got {other}"
            ))));
        }
    };

    let index = lua
        .create_function(move |lua, (_table, key): (Value, Value)| {
            let Value::String(name) = key else {
                return Err(mlua::Error::runtime(
                    "sys fields must be accessed by string key".to_owned(),
                ));
            };
            let field = name.to_string_lossy();
            match data.get(field.as_str()) {
                None => Err(mlua::Error::runtime(format!("unknown sys field '{field}'"))),
                Some(Json::Null) => Ok(Value::Nil),
                Some(value) => lua.to_value(value),
            }
        })
        .map_err(Error::lua)?;

    read_only_proxy(
        lua,
        Value::Function(index),
        |key| format!("sys is read-only; cannot set '{}'", key_text(key)),
        "sys is sealed",
    )
    .map_err(Error::lua)
}
