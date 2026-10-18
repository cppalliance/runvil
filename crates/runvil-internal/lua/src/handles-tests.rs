//! Tests for the tool binding built from a catalog tool info: the
//! tool info's data is copied verbatim and its structured-output flag
//! selects the binding's output kind. The tool set finds an offered
//! binding by wire name or by canonical id.

use std::collections::BTreeMap;

use runvil_types::tools::{ToolId, ToolInfo};
use serde_json::json;

use super::{ToolBinding, ToolOutputKind, ToolSet};

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
fn an_offered_binding_is_found_by_its_wire_name_or_its_id_as_one_binding() {
    let offered = ToolBinding::from_info("tools_fetch", &tool_info(false));
    let set = ToolSet::new(Vec::new(), BTreeMap::new(), vec![offered.clone()]);
    assert_eq!(set.offered(), std::slice::from_ref(&offered));
    assert_eq!(set.offered_binding("tools_fetch"), Some(&offered));
    assert_eq!(
        set.offered_binding("tools/fetch"),
        Some(&offered),
        "the canonical id finds the binding the wire name finds"
    );
    for miss in ["fetch", "tools/missing", "tools_missing", "Tools/Fetch"] {
        assert_eq!(set.offered_binding(miss), None, "{miss:?} names no binding");
    }
}
