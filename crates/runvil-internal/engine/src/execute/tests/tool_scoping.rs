//! Tests for model-visible tool scoping through `tools.offer`.

use std::collections::BTreeMap;

use runvil_parser::test_support::one_section;
use runvil_types::tools::ToolCatalog;

use super::fixtures::scoped_tool;
use super::models_loop::{LOOP_TO_TEXT, loop_scenario};
use super::*;
use crate::execute::scope::{merge_tool_sets, offered_packs, tool_schema};
use crate::lua::{OfferedTool, ToolSet};
use crate::model::ToolSchema;
use crate::test_support::scenario::TEST_SEED;
use runvil_lua::{AdhocTools, OfferMode};

/// The tool set whose catalog holds `tool` under the wire name `alias`,
/// described as `description`, for a test that reads the offers without a
/// run.
fn offering_of(alias: &str, description: &str, tool: &FnTool) -> ToolSet {
    let info = tool
        .clone()
        .description(description)
        .wire_name(alias)
        .info();
    ToolSet::new(
        Vec::new(),
        BTreeMap::new(),
        ToolCatalog::new([info]).expect("one tool"),
        BTreeMap::new(),
    )
}

/// The schemas a round advertises when the static offers name `offered`
/// from `tools`.
fn advertised(tools: &ToolSet, offered: &[OfferedTool]) -> Vec<ToolSchema> {
    let frozen = merge_tool_sets(offered, &[], offered_packs(offered, &[]), tools.catalog())
        .expect("the sets merge");
    frozen
        .static_set
        .iter()
        .map(|binding| tool_schema(binding).expect("the schema builds"))
        .collect()
}

/// An offered tool stays out of the model-visible scope until
/// `tools.offer` names it: the scope snapshot over an untouched runtime is
/// empty. (The advertised-set half of this rule is the loop's schema
/// build, pinned by the offer tests below.)
#[test]
fn offered_tools_are_not_injected_without_an_offer() {
    let (tool, _) = scoped_tool("concrete", "Concrete description.");
    let tools = offering_of("tools_concrete", "capability", &tool);
    let runtime = Mutex::new(runvil_lua::ToolRuntime::default());
    let effective = current_tool_bindings(&tools, &runtime, &AdhocTools::default())
        .expect("the scope must snapshot");
    assert!(
        effective.is_empty(),
        "an offering must not expose a tool without an explicit offer"
    );
}

#[test]
fn an_offered_tool_advertises_its_concrete_schema_under_its_wire_name_and_dispatches_by_id() {
    let gateway = ScriptedChat::new(vec![
        calls(&[("aliased_call", "tools_concrete", "{\"value\":\"payload\"}")]),
        text("aliased final"),
    ]);
    let (tool, called) = scoped_tool("concrete", "Concrete description.");
    let tools = offering_of("tools_concrete", "Concrete description.", &tool);
    let mut runtime = runvil_lua::ToolRuntime::default();
    runtime.add_tool(
        ToolId::parse("tools/concrete").expect("a fixture tool id"),
        OfferMode::Static,
    );
    let effective = current_tool_bindings(&tools, &Mutex::new(runtime), &AdhocTools::default())
        .expect("the offered scope snapshots");
    let schemas = advertised(&tools, &effective);
    assert_eq!(schemas.len(), 1);
    assert_eq!(schemas[0].name(), "tools_concrete");
    assert_eq!(schemas[0].description(), "Concrete description.");

    let source = one_section(&format!("tools.offer('tools/concrete')\n{LOOP_TO_TEXT}"));
    let trace = loop_scenario(source)
        .offer("tools_concrete", tool)
        .chat(&gateway)
        .run();

    assert_eq!(trace.text(), "aliased final");
    assert_eq!(called.load(Ordering::SeqCst), 1);
    let bodies = gateway.requests();
    let function = &bodies[0].tools[0];
    assert_eq!(function.name(), "tools_concrete");
    assert_eq!(function.description(), "Concrete description.");
    assert_eq!(
        function.parameters(),
        &json!({
            "type": "object",
            "properties": {"value": {"type": "string"}},
            "required": ["value"]
        })
    );
}

#[test]
fn a_section_offer_by_id_scopes_the_tool_and_dispatches_it() {
    let gateway = ScriptedChat::new(vec![
        calls(&[("aliased_call", "tools_section", "{\"value\":\"payload\"}")]),
        text("aliased final"),
    ]);
    let (tool, called) = scoped_tool("section", "Section concrete.");
    let tools = offering_of("tools_section", "capability", &tool);
    let vm = SectionVm::new_for_section(
        &GuardNonce::from_seed(&TEST_SEED),
        &Arc::new(tools),
        &Arc::new(Mutex::new(ModelSet::default())),
        &null_emitter(),
        "Only",
    )
    .expect("the section VM builds")
    .inject_values("", &json!({}), &fresh_access())
    .expect("values must inject");

    // The section's `tools.offer` lands in its tool runtime; the scope
    // snapshot over it includes the offered tool's wire name.
    let offer = LuaProgram::compile(
        "tools.offer('tools/section')",
        "prologue",
        NonZeroU32::new(1).expect("compile source line is non-zero"),
        &null_emitter(),
        "Only",
    )
    .expect("the offer chunk must compile");
    vm.run_chunk(&offer, &null_emitter(), "Only")
        .expect("tools.offer must succeed");
    let (tool_set, tool_runtime) = vm.tool_bag_handles();
    let scope = current_tool_bindings(&tool_set, &tool_runtime, vm.adhoc_tools())
        .expect("tool scope must snapshot");
    let schemas = advertised(&tool_set, &scope);
    assert_eq!(schemas.len(), 1);
    assert_eq!(schemas[0].name(), "tools_section");
    vm.teardown(&null_emitter(), "Only");

    // The same `tools.offer` inside a section scopes the tool for the
    // loop's rounds: the round advertises it and dispatches the concrete
    // tool behind it.
    let source = one_section(&format!("tools.offer('tools/section')\n{LOOP_TO_TEXT}"));
    let trace = loop_scenario(source)
        .offer("tools_section", tool)
        .chat(&gateway)
        .run();

    assert_eq!(trace.text(), "aliased final");
    assert_eq!(called.load(Ordering::SeqCst), 1);
    assert_eq!(gateway.requests()[0].tools[0].name(), "tools_section");
}
