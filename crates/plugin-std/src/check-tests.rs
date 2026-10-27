//! Argument checking: a matching call passes, and each kind of mismatch
//! fails naming the parameter.

use runvil_plugin::ToolErrorKind;
use serde_json::json;

use super::check;

/// The schema the `StrReplace` tool ships, which has a required string, an
/// optional string, and an optional boolean.
fn replace() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "The path." },
            "old_string": { "type": "string", "description": "The old text." },
            "new_string": { "type": "string", "description": "The new text." },
            "replace_all": { "type": "boolean", "description": "Every occurrence." }
        },
        "required": ["path", "old_string", "new_string"]
    })
}

/// The refusal of `args` against `schema`, as the message and kind.
fn refused(schema: &serde_json::Value, args: &serde_json::Value) -> (String, ToolErrorKind) {
    let Err(error) = check("StrReplace", schema, args) else {
        panic!("{args} is refused");
    };
    (error.to_string(), error.kind())
}

#[test]
fn matching_arguments_pass() {
    let full = json!({
        "path": "/a.md",
        "old_string": "one",
        "new_string": "two",
        "replace_all": true
    });
    assert!(check("StrReplace", &replace(), &full).is_ok(), "{full}");
    let least = json!({ "path": "/a.md", "old_string": "one", "new_string": "two" });
    assert!(check("StrReplace", &replace(), &least).is_ok(), "{least}");
}

#[test]
fn a_null_argument_list_reads_as_an_empty_object() {
    let schema = json!({ "type": "object", "properties": {} });
    assert!(check("Read", &schema, &json!(null)).is_ok());
}

#[test]
fn a_missing_required_parameter_fails_naming_it() {
    let args = json!({ "path": "/a.md", "old_string": "one" });
    let (message, kind) = refused(&replace(), &args);
    assert_eq!(
        message,
        "std_StrReplace: `new_string` is required, and the call left it out"
    );
    assert_eq!(kind, ToolErrorKind::InvalidArguments);
}

#[test]
fn a_required_parameter_passed_as_null_fails_naming_it() {
    let args = json!({ "path": null, "old_string": "one", "new_string": "two" });
    let (message, kind) = refused(&replace(), &args);
    assert_eq!(
        message,
        "std_StrReplace: `path` is required, and the call passed null"
    );
    assert_eq!(kind, ToolErrorKind::InvalidArguments);
}

#[test]
fn a_parameter_of_the_wrong_type_fails_naming_it_and_the_value() {
    let args = json!({ "path": "/a.md", "old_string": "one", "new_string": 2 });
    let (message, kind) = refused(&replace(), &args);
    assert_eq!(
        message,
        "std_StrReplace: `new_string` must be a JSON string, and the call passed 2"
    );
    assert_eq!(kind, ToolErrorKind::InvalidArguments);
}

#[test]
fn arguments_that_are_not_an_object_fail() {
    let schema = json!({ "type": "object", "properties": {} });
    let (message, kind) = refused(&schema, &json!(["a"]));
    assert_eq!(
        message,
        "std_StrReplace: the arguments must be a JSON object"
    );
    assert_eq!(kind, ToolErrorKind::InvalidArguments);
}

#[test]
fn a_value_outside_an_enum_fails_naming_the_allowed_values() {
    let schema = json!({
        "type": "object",
        "properties": {
            "output_mode": { "type": "string", "enum": ["content", "count"] }
        }
    });
    let (message, kind) = refused(&schema, &json!({ "output_mode": "files" }));
    assert_eq!(
        message,
        "std_StrReplace: `output_mode` must be one of \"content\", \"count\", and the call passed \"files\""
    );
    assert_eq!(kind, ToolErrorKind::InvalidArguments);
}

#[test]
fn an_integer_schema_accepts_a_whole_number_and_refuses_a_fraction() {
    let schema = json!({
        "type": "object",
        "properties": { "limit": { "type": "integer" } }
    });
    assert!(check("Read", &schema, &json!({ "limit": 3 })).is_ok());
    assert!(check("Read", &schema, &json!({ "limit": 3.5 })).is_err());
}

#[test]
fn an_array_schema_checks_every_element() {
    let schema = json!({
        "type": "object",
        "properties": {
            "paths": { "type": "array", "items": { "type": "string" } }
        }
    });
    assert!(check("Read", &schema, &json!({ "paths": ["/a", "/b"] })).is_ok());
    assert!(check("Read", &schema, &json!({ "paths": ["/a", 2] })).is_err());
}

#[test]
fn a_parameter_the_schema_does_not_declare_passes_so_the_tool_can_see_it() {
    let args = json!({ "path": "/a.md", "old_string": "one", "new_string": "two", "x": 1 });
    assert!(
        check("StrReplace", &replace(), &args).is_ok(),
        "extra arguments reach the tool, which refuses them itself"
    );
}
