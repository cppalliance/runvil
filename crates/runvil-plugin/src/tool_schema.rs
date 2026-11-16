//! Typed tool schemas and the JSON boundary shared by their handlers.

use schemars::{JsonSchema, generate::SchemaSettings};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::{Error, ErrorCode, OutputTrust, ToolOutput, make_tool_error};

pub(crate) fn parameters<Args: JsonSchema>() -> Result<Value, Error> {
    let mut schema = SchemaSettings::draft2020_12()
        .with(|settings| settings.inline_subschemas = true)
        .into_generator()
        .into_root_schema_for::<Args>();
    if schema.get("type").and_then(Value::as_str) != Some("object") {
        let message = "tool arguments must have an object schema; use a named-field Rust struct";
        return Err(Error::new(ErrorCode::InvalidValue, message, message));
    }
    schema.remove("$schema");
    schema.remove("title");
    Ok(schema.to_value())
}

pub(crate) fn decode<Args: DeserializeOwned>(args: Value) -> Result<Args, Error> {
    if !args.is_object() {
        return Err(make_tool_error(
            ErrorCode::ToolInvalidArguments,
            "invalid arguments: expected an object",
        ));
    }
    serde_json::from_value(args).map_err(|error| {
        make_tool_error(
            ErrorCode::ToolInvalidArguments,
            format!("invalid arguments: {error}"),
        )
    })
}

pub(crate) fn encode(output: impl Serialize, trust: OutputTrust) -> Result<ToolOutput, Error> {
    let text = serde_json::to_value(output)
        .map_err(|error| {
            make_tool_error(
                ErrorCode::ToolOther,
                "the tool output could not be rendered as JSON",
            )
            .with_cause(&error)
        })?
        .to_string();
    Ok(match trust {
        OutputTrust::Trusted => ToolOutput::trusted(text),
        _ => ToolOutput::untrusted(text),
    })
}
