//! Parses the `inputSchema` frontmatter key: the JSON Schema of a prompt's
//! input.
//!
//! The schema advertises, documents, and derives the tool schema; it does
//! not enforce - enforcement belongs to the prompt's H1. Parsing checks the
//! static shape alone: the value converts to JSON, its `type` is `object`,
//! a present `properties` maps each name to a schema, and a present
//! `required` lists distinct names that `properties` declares. An absent
//! or `null` key declares no
//! schema, and the input text then arrives wrapped as
//! `argv = { prose = "<text>" }`.

use serde::Deserialize;
use serde_json::{Map, Number, Value};
use serde_yaml_ng::Value as Yaml;

/// A prompt's input schema: a JSON Schema object whose `type` is `object`.
pub(crate) type InputSchema = Map<String, Value>;

/// Deserializes the `inputSchema` key: `null` to `None`, and a mapping to
/// the checked schema.
pub(crate) fn deserialize<'de, D>(deserializer: D) -> Result<Option<InputSchema>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let Some(yaml) = Option::<Yaml>::deserialize(deserializer)? else {
        return Ok(None);
    };
    to_json(yaml, "inputSchema")
        .and_then(check)
        .map(Some)
        .map_err(serde::de::Error::custom)
}

/// Converts YAML to JSON, refusing what JSON cannot hold: tags, non-string
/// keys, and non-finite numbers. `path` names the value in errors.
fn to_json(yaml: Yaml, path: &str) -> Result<Value, String> {
    Ok(match yaml {
        Yaml::Null => Value::Null,
        Yaml::Bool(flag) => Value::Bool(flag),
        Yaml::String(text) => Value::String(text),
        Yaml::Number(number) => {
            if let Some(int) = number.as_i64() {
                Value::from(int)
            } else if let Some(int) = number.as_u64() {
                Value::from(int)
            } else {
                number
                    .as_f64()
                    .and_then(Number::from_f64)
                    .map(Value::Number)
                    .ok_or_else(|| format!("{path} is {number}; a JSON number must be finite"))?
            }
        }
        Yaml::Sequence(items) => Value::Array(
            items
                .into_iter()
                .enumerate()
                .map(|(index, item)| to_json(item, &format!("{path}[{index}]")))
                .collect::<Result<_, _>>()?,
        ),
        Yaml::Mapping(entries) => {
            let mut object = Map::new();
            for (key, value) in entries {
                let Yaml::String(key) = key else {
                    return Err(format!(
                        "{path} has a key of type {}; a JSON object key must be a string, \
                         so quote it",
                        yaml_type(&key)
                    ));
                };
                let value = to_json(value, &format!("{path}.{key}"))?;
                object.insert(key, value);
            }
            Value::Object(object)
        }
        Yaml::Tagged(tagged) => {
            return Err(format!(
                "{path} has the YAML tag {}; JSON has no tags",
                tagged.tag
            ));
        }
    })
}

/// Checks the static shape of a JSON Schema whose `type` is `object`.
fn check(value: Value) -> Result<InputSchema, String> {
    let Value::Object(schema) = value else {
        return Err(format!(
            "inputSchema must be null or a JSON Schema mapping with type: object, got {}",
            json_type(&value)
        ));
    };
    match schema.get("type") {
        Some(Value::String(kind)) if kind == "object" => {}
        Some(other) => {
            return Err(format!("inputSchema.type must be \"object\", got {other}"));
        }
        None => return Err("inputSchema.type must be \"object\", got none".to_owned()),
    }
    let properties = match schema.get("properties") {
        None => None,
        Some(Value::Object(properties)) => Some(properties),
        Some(other) => {
            return Err(format!(
                "inputSchema.properties must be a mapping, got {}",
                json_type(other)
            ));
        }
    };
    if let Some(properties) = properties {
        for (name, property) in properties {
            if !matches!(property, Value::Object(_) | Value::Bool(_)) {
                return Err(format!(
                    "inputSchema.properties.{name} must be a schema mapping or boolean, got {}",
                    json_type(property)
                ));
            }
        }
    }
    if let Some(required) = schema.get("required") {
        let Value::Array(names) = required else {
            return Err(format!(
                "inputSchema.required must be an array of strings, got {}",
                json_type(required)
            ));
        };
        for (index, name) in names.iter().enumerate() {
            let Value::String(name) = name else {
                return Err(format!(
                    "inputSchema.required[{index}] must be a string, got {}",
                    json_type(name)
                ));
            };
            if names[..index].iter().any(|seen| seen == name) {
                return Err(format!(
                    "inputSchema.required lists \"{name}\" more than once"
                ));
            }
            if !properties.is_some_and(|properties| properties.contains_key(name)) {
                return Err(format!(
                    "inputSchema.required lists \"{name}\", which inputSchema.properties \
                     does not declare"
                ));
            }
        }
    }
    Ok(schema)
}

/// Names the JSON type of `value`.
fn json_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Names the YAML type of `value`.
fn yaml_type(value: &Yaml) -> &'static str {
    match value {
        Yaml::Null => "null",
        Yaml::Bool(_) => "boolean",
        Yaml::Number(_) => "number",
        Yaml::String(_) => "string",
        Yaml::Sequence(_) => "sequence",
        Yaml::Mapping(_) => "mapping",
        Yaml::Tagged(_) => "tagged value",
    }
}
