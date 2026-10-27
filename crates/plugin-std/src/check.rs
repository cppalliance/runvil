//! Argument checking against a tool's own JSON Schema.

use runvil_plugin::{ToolError, ToolErrorKind};
use serde_json::Value;

/// Checks that `args` matches the `schema`, which is the `object` schema a
/// tool's definition file holds.
///
/// Only the keywords these definitions use are honored: `type`,
/// `properties`, `required`, and `enum`, plus `items` for an array. A
/// schema this function does not understand is not a reason to refuse the
/// call, because the tool's own body decides what it can do with the value.
///
/// # Errors
/// Returns an invalid-arguments [`ToolError`] naming the parameter and
/// what it expected.
pub(crate) fn check(tool: &str, schema: &Value, args: &Value) -> Result<(), ToolError> {
    let args = if args.is_null() {
        &Value::Object(serde_json::Map::new())
    } else {
        args
    };
    let Some(object) = args.as_object() else {
        return Err(refusal(tool, "the arguments must be a JSON object"));
    };
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for name in required.iter().filter_map(Value::as_str) {
            match object.get(name) {
                None => {
                    return Err(refusal(
                        tool,
                        format!("`{name}` is required, and the call left it out"),
                    ));
                }
                Some(Value::Null) => {
                    return Err(refusal(
                        tool,
                        format!("`{name}` is required, and the call passed null"),
                    ));
                }
                Some(_) => {}
            }
        }
    }
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return Ok(());
    };
    for (name, property) in properties {
        let Some(value) = object.get(name).filter(|value| !value.is_null()) else {
            continue;
        };
        check_value(tool, name, property, value)?;
    }
    Ok(())
}

/// Checks one parameter's `value` against its `property` schema.
fn check_value(tool: &str, name: &str, property: &Value, value: &Value) -> Result<(), ToolError> {
    if let Some(allowed) = property.get("enum").and_then(Value::as_array)
        && !allowed.contains(value)
    {
        let allowed: Vec<String> = allowed.iter().map(ToString::to_string).collect();
        return Err(refusal(
            tool,
            format!(
                "`{name}` must be one of {}, and the call passed {value}",
                allowed.join(", ")
            ),
        ));
    }
    let Some(expected) = property.get("type").and_then(Value::as_str) else {
        return Ok(());
    };
    let matches = match expected {
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.as_f64().is_some_and(|number| number.fract() == 0.0),
        "boolean" => value.is_boolean(),
        "array" => value.is_array(),
        "object" => value.is_object(),
        "null" => value.is_null(),
        other => {
            return Err(refusal(
                tool,
                format!("`{name}` has a schema type {other} this build does not know"),
            ));
        }
    };
    if matches {
        return check_items(tool, name, property, value);
    }
    Err(refusal(
        tool,
        format!("`{name}` must be a JSON {expected}, and the call passed {value}"),
    ))
}

/// Checks every element of `value`, when it is an array with an `items`
/// schema.
fn check_items(tool: &str, name: &str, property: &Value, value: &Value) -> Result<(), ToolError> {
    let (Some(items), Some(elements)) = (
        property.get("items"),
        value.as_array().filter(|_| property.get("items").is_some()),
    ) else {
        return Ok(());
    };
    for element in elements {
        check_value(tool, name, items, element)?;
    }
    Ok(())
}

/// An invalid-arguments error whose message opens with the tool's id
/// segment.
fn refusal(tool: &str, reason: impl std::fmt::Display) -> ToolError {
    ToolError::new(
        ToolErrorKind::InvalidArguments,
        format!("std_{tool}: {reason}"),
    )
}

#[cfg(test)]
#[path = "check-tests.rs"]
mod tests;
