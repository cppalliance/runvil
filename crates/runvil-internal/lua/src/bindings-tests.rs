//! Tests for the tool binding built from a catalog tool info: the
//! tool info's data is copied verbatim and its structured-output flag
//! selects the binding's output kind. A catalog tool's wire name is its
//! own when set and its default otherwise, and the tool set reads the run
//! catalog and derives each tool's vendor from its Pack, `runvil` for the
//! Engine's discovery tools.

use std::collections::BTreeMap;

use runvil_types::packs::PackId;
use runvil_types::tools::{ToolCatalog, ToolId, ToolInfo};
use serde_json::json;

use super::{RosterEntry, ToolBinding, ToolOutputKind, ToolSet, wire_name_of};

/// A tool info for `tools/fetch`.
fn tool_info(structured: bool) -> ToolInfo {
    ToolInfo::new(
        ToolId::parse("tools/fetch").expect("the id is valid"),
        "Fetch a page",
        json!({"type": "object", "properties": {"url": {"type": "string"}}}),
    )
    .structured(structured)
}

#[test]
fn a_structured_tool_info_binds_with_structured_output() {
    let info = tool_info(true);
    let binding = ToolBinding::from_info("fetch_alias", &info);
    assert_eq!(binding.output_kind(), ToolOutputKind::Structured);
    assert_eq!(binding.alias(), "fetch_alias");
    assert_eq!(binding.id(), &info.id);
    assert_eq!(binding.description(), "Fetch a page");
    assert_eq!(binding.schema(), &info.parameters_schema);
}

#[test]
fn a_plain_tool_info_binds_with_plain_output() {
    let binding = ToolBinding::from_info("fetch_alias", &tool_info(false));
    assert_eq!(binding.output_kind(), ToolOutputKind::Plain);
}

#[test]
fn a_new_tool_info_binds_callable_by_a_script() {
    assert!(
        !ToolBinding::from_info("fetch_alias", &tool_info(false)).model_only_call(),
        "a tool info the Pack did not mark stays callable from a script"
    );
}

#[test]
fn a_model_only_tool_info_binds_with_the_flag_set() {
    let info = tool_info(false).model_only_call(true);
    let binding = ToolBinding::from_info("fetch_alias", &info);
    assert!(
        binding.model_only_call(),
        "the binding carries the catalog's refusal"
    );
}

#[test]
fn the_test_seam_overrides_only_the_description() {
    let info = tool_info(true);
    let binding = ToolBinding::for_test("fetch_alias", "test double", &info);
    assert_eq!(binding.description(), "test double");
    assert_eq!(
        binding.output_kind(),
        ToolOutputKind::Structured,
        "the output kind still follows the tool info"
    );
}

#[test]
fn a_catalog_tools_wire_name_is_its_own_when_set_and_its_default_otherwise() {
    let info = tool_info(false);
    assert_eq!(wire_name_of(&info), "tools_fetch");
    assert_eq!(wire_name_of(&info.wire_name("fetch")), "fetch");
    let dotted = ToolInfo::new(
        ToolId::parse("gh/issues.list").expect("the id is valid"),
        "List issues",
        json!({ "type": "object" }),
    );
    assert_eq!(
        wire_name_of(&dotted),
        "gh_issues_list",
        "both `/` and `.` turn into `_`"
    );
}

#[test]
fn the_tool_set_reads_the_catalog_and_takes_each_tools_vendor_from_its_pack() {
    let pack = |name: &str| PackId::parse(name).expect("the Pack id is valid");
    let id = |id: &str| ToolId::parse(id).expect("the id is valid");
    let set = ToolSet::new(
        Vec::new(),
        BTreeMap::from([(
            pack("tools"),
            RosterEntry::Usable {
                vendor: "acme".to_owned(),
            },
        )]),
        ToolCatalog::new([tool_info(false)]).expect("one tool"),
    );
    assert_eq!(
        set.catalog()
            .get(&id("tools/fetch"))
            .map(|info| info.description.as_str()),
        Some("Fetch a page")
    );
    assert_eq!(set.vendor_for(&id("tools/fetch")), Some("acme"));
    assert_eq!(
        set.vendor_for(&id("tools/rendered_only")),
        Some("acme"),
        "a tool outside the catalog reports its Pack's vendor"
    );
    assert_eq!(
        set.vendor_for(&id("adhoc/grab")),
        None,
        "an adhoc tool reports none"
    );
    assert_eq!(
        set.vendor_for(&id("web/fetch")),
        None,
        "a Pack with no vendor reports none"
    );
    for discovery in ["dynamic/search", "dynamic/call"] {
        assert_eq!(
            set.vendor_for(&id(discovery)),
            Some("runvil"),
            "the Engine's discovery tool {discovery} reports runvil"
        );
    }
}
