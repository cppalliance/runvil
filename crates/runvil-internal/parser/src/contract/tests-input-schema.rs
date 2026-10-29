//! Tests for the `inputSchema` key: an absent or null key, the accepted
//! object schemas, and the refused shapes and values.

use super::*;

#[test]
fn an_absent_or_null_input_schema_declares_none() {
    for front in ["", "inputSchema: null", "inputSchema: ~"] {
        let prompt = parse_ok(&prompt(front, &[]));
        assert_eq!(prompt.frontmatter().input_schema(), None, "{front:?}");
    }
}

#[test]
fn an_object_schema_parses_into_its_json_value() {
    let prompt = parse_ok(&prompt(
        "inputSchema: { type: object, properties: { query: { type: string }, \
         limit: { type: integer, default: 3, minimum: 1 }, tags: { type: array, \
         items: { type: string } }, any: true }, required: [query], \
         additionalProperties: false }",
        &[],
    ));
    let schema = prompt.frontmatter().input_schema().expect("declared");
    assert_eq!(
        serde_json::Value::Object(schema.clone()),
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" },
                "limit": { "type": "integer", "default": 3, "minimum": 1 },
                "tags": { "type": "array", "items": { "type": "string" } },
                "any": true,
            },
            "required": ["query"],
            "additionalProperties": false,
        })
    );
}

#[test]
fn a_bare_object_schema_parses() {
    let prompt = parse_ok(&prompt("inputSchema: { type: object }", &[]));
    let schema = prompt.frontmatter().input_schema().expect("declared");
    assert_eq!(schema.len(), 1);
    assert_eq!(schema["type"], "object");
}

#[test]
fn a_property_may_take_a_reserved_name_because_input_fields_are_argv_fields() {
    let prompt = parse_ok(&prompt(
        "inputSchema: { type: object, properties: { prose: {}, store: {}, end: {} } }",
        &[],
    ));
    let schema = prompt.frontmatter().input_schema().expect("declared");
    assert_eq!(
        schema["properties"].as_object().map(serde_json::Map::len),
        Some(3)
    );
}

#[test]
fn an_input_schema_that_is_not_an_object_schema_is_refused() {
    for (front, message) in [
        (
            "inputSchema: [type, object]",
            "inputSchema must be null or a JSON Schema mapping with type: object, got array",
        ),
        (
            "inputSchema: object",
            "inputSchema must be null or a JSON Schema mapping with type: object, got string",
        ),
        (
            "inputSchema: { properties: {} }",
            "inputSchema.type must be \"object\", got none",
        ),
        (
            "inputSchema: { type: string }",
            "inputSchema.type must be \"object\", got \"string\"",
        ),
        (
            "inputSchema: { type: [object, 'null'] }",
            "inputSchema.type must be \"object\", got [\"object\",\"null\"]",
        ),
    ] {
        assert_parse_error(&prompt(front, &[]), ParseErrorKind::Frontmatter, message);
    }
}

#[test]
fn malformed_properties_and_required_are_refused() {
    for (front, message) in [
        (
            "inputSchema: { type: object, properties: [query] }",
            "inputSchema.properties must be a mapping, got array",
        ),
        (
            "inputSchema: { type: object, properties: { query: string } }",
            "inputSchema.properties.query must be a schema mapping or boolean, got string",
        ),
        (
            "inputSchema: { type: object, required: query }",
            "inputSchema.required must be an array of strings, got string",
        ),
        (
            "inputSchema: { type: object, properties: { query: {} }, required: [query, 1] }",
            "inputSchema.required[1] must be a string, got number",
        ),
        (
            "inputSchema: { type: object, properties: { query: {} }, required: [query, query] }",
            "inputSchema.required lists \"query\" more than once",
        ),
        (
            "inputSchema: { type: object, properties: { query: {} }, required: [query, limit] }",
            "inputSchema.required lists \"limit\", which inputSchema.properties does not declare",
        ),
        (
            "inputSchema: { type: object, required: [query] }",
            "inputSchema.required lists \"query\", which inputSchema.properties does not declare",
        ),
    ] {
        assert_parse_error(&prompt(front, &[]), ParseErrorKind::Frontmatter, message);
    }
}

#[test]
fn yaml_that_json_cannot_hold_is_refused() {
    for (front, message) in [
        (
            "inputSchema: { type: object, 1: x }",
            "inputSchema has a key of type number; a JSON object key must be a string",
        ),
        (
            "inputSchema: { type: object, properties: { n: { maximum: .inf } } }",
            "inputSchema.properties.n.maximum is .inf; a JSON number must be finite",
        ),
        (
            "inputSchema: { type: object, examples: [!thing x] }",
            "inputSchema.examples[0] has the YAML tag !thing; JSON has no tags",
        ),
    ] {
        assert_parse_error(&prompt(front, &[]), ParseErrorKind::Frontmatter, message);
    }
}

#[test]
fn the_retired_args_key_is_an_unknown_key() {
    let error = parse_err(&prompt("args: { query: { type: string } }", &[]));
    assert_eq!(error.kind(), ParseErrorKind::Frontmatter);
    assert!(error.to_string().contains("args"), "{error}");
}
