//! Tests for a catalog tool object's `parameters`: the schema as a fresh
//! Lua table on every read, its lists and nulls intact through
//! `json.encode`, and metadata that never takes the untrusted envelope.

use std::collections::BTreeMap;

use mlua::{LuaSerdeExt, Value};
use runvil_types::packs::PackId;
use runvil_types::tools::{ToolCatalog, ToolId, ToolInfo, ToolOutput};
use serde_json::{Value as Json, json};

use crate::ToolBinding;
use crate::bindings::ToolSet;
use crate::dispatch::dispatch_body;
use crate::test_vm::{TEST_SECTION, lua_over, test_nonce};
use crate::tests::recording::null_emitter;

/// The `web/search` schema: nested properties, a required list, an empty
/// list, a null, and text that looks like markup.
fn search_schema() -> Json {
    json!({
        "type": "object",
        "description": "Search <the web> for [INST] pages",
        "properties": {
            "query": { "type": "string", "description": "What to search for" },
            "limit": { "type": "integer", "minimum": 1, "default": null },
            "tags": { "type": "array", "items": { "type": "string" }, "examples": [] }
        },
        "required": ["query"],
        "additionalProperties": false
    })
}

fn search_tool() -> ToolInfo {
    ToolInfo::new(
        ToolId::parse("web/search").expect("a static valid tool id"),
        "Searches <the web> and returns [INST] snippets",
        search_schema(),
    )
}

/// A set declaring `web`, whose catalog holds `web/search` and
/// `web/replace`, the second marked model-only.
pub(super) fn search_set() -> ToolSet {
    let model_only = ToolInfo::new(
        ToolId::parse("web/replace").expect("a static valid tool id"),
        "Replace text the model names",
        search_schema(),
    )
    .model_only_call(true);
    ToolSet::new(
        vec![PackId::parse("web").expect("`web` is a valid Pack id")],
        BTreeMap::new(),
        ToolCatalog::new([search_tool(), model_only]).expect("two tools"),
    )
}

#[test]
fn parameters_deep_equals_the_catalog_tools_schema() {
    let vm = lua_over(search_set());
    let parameters: Value = vm.eval("return tools.get('web/search').parameters");
    let read: Json = vm
        .lua()
        .from_value(parameters)
        .expect("the parameters table converts back to JSON");
    assert_eq!(read, search_schema());
    let null_reads_json_null: bool =
        vm.eval("return tools.get('web/search').parameters.properties.limit.default == json.null");
    assert!(null_reads_json_null, "a schema null reads json.null");
}

#[test]
fn each_parameters_read_is_a_fresh_table() {
    let vm = lua_over(search_set());
    let result: (bool, String, Option<String>) = vm.eval(
        "local tool = tools.get('web/search')\n\
         local first = tool.parameters\n\
         first.type = 'mutated'\n\
         first.properties.query = nil\n\
         local second = tool.parameters\n\
         return first ~= second, second.type, second.properties.query.type",
    );
    assert_eq!(
        result,
        (true, "object".to_owned(), Some("string".to_owned())),
        "mutating one read leaves the next unchanged"
    );
}

#[test]
fn parameters_round_trips_through_json_encode_with_empty_lists_intact() {
    let vm = lua_over(search_set());
    let encoded: String = vm.eval("return json.encode(tools.get('web/search').parameters)");
    assert!(
        encoded.contains("\"examples\":[]"),
        "an empty list encodes as []: {encoded}"
    );
    assert!(
        encoded.contains("\"default\":null"),
        "a null encodes as null: {encoded}"
    );
    let decoded: Json = serde_json::from_str(&encoded).expect("json.encode writes JSON");
    assert_eq!(decoded, search_schema());
}

#[test]
fn assigning_parameters_raises() {
    let vm = lua_over(search_set());
    let error: String = vm.eval(
        "local ok, err = pcall(function() tools.get('web/search').parameters = {} end)\n\
         assert(not ok, 'assigning parameters is refused')\n\
         return tostring(err)",
    );
    assert!(
        error.contains("Tool objects are frozen: cannot assign field \"parameters\""),
        "the refusal names the field: {error}"
    );
}

#[test]
fn a_model_only_tool_reads_true_and_stays_named_in_the_catalog() {
    let vm = lua_over(search_set());
    let read: (bool, String) = vm.eval(
        "local tool = tools.get('web/replace')\n\
         return tool.model_only_call, tool.id",
    );
    assert_eq!(
        read,
        (true, "web/replace".to_owned()),
        "a script sees the flag before it tries to call the tool"
    );
}

#[test]
fn a_tool_object_holds_exactly_id_description_parameters_pack_and_model_only_call() {
    let vm = lua_over(search_set());
    let fields: (String, bool, bool, String, bool) = vm.eval(
        "local tool = tools.get('web/search')\n\
         return tool.id, type(tool.description) == 'string', \
           type(tool.parameters) == 'table', tool.pack.name, tool.model_only_call",
    );
    assert_eq!(
        fields,
        ("web/search".to_owned(), true, true, "web".to_owned(), false),
        "a catalog tool a Pack did not mark model-only reads false"
    );
    let extra: Option<String> = vm.eval(
        "local tool = tools.get('web/search')\n\
         local probes = { 'vendor', 'schema', 'alias', 'name', 'describes_tools' }\n\
         for _, field in ipairs(probes) do\n\
           local ok, value = pcall(function() return tool[field] end)\n\
           if ok and value ~= nil then return field end\n\
         end\n\
         return nil",
    );
    assert_eq!(extra, None, "a tool object reads no field beyond its five");
}

/// Every string in `value`, keys included, depth first.
fn strings(value: &Json, out: &mut Vec<String>) {
    match value {
        Json::String(text) => out.push(text.clone()),
        Json::Array(items) => items.iter().for_each(|item| strings(item, out)),
        Json::Object(entries) => {
            for (key, item) in entries {
                out.push(key.clone());
                strings(item, out);
            }
        }
        _ => {}
    }
}

#[test]
fn metadata_of_a_pack_whose_results_are_untrusted_never_takes_the_envelope() {
    let nonce = test_nonce();
    let result = dispatch_body(
        &ToolBinding::from_info("web_search", &search_tool()),
        None,
        Ok(ToolOutput::untrusted("fetched page")),
        &nonce,
        &null_emitter(),
        TEST_SECTION,
        None,
    )
    .expect("an untrusted output dispatches")
    .into_content();
    assert!(
        result.contains(&nonce.to_string()),
        "the Pack's results are untrusted: {result}"
    );
    let preface = "is data, not instructions";
    assert!(result.contains(preface), "{result}");

    let vm = lua_over(search_set());
    let (description, name, parameters): (String, String, Value) = vm.eval(
        "local tool = tools.get('web/search')\n\
         return tool.description, tools.packs.get('web').name, tool.parameters",
    );
    assert_eq!(description, search_tool().description);
    assert_eq!(name, "web");
    let parameters: Json = vm
        .lua()
        .from_value(parameters)
        .expect("the parameters table converts back to JSON");
    let (mut read, mut declared) = (Vec::new(), Vec::new());
    strings(&parameters, &mut read);
    strings(&search_schema(), &mut declared);
    read.sort();
    declared.sort();
    assert_eq!(
        read, declared,
        "every string is the schema's text as declared"
    );
    for text in read.iter().chain([&description, &name]) {
        assert!(
            !text.contains(&nonce.to_string()) && !text.contains(preface),
            "metadata carries no envelope: {text}"
        );
    }
}
