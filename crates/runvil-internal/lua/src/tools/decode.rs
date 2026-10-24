//! Shared value decoding for the `tools.*` Engine functions.
//!
//! The id-or-tool-object polymorphism of `tools.call` lives here once: a
//! string holding a `/` is a tool id and a tool object stands for its id.
//! The short-form `parameters` conversion of `tools.adhoc` sits beside it
//! as the namespace's other argument decode.

use mlua::Value;
use serde_json::{Value as Json, json};

use super::userdata::LuaToolHandle;

/// Resolves the tool a script's `tools.call` names to its id.
///
/// A string holding a `/` is the id verbatim, a tool object contributes
/// its id, and anything else is an argument error naming the accepted
/// forms.
///
/// # Errors
/// Returns an `mlua` external error when the value is neither a string
/// holding a `/` nor a tool object.
pub(crate) fn tool_name(value: &Value) -> mlua::Result<String> {
    match value {
        Value::String(text) => {
            let name = text.to_string_lossy();
            if name.contains('/') {
                Ok(name)
            } else {
                Err(mlua::Error::external(format!(
                    "tools.call: {name:?} is not a tool id"
                )))
            }
        }
        other => object_id(other).ok_or_else(|| {
            mlua::Error::external(format!(
                "tools.call takes a tool id or a tool object, got {}",
                other.type_name()
            ))
        }),
    }
}

/// Resolves the tool a model-issued call names: the wire name the model
/// sent, verbatim.
///
/// # Errors
/// Returns an `mlua` external error when the value is not a string.
pub(crate) fn model_tool_name(value: &Value) -> mlua::Result<String> {
    match value {
        Value::String(text) => Ok(text.to_string_lossy()),
        other => Err(mlua::Error::external(format!(
            "a model-issued tool call names a tool by its wire name, got {}",
            other.type_name()
        ))),
    }
}

/// The id of the tool object `value` is, or `None` for any other value,
/// a userdata that is not a tool object included.
pub(super) fn object_id(value: &Value) -> Option<String> {
    match value {
        Value::UserData(object) => object
            .borrow::<LuaToolHandle>()
            .ok()
            .map(|handle| handle.id().to_string()),
        _ => None,
    }
}

/// Builds the JSON Schema `parameters` object from the short form of a
/// `tools.adhoc` `parameters` table. Each value is a bare type string or a
/// `{type, description}` array; every declared parameter is required.
///
/// The `required` list is the only Rust-side `table.pairs` walk in this crate
/// that builds an ordered array without sorting it, so it sorts its names
/// itself. The other ordered walks, `ordered_keys` in `iteration.rs` and
/// `collection_members` in `collection.rs`, already sort their output. The
/// walk's order is unspecified, so a schema left in it would read differently
/// in two VMs; `properties` needs no sort because it is a `serde_json::Map`,
/// a `BTreeMap` with no `preserve_order` in the graph.
pub(super) fn adhoc_params_schema(params: &mlua::Table) -> mlua::Result<Json> {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();
    for pair in params.pairs::<String, Value>() {
        let (name, spec) = pair?;
        let (ty, description) = match spec {
            Value::String(s) => (s.to_string_lossy(), None),
            Value::Table(t) => (t.get::<String>(1)?, t.get::<Option<String>>(2)?),
            _ => {
                return Err(mlua::Error::external(format!(
                    "tools.adhoc: param {name:?} must be a type string or a {{type, description}} array"
                )));
            }
        };
        if !matches!(ty.as_str(), "string" | "integer" | "number" | "boolean") {
            return Err(mlua::Error::external(format!(
                "tools.adhoc: param {name:?} has unsupported type {ty:?}: \
                 expected \"string\", \"integer\", \"number\", or \"boolean\""
            )));
        }
        let mut property = json!({ "type": ty });
        if let Some(description) = description {
            property["description"] = Json::String(description);
        }
        properties.insert(name.clone(), property);
        required.push(name);
    }
    // Bytewise sort, matching the shared `SortKey::Text` order, so the
    // generated schema is the same text in every process.
    required.sort();
    Ok(json!({
        "type": "object",
        "properties": properties,
        "required": required,
    }))
}
