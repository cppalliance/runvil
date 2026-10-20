//! Tests for the `Listed` values `catalog` keeps.

use rmcp::model::Tool;
use serde_json::{Value, json};

use super::catalog;

fn tool(name: &str, extra: &Value) -> Tool {
    let mut value = json!({ "name": name, "inputSchema": { "type": "object" } });
    for (key, member) in extra.as_object().expect("extra is an object") {
        value[key] = member.clone();
    }
    serde_json::from_value(value).expect("a tool parses")
}

fn plain(name: &str) -> Tool {
    tool(name, &json!({}))
}

#[test]
fn every_tool_is_kept_under_the_name_its_server_lists() {
    let names: Vec<String> = catalog(vec![
        plain("Get_Paper"),
        plain("has space"),
        plain("café"),
        plain("v1.2"),
    ])
    .into_iter()
    .map(|listed| listed.name)
    .collect();
    assert_eq!(names, ["Get_Paper", "has space", "café", "v1.2"]);
}

#[test]
fn the_description_is_the_description_else_the_title_else_the_name() {
    let listed = catalog(vec![
        tool("a", &json!({ "description": "does a", "title": "A" })),
        tool("b", &json!({ "title": "Title B" })),
        plain("c"),
    ]);
    let described: Vec<_> = listed.iter().map(|l| l.description.as_str()).collect();
    assert_eq!(described, ["does a", "Title B", "c"]);
}

#[test]
fn the_input_schema_is_kept_as_the_server_lists_it() {
    let schema = json!({ "type": "object", "properties": { "id": { "type": "string" } }, "required": ["id"] });
    let listed = catalog(vec![tool("get", &json!({ "inputSchema": schema }))]);
    assert_eq!(listed[0].input_schema, schema);
}
