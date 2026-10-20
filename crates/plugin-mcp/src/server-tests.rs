//! Tests for the tool-name rule and the `ToolInfo` values `catalog` builds.

use rmcp::model::Tool;
use runvil_plugin::PackId;
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

fn ids(pack: &str, tools: Vec<Tool>) -> Vec<String> {
    let pack = PackId::parse(pack).expect("a Pack id parses");
    catalog(&pack, tools)
        .iter()
        .map(|listed| listed.info.id.to_string())
        .collect()
}

#[test]
fn a_tool_name_is_lowercased_under_the_pack_name() {
    assert_eq!(
        ids(
            "papers",
            vec![plain("Get_Paper"), plain("list-mailings"), plain("v1.2")]
        ),
        ["papers/get_paper", "papers/list-mailings", "papers/v1.2"]
    );
}

#[test]
fn the_original_name_is_kept_for_the_call() {
    let pack = PackId::parse("papers").expect("parses");
    let listed = catalog(&pack, vec![plain("Get_Paper")]);
    assert_eq!(listed[0].mcp_name, "Get_Paper");
}

#[test]
fn a_name_that_is_still_invalid_after_lowercasing_is_dropped() {
    assert_eq!(
        ids(
            "papers",
            vec![
                plain("has space"),
                plain("keep"),
                plain("sym@bol"),
                plain(""),
                plain("café")
            ]
        ),
        ["papers/keep"]
    );
}

#[test]
fn two_names_that_collide_after_lowercasing_keep_the_first() {
    let pack = PackId::parse("papers").expect("parses");
    let listed = catalog(
        &pack,
        vec![plain("Search"), plain("other"), plain("SEARCH")],
    );
    let kept: Vec<_> = listed.iter().map(|l| l.mcp_name.as_str()).collect();
    assert_eq!(kept, ["Search", "other"]);
}

#[test]
fn the_description_is_the_description_else_the_title_else_the_name() {
    let pack = PackId::parse("papers").expect("parses");
    let listed = catalog(
        &pack,
        vec![
            tool("a", &json!({ "description": "does a", "title": "A" })),
            tool("b", &json!({ "title": "Title B" })),
            plain("c"),
        ],
    );
    let described: Vec<_> = listed.iter().map(|l| l.info.description.as_str()).collect();
    assert_eq!(described, ["does a", "Title B", "c"]);
}

#[test]
fn the_parameters_are_the_input_schema_and_the_tool_is_plain_and_stoppable() {
    let pack = PackId::parse("papers").expect("parses");
    let schema = json!({ "type": "object", "properties": { "id": { "type": "string" } }, "required": ["id"] });
    let listed = catalog(&pack, vec![tool("get", &json!({ "inputSchema": schema }))]);
    let info = &listed[0].info;
    assert_eq!(info.parameters_schema, schema);
    assert!(!info.structured_output);
    assert!(!info.survives_stop);
}
