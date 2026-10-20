//! Tests for the offering: every catalog tool the run can offer, bound
//! under its wire name, which `tools.offer` scopes by id or tool object, a
//! model call resolves by wire name, and a script call resolves by id.

use runvil_parser::test_support::{PromptSource, one_section, prompt};

use super::fixtures::echo_tool;
use super::models_loop::catalog_scenario;
use super::*;
use crate::execute::run::{EffectRecord, ToolCallOrigin, ToolCaller};
use crate::execute::scope::{DispatchTarget, prepare_scoped_tools};
use crate::test_support::rig::Rig;
use crate::test_support::scenario::EXECUTION;
use runvil_model_client::detail::tool_schema_new;
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

/// A tool whose parameters are not a JSON object, which no model round can
/// advertise.
#[track_caller]
fn shapeless_tool(id: &str) -> FnTool {
    tool_at(id, json!("scalar"))
}

/// The one-section prompt that declares the `tools` Pack and returns at
/// once, prepared against a catalog of `tools`.
fn declaring_tools(tools: Vec<FnTool>) -> Scenario {
    let source = prompt("uses: [tools]", &[("Only", "return 'ok'")]);
    tools
        .into_iter()
        .fold(catalog_scenario(source), Scenario::tool)
}

/// The offered bindings' names and ids, in the set's order.
#[track_caller]
fn offered(rig: &mut Rig) -> Vec<(String, String)> {
    rig.scheduler_for_test()
        .tool_set_for_test()
        .offered()
        .iter()
        .map(|binding| (binding.alias().to_owned(), binding.id().to_string()))
        .collect()
}

fn pair(name: &str, id: &str) -> (String, String) {
    (name.to_owned(), id.to_owned())
}

#[test]
fn the_offering_binds_every_catalog_tool_under_its_wire_name_in_id_order() {
    let mut rig = Rig::start(declaring_tools(vec![
        echo_tool(),
        offered_tool("tools/concrete"),
        offered_tool("gh/search"),
        offered_tool("alpha/zed"),
        offered_tool("gh/issues.list"),
        offered_tool("alpha/fetch"),
    ]));
    assert_eq!(
        offered(&mut rig),
        vec![
            pair("alpha_fetch", "alpha/fetch"),
            pair("alpha_zed", "alpha/zed"),
            pair("gh_issues_list", "gh/issues.list"),
            pair("gh_search", "gh/search"),
            pair("tools_concrete", "tools/concrete"),
            pair("tools_echo", "tools/echo"),
        ],
        "a declared Pack's tools are offered beside every other Pack's"
    );
    let set = rig.scheduler_for_test().tool_set_for_test();
    assert_eq!(
        set.declared(),
        [PackId::parse("tools").expect("valid Pack")],
        "the set records the declared Packs"
    );
    let fetch = set
        .offered_binding("alpha_fetch")
        .expect("the offered name resolves");
    assert_eq!(
        fetch.description(),
        "alpha/fetch",
        "the binding holds the tool info's own text"
    );
    assert_eq!(
        set.offered_binding("alpha/fetch"),
        Some(fetch),
        "the canonical id finds the binding the wire name finds"
    );
}

#[test]
fn the_offering_leaves_out_a_name_that_is_unadvertisable_or_repeated() {
    let mut rig = Rig::start(declaring_tools(vec![
        echo_tool(),
        offered_tool("gh/list"),
        shapeless_tool("bad/shape"),
        offered_tool("dup/a.b"),
        offered_tool("dup/a/b"),
        offered_tool("ok/fine"),
    ]));
    assert_eq!(
        offered(&mut rig),
        vec![
            pair("dup_a_b", "dup/a/b"),
            pair("gh_list", "gh/list"),
            pair("ok_fine", "ok/fine"),
            pair("tools_echo", "tools/echo"),
        ],
        "the schema no round can advertise and the later repeat are each \
         left out"
    );
}

#[test]
fn a_script_offers_the_extras_and_the_model_calls_an_offered_tool_by_its_wire_name() {
    let gateway = ScriptedChat::new(vec![
        calls(&[("call_1", "tools_echo", r#"{"value":"hi"}"#)]),
        text("done"),
    ]);
    let source = one_section(
        "tools.offer(tools.extras())\n\
         local history = messages.new()\n\
         history:user('go')\n\
         models.loop(history)\n\
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
const TOOLS_TABLE_KEYS: &str = "call:function,extras:function,get:function,offer:function,offer_local:function,required:function";

#[test]
fn a_section_tools_table_holds_only_its_api_and_its_round_advertises_only_the_offered_tool() {
    let gateway = ScriptedChat::new(vec![text("done")]);
    let source = one_section(
        "local keys = {}\n\
         for key, value in pairs(tools) do keys[#keys + 1] = key .. ':' .. type(value) end\n\
         table.sort(keys)\n\
         tools.offer(tools.extras())\n\
         local history = messages.new()\n\
         history:user('go')\n\
         models.loop(history)\n\
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
        "local tool = tools.extras()[1]\n\
         return tools.call(tool, { value = 'a' }) .. '|' .. \
           tools.call('tools/echo', { value = 'b' }) .. '|' .. tool.id",
    );
    let rig = Rig::run(catalog_scenario(source).tool(echo_tool()));
    assert_eq!(rig.trace().text(), "echoed: a|echoed: b|tools/echo");
    let call = |value: &str| EffectRecord::ToolCall {
        tool: ToolId::parse("tools/echo").expect("valid id"),
        alias: "tools_echo".to_owned(),
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
            "tools.offer(packs.get('tools').tools)\n\
             local history = messages.new()\n\
             history:user('go')\n\
             models.loop(history)\n\
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
        "assert(packs.get('missing') == nil, 'an unknown Pack reads nil')\n\
         local gh = packs.get('gh')\n\
         if gh then tools.offer(gh.tools) end\n\
         local history = messages.new()\n\
         history:user('go')\n\
         models.loop(history)\n\
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
fn the_packs_table_is_installed_in_the_h1_vm() {
    let source = PromptSource::new()
        .front("uses: [tools]")
        .h1(
            "return packs.required()[1].name .. '|' .. packs.extras()[1].name .. '|' .. \
             tostring(tools.get('gh/search').pack == packs.get('gh'))",
        )
        .build();
    let rig = Rig::run(
        catalog_scenario(source)
            .tool(echo_tool())
            .tool(offered_tool("gh/search")),
    );
    assert_eq!(
        rig.trace().text(),
        "tools|gh|true",
        "the H1 pass reads the packs table"
    );
}

#[test]
fn a_round_leaves_out_an_offered_binding_a_local_tool_shares_a_name_with() {
    let offered = crate::lua::ToolBinding::from_info("tools_echo", &echo_tool().info());
    let local = tool_schema_new("tools_echo", "the local echo", json!({ "type": "object" }))
        .expect("the local schema is valid");
    let (schemas, dispatch) =
        prepare_scoped_tools(&[offered], &[local]).expect("the round's scope builds");
    assert_eq!(schemas.len(), 1, "the model sees one tool under the name");
    assert_eq!(schemas[0].description(), "the local echo");
    assert!(
        matches!(dispatch.get("tools_echo"), Some(DispatchTarget::Local)),
        "the local tool answers the name: {dispatch:?}"
    );
}

#[path = "offering-packs.rs"]
mod packs;
