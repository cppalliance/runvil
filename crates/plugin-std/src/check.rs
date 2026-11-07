//! Argument checking against a tool's own JSON Schema, and the helpers
//! the file tools share to refuse what the schema alone cannot.

use runvil_plugin::{Error, ErrorCode, make_tool_error};
use serde_json::Value;

use crate::matcher::PatternError;
use crate::text::clip_to_boundary;

/// Checks that `args` matches the `schema`, which is the `object` schema a
/// tool's definition file holds.
///
/// Only the keywords these definitions use are honored: `type`,
/// `properties`, `required`, and `enum`, plus `items` for an array. A
/// schema this function does not understand is not a reason to refuse the
/// call, because the tool's own body decides what it can do with the value.
///
/// # Errors
/// Returns an invalid-arguments [`Error`] naming the parameter and
/// what it expected.
pub(crate) fn check(tool: &str, schema: &Value, args: &Value) -> Result<(), Error> {
    let args = if args.is_null() {
        &Value::Object(serde_json::Map::new())
    } else {
        args
    };
    let Some(object) = args.as_object() else {
        return Err(make_invalid_arguments_error(
            tool,
            "the arguments must be a JSON object",
        ));
    };
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for name in required.iter().filter_map(Value::as_str) {
            match object.get(name) {
                None => {
                    return Err(make_invalid_arguments_error(
                        tool,
                        format!("`{name}` is required, and the call left it out"),
                    ));
                }
                Some(Value::Null) => {
                    return Err(make_invalid_arguments_error(
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
fn check_value(tool: &str, name: &str, property: &Value, value: &Value) -> Result<(), Error> {
    if let Some(allowed) = property.get("enum").and_then(Value::as_array)
        && !allowed.contains(value)
    {
        let allowed: Vec<String> = allowed.iter().map(ToString::to_string).collect();
        return Err(make_invalid_arguments_error(
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
            return Err(make_invalid_arguments_error(
                tool,
                format!("`{name}` has a schema type {other} this build does not know"),
            ));
        }
    };
    if matches {
        return check_items(tool, name, property, value);
    }
    Err(make_invalid_arguments_error(
        tool,
        format!("`{name}` must be a JSON {expected}, and the call passed {value}"),
    ))
}

/// Checks every element of `value`, when it is an array with an `items`
/// schema.
fn check_items(tool: &str, name: &str, property: &Value, value: &Value) -> Result<(), Error> {
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

/// Refuses a key in `args` that the `schema`'s `properties` do not
/// declare.
///
/// [`check`] lets an undeclared key through so the tool can decide what to
/// do with it, and the file tools refuse it here, after `check` and before
/// any other work. The message names the smallest undeclared key, and
/// lists the declared parameters sorted bytewise, so the text does not
/// depend on the order the JSON holds them in. A schema with no
/// `properties`, and arguments that are not an object, refuse nothing.
///
/// # Errors
/// Returns an invalid-arguments [`Error`] naming the key.
pub(crate) fn refuse_unknown(tool: &str, schema: &Value, args: &Value) -> Result<(), Error> {
    let (Some(properties), Some(object)) = (
        schema.get("properties").and_then(Value::as_object),
        args.as_object(),
    ) else {
        return Ok(());
    };
    let Some(unknown) = object
        .keys()
        .filter(|key| !properties.contains_key(*key))
        .min()
    else {
        return Ok(());
    };
    let mut names: Vec<&str> = properties.keys().map(String::as_str).collect();
    names.sort_unstable();
    let quoted: Vec<String> = names.iter().map(|name| format!("`{name}`")).collect();
    let tail = match quoted.split_last() {
        None => "This tool takes no parameters.".to_owned(),
        Some((last, [])) => format!("The parameters are {last}."),
        Some((last, first)) => format!("The parameters are {} and {last}.", first.join(", ")),
    };
    Err(make_invalid_arguments_error(
        tool,
        format!("`{unknown}` is not a parameter of this tool. {tail}"),
    ))
}

/// Refuses `value` when it is empty or only whitespace, naming the
/// parameter `name` and echoing the value.
///
/// This runs before any I/O, so a blank path never reaches the
/// filesystem.
///
/// # Errors
/// Returns an invalid-arguments [`Error`] naming the parameter.
pub(crate) fn refuse_blank(tool: &str, name: &str, value: &str) -> Result<(), Error> {
    if value.trim().is_empty() {
        return Err(make_invalid_arguments_error(
            tool,
            format!("`{name}` is empty or only whitespace, and the call passed \"{value}\""),
        ));
    }
    Ok(())
}

/// The most bytes of a refused pattern the error echoes.
const PATTERN_ECHO_BYTES: usize = 80;

/// The invalid-arguments error for the pattern `pattern`, which the
/// matcher refused with `error`, passed under the parameter `name`.
///
/// The error echoes the pattern, cut to 80 bytes on a character boundary
/// with `...` added when it was cut, so a long pattern does not fill the
/// message.
pub(crate) fn make_invalid_pattern_error(
    tool: &str,
    name: &str,
    pattern: &str,
    error: PatternError,
) -> Error {
    let shown = clip_to_boundary(pattern, 0, PATTERN_ECHO_BYTES);
    let dots = if shown.len() < pattern.len() {
        "..."
    } else {
        ""
    };
    make_invalid_arguments_error(
        tool,
        format!(
            "`{name}` is invalid: \"{shown}{dots}\" does not parse: {}.",
            error.reason()
        ),
    )
}

/// `{n} {noun}` for a count, with `s` added to the noun for any `n` but 1.
///
/// A noun that ends in a consonant and then `y` takes `ies` instead, so
/// `directory` gives `directories`. Examples: `1 line`, `10 lines`,
/// `0 lines`.
pub(crate) fn plural(n: usize, noun: &str) -> String {
    if n == 1 {
        return format!("{n} {noun}");
    }
    if let Some(stem) = noun.strip_suffix('y')
        && let Some(before) = stem.chars().next_back()
        && before.is_alphabetic()
        && !matches!(before.to_ascii_lowercase(), 'a' | 'e' | 'i' | 'o' | 'u')
    {
        return format!("{n} {stem}ies");
    }
    format!("{n} {noun}s")
}

/// An invalid-arguments error whose message opens with the tool's id
/// segment.
pub(crate) fn make_invalid_arguments_error(tool: &str, reason: impl std::fmt::Display) -> Error {
    make_tool_error(
        ErrorCode::ToolInvalidArguments,
        format!("std_{tool}: {reason}"),
    )
}

#[cfg(test)]
#[path = "check-tests.rs"]
mod tests;
