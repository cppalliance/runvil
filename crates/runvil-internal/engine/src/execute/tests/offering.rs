//! Tests for offering catalog tools: the run catalog every section entry
//! reads, which `tools.offer` offers by id, Pack, or object, a model call
//! reaches by wire name in its section's frozen sets, and a script call
//! reaches by id.

use runvil_parser::test_support::{PromptSource, one_section, prompt};

use super::fixtures::echo_tool;
use super::models_loop::catalog_scenario;
use super::*;
use crate::execute::run::{EffectRecord, ToolCallOrigin, ToolCaller};
use crate::test_support::rig::Rig;
use crate::test_support::scenario::EXECUTION;
use runvil_types::packs::PackId;

/// A catalog tool under any id, described by its id, echoing its `value`
/// argument as `<id>: <value>`, with parameters `schema`.
#[track_caller]
fn tool_at(id: &str, schema: Value) -> FnTool {
    let label = id.to_owned();
    let tool = FnTool::new(id, schema, move |args| {
        let value = args["value"]
            .as_str()
            .ok_or_else(|| ToolError::message("the value argument is required"))?;
        Ok(ToolOutput::trusted(format!("{label}: {value}")))
    });
    tool.description(id)
}

/// A catalog tool under `id` taking a string `value`.
#[track_caller]
fn offered_tool(id: &str) -> FnTool {
    tool_at(
        id,
        json!({ "type": "object", "properties": { "value": { "type": "string" } } }),
    )
}

#[test]
fn the_run_catalog_keeps_its_order_and_a_pack_offer_expands_in_id_order() {
    let gateway = ScriptedChat::new(vec![text("done")]);
    let source = prompt(
        "uses: [tools]",
        &[(
            "Only",
            "tools.offer('gh')\n\
             tools.offer('alpha')\n\
             messages:user('go')\n\
             models.loop()\n\
             return tools.get('alpha/fetch').description",
        )],
    );
    let mut rig = Rig::start(
        catalog_scenario(source)
            .tool(echo_tool())
            .tool(offered_tool("gh/search"))
            .tool(offered_tool("alpha/zed"))
            .tool(offered_tool("gh/issues.list"))
            .tool(offered_tool("alpha/fetch"))
            .chat(&gateway),
    );
    let set = rig.scheduler_for_test().tool_set_for_test();
    let ids: Vec<String> = set
        .catalog()
        .tools()
        .iter()
        .map(|info| info.id.to_string())
        .collect();
    assert_eq!(
        ids,
        [
            "tools/echo",
            "gh/search",
            "alpha/zed",
            "gh/issues.list",
            "alpha/fetch"
        ],
        "the run catalog keeps the order its tools were supplied in"
    );
    assert_eq!(
        set.declared(),
        [PackId::parse("tools").expect("valid Pack")],
        "the set records the declared Packs"
    );
    let rig = rig.finish();
    assert_eq!(
        rig.trace().text(),
        "alpha/fetch",
        "a tool object holds the catalog's own text"
    );
    let (advertised, _) = rounds_and_calls(rig.issued());
    let names = ["gh_issues_list", "gh_search", "alpha_fetch", "alpha_zed"];
    assert_eq!(
        advertised,
        vec![names.map(str::to_owned).to_vec()],
        "each Pack's tools sit together in id order, the Packs in offer order"
    );
}

#[test]
fn a_script_offers_the_extras_and_the_model_calls_an_offered_tool_by_its_wire_name() {
    let gateway = ScriptedChat::new(vec![
        calls(&[("call_1", "tools_echo", r#"{"value":"hi"}"#)]),
        text("done"),
    ]);
    let source = one_section(
        "tools.offer(tools.packs.extras())\n\
         messages:user('go')\n\
         models.loop()\n\
         return 'ok'",
    );
    let rig = Rig::run(catalog_scenario(source).tool(echo_tool()).chat(&gateway));
    assert_eq!(rig.trace().text(), "ok");
    let records = rig.issued();
    let advertised: Vec<Vec<String>> = records
        .iter()
        .filter_map(|record| match record {
            EffectRecord::Chat { tools, .. } => Some(tools.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(advertised, vec![vec!["tools_echo".to_owned()]; 2]);
    let calls: Vec<&EffectRecord> = records
        .iter()
        .filter(|record| matches!(record, EffectRecord::ToolCall { .. }))
        .collect();
    assert_eq!(
        calls,
        vec![&EffectRecord::ToolCall {
            tool: ToolId::parse("tools/echo").expect("valid id"),
            alias: "tools_echo".to_owned(),
            args: json!({ "value": "hi" }),
            origin: ToolCallOrigin::new(EXECUTION, "Only", ToolCaller::Model),
        }]
    );
}

/// Every key on a section's `tools` table, sorted, each with its Lua type.
const TOOLS_TABLE_KEYS: &str =
    "adhoc:function,call:function,get:function,offer:function,offer_dynamic:function,packs:table";

#[test]
fn a_section_tools_table_holds_only_its_api_and_its_round_advertises_only_the_offered_tool() {
    let gateway = ScriptedChat::new(vec![text("done")]);
    let source = one_section(
        "local keys = {}\n\
         for key, value in pairs(tools) do keys[#keys + 1] = key .. ':' .. type(value) end\n\
         table.sort(keys)\n\
         tools.offer(tools.packs.extras())\n\
         messages:user('go')\n\
         models.loop()\n\
         return table.concat(keys, ',')",
    );
    let rig = Rig::run(catalog_scenario(source).tool(echo_tool()).chat(&gateway));
    assert_eq!(
        rig.trace().text(),
        TOOLS_TABLE_KEYS,
        "a section's tools table holds exactly the tools API"
    );
    let advertised: Vec<Vec<String>> = rig
        .issued()
        .iter()
        .filter_map(|record| match record {
            EffectRecord::Chat { tools, .. } => Some(tools.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        advertised,
        vec![vec!["tools_echo".to_owned()]],
        "the round advertises exactly the offered tool"
    );
}

#[test]
fn a_script_calls_an_offered_tool_by_its_object_and_by_its_id_without_offering_it() {
    let source = one_section(
        "local tool = tools.packs.get('tools').tools[1]\n\
         return tools.call(tool, { value = 'a' }) .. '|' .. \
           tools.call('tools/echo', { value = 'b' }) .. '|' .. tool.id",
    );
    let rig = Rig::run(catalog_scenario(source).tool(echo_tool()));
    assert_eq!(rig.trace().text(), "echoed: a|echoed: b|tools/echo");
    let call = |value: &str| EffectRecord::ToolCall {
        tool: ToolId::parse("tools/echo").expect("valid id"),
        alias: "tools/echo".to_owned(),
        args: json!({ "value": value }),
        origin: ToolCallOrigin::new(EXECUTION, "Only", ToolCaller::Script),
    };
    assert_eq!(rig.issued(), vec![call("a"), call("b")]);
}

/// The tool names each chat round advertised, and the tool calls issued,
/// in record order.
fn rounds_and_calls(records: &[EffectRecord]) -> (Vec<Vec<String>>, Vec<EffectRecord>) {
    let advertised = records
        .iter()
        .filter_map(|record| match record {
            EffectRecord::Chat { tools, .. } => Some(tools.clone()),
            _ => None,
        })
        .collect();
    let calls = records
        .iter()
        .filter(|record| matches!(record, EffectRecord::ToolCall { .. }))
        .cloned()
        .collect();
    (advertised, calls)
}

/// A model call of `id` under `alias` with `value`, from the one section.
fn model_call(id: &str, alias: &str, value: &str) -> EffectRecord {
    EffectRecord::ToolCall {
        tool: ToolId::parse(id).expect("valid id"),
        alias: alias.to_owned(),
        args: json!({ "value": value }),
        origin: ToolCallOrigin::new(EXECUTION, "Only", ToolCaller::Model),
    }
}

#[test]
fn a_script_offers_a_declared_packs_tools_and_the_model_calls_one_by_its_wire_name() {
    let gateway = ScriptedChat::new(vec![
        calls(&[("call_1", "tools_echo", r#"{"value":"hi"}"#)]),
        text("done"),
    ]);
    let source = prompt(
        "uses: [tools]",
        &[(
            "Only",
            "tools.offer(tools.packs.get('tools').tools)\n\
             messages:user('go')\n\
             models.loop()\n\
             return 'ok'",
        )],
    );
    let rig = Rig::run(
        catalog_scenario(source)
            .tool(echo_tool())
            .tool(offered_tool("gh/search"))
            .chat(&gateway),
    );
    assert_eq!(rig.trace().text(), "ok");
    let (advertised, calls) = rounds_and_calls(rig.issued());
    assert_eq!(
        advertised,
        vec![vec!["tools_echo".to_owned()]; 2],
        "only the declared Pack's tools are offered"
    );
    assert_eq!(calls, vec![model_call("tools/echo", "tools_echo", "hi")]);
}

#[test]
fn a_script_offers_an_extra_packs_tools_after_checking_packs_get() {
    let gateway = ScriptedChat::new(vec![
        calls(&[("call_1", "gh_search", r#"{"value":"q"}"#)]),
        text("done"),
    ]);
    let source = one_section(
        "assert(tools.packs.get('missing') == nil, 'an unknown Pack reads nil')\n\
         local gh = tools.packs.get('gh')\n\
         if gh then tools.offer(gh.tools) end\n\
         messages:user('go')\n\
         models.loop()\n\
         return 'ok'",
    );
    let rig = Rig::run(
        catalog_scenario(source)
            .tool(echo_tool())
            .tool(offered_tool("gh/search"))
            .chat(&gateway),
    );
    assert_eq!(rig.trace().text(), "ok");
    let (advertised, calls) = rounds_and_calls(rig.issued());
    assert_eq!(
        advertised,
        vec![vec!["gh_search".to_owned()]; 2],
        "the extra Pack's tools are offered and nothing else"
    );
    assert_eq!(calls, vec![model_call("gh/search", "gh_search", "q")]);
}

#[test]
fn the_pack_objects_and_the_adhoc_pack_are_installed_in_the_h1_vm() {
    let source = PromptSource::new()
        .front("uses: [tools]")
        .h1(
            "return tostring(tools.packs.get('tools').required) .. '|' .. \
             tools.packs.extras()[1].name .. '|' .. \
             tostring(tools.get('gh/search').pack == tools.packs.get('gh')) .. '|' .. \
             #tools.packs.get('adhoc').tools",
        )
        .build();
    let rig = Rig::run(
        catalog_scenario(source)
            .tool(echo_tool())
            .tool(offered_tool("gh/search")),
    );
    assert_eq!(
        rig.trace().text(),
        "true|gh|true|0",
        "the H1 pass reads the Pack objects, the empty adhoc Pack included"
    );
}

#[path = "offering-packs.rs"]
mod packs;

#[path = "offering-sets.rs"]
mod sets;
