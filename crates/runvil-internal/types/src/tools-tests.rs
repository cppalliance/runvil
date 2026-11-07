//! Tests for `ToolId`, `ToolInfo`, `ToolCatalog`, tool output trust, and
//! `make_tool_error`.

use serde_json::json;

use super::{ToolCatalog, ToolId, ToolInfo};
use crate::packs::PackId;
use crate::test_support::{assert_send_sync, assert_wire, round_trip};

fn inspect_id() -> ToolId {
    ToolId::parse("fixtures/tools/inspect").expect("fixture id is valid")
}

/// The fixture tool info: the `inspect` tool as data.
fn inspect_info() -> ToolInfo {
    ToolInfo::new(
        inspect_id(),
        "Inspect a fixture.",
        json!({
            "type": "object",
            "properties": {"path": {"type": "string"}},
            "required": ["path"]
        }),
    )
}

/// A catalog fixture tool info under `fixtures/tools/<id_name>`, described
/// as `id_name`.
fn catalog_info(id_name: &str) -> ToolInfo {
    ToolInfo::new(
        ToolId::parse(&format!("fixtures/tools/{id_name}")).expect("fixture id is valid"),
        id_name,
        json!({"type": "object"}),
    )
}

#[test]
fn tool_output_has_mandatory_trust() {
    use super::{OutputTrust, ToolOutput};
    assert_eq!(ToolOutput::trusted("a").trust(), OutputTrust::Trusted);
    assert_eq!(ToolOutput::untrusted("b").trust(), OutputTrust::Untrusted);
    assert_eq!(ToolOutput::trusted("a").text(), "a");
}

#[test]
fn output_trust_serializes_as_its_snake_case_name() {
    use super::OutputTrust;
    assert_wire(&OutputTrust::Trusted, r#""trusted""#);
    assert_wire(&OutputTrust::Untrusted, r#""untrusted""#);
}

#[test]
fn tool_catalog_is_send_and_sync() {
    // The public catalog must stay `Send + Sync` so downstream callers can
    // share it across tasks; a representation change that dropped either
    // auto trait would fail to compile here.
    assert_send_sync::<ToolCatalog>();
}

#[test]
fn a_tool_error_shows_its_text_as_message_and_prose_and_keeps_a_cause_out_of_the_prose() {
    use super::make_tool_error;
    use crate::error::ErrorCode;
    let error = make_tool_error(ErrorCode::ToolBackend, "backend failed");
    assert_eq!(error.code, ErrorCode::ToolBackend);
    assert_eq!(error.message, "backend failed");
    assert_eq!(error.prose, "backend failed");

    let caused = error.with_cause(&std::io::Error::other("boom"));
    assert_eq!(
        caused.prose, "backend failed",
        "a cause never reaches the prose the model reads"
    );
    assert!(
        caused.message.contains("boom"),
        "the message holds the cause: {}",
        caused.message
    );
}

#[test]
fn an_empty_catalog_builds_and_holds_no_tools() {
    let catalog = ToolCatalog::new([]).expect("an empty catalog builds");
    assert!(catalog.tools().is_empty());
}

#[test]
fn a_tool_info_has_the_tools_surface_and_round_trips_through_serde() {
    // The tool info is the tool as data: identity, description, schema,
    // and the output kind, so a catalog built from `ToolInfo` values holds
    // no implementation and round-trips through serde.
    let info = inspect_info();
    assert_eq!(info.id, inspect_id());
    assert_eq!(info.description, "Inspect a fixture.");
    assert_eq!(info.parameters_schema["required"], json!(["path"]));
    assert!(
        !info.structured_output,
        "a tool info that does not declare structured output stays plain text"
    );
    assert_eq!(round_trip(&info), info);
}

#[test]
fn survives_stop_sets_the_flag_and_round_trips_through_serde() {
    assert!(
        !inspect_info().survives_stop,
        "a new tool info does not survive a stop"
    );
    let info = inspect_info().survives_stop(true);
    assert!(info.survives_stop);
    assert_eq!(round_trip(&info), info);
}

#[test]
fn a_wire_name_its_builder_sets_round_trips_exactly_in_canonical_key_order() {
    let info = inspect_info().wire_name("inspect");
    assert_eq!(info.wire_name.as_deref(), Some("inspect"));
    let canonical = serde_json::to_value(&info).expect("a tool info converts to a JSON value");
    assert_wire(
        &canonical,
        r#"{"describes_tools":false,"description":"Inspect a fixture.","id":"fixtures/tools/inspect","model_only_call":false,"parameters_schema":{"properties":{"path":{"type":"string"}},"required":["path"],"type":"object"},"structured_output":false,"survives_stop":false,"wire_name":"inspect"}"#,
    );
    let back: ToolInfo = serde_json::from_value(canonical).expect("the canonical form reads back");
    assert_eq!(back, info);
    assert_eq!(round_trip(&info), info);
}

#[test]
fn model_only_call_sets_the_flag_and_round_trips_through_serde() {
    assert!(
        !inspect_info().model_only_call,
        "a new tool info is callable by a script"
    );
    let info = inspect_info().model_only_call(true);
    assert!(info.model_only_call);
    assert_eq!(round_trip(&info), info);
    let canonical = serde_json::to_value(&info).expect("a tool info converts to a JSON value");
    assert_eq!(canonical["model_only_call"], json!(true));
}

#[test]
fn a_tool_call_origin_keeps_its_logged_shape() {
    use super::{ToolCallOrigin, ToolCaller};
    let origin = ToolCallOrigin::new("Chat", ToolCaller::Model);
    let line = r#"{"section":"Chat","caller":"model"}"#;
    assert_wire(&origin, line);
}

#[test]
fn a_serialized_tool_call_origin_has_no_execution_key() {
    use super::{ToolCallOrigin, ToolCaller};
    let origin = ToolCallOrigin::new("Chat", ToolCaller::Script).with_entry("0.1");
    let value = serde_json::to_value(&origin).expect("an origin converts to a JSON value");
    assert!(
        value.get("execution").is_none(),
        "an origin names no run label: {value}"
    );
}

#[test]
fn a_tool_call_origin_without_entry_or_model_reads_both_as_absent() {
    use super::ToolCallOrigin;
    let line = r#"{"section":"Chat","caller":"script"}"#;
    let origin: ToolCallOrigin =
        serde_json::from_str(line).expect("an origin without entry or model deserializes");
    assert_eq!(
        (origin.entry.as_deref(), origin.model.as_deref()),
        (None, None),
        "absent fields read as none"
    );
    assert_wire(&origin, line);
}

#[test]
fn a_tool_call_origin_naming_its_entry_and_model_logs_both_after_its_caller() {
    use super::{ToolCallOrigin, ToolCaller};
    let origin = ToolCallOrigin::new("Chat", ToolCaller::Model)
        .with_entry("0.1")
        .with_model("test-model");
    let line = r#"{"section":"Chat","caller":"model","entry":"0.1","model":"test-model"}"#;
    assert_wire(&origin, line);
}

#[test]
fn catalog_lookup_matches_the_whole_tool_id() {
    let catalog = ToolCatalog::new([inspect_info()]).expect("unique catalog");

    let found = catalog
        .get(&inspect_id())
        .expect("the stable identity should resolve");
    assert_eq!(found.description, "Inspect a fixture.");
    assert!(
        catalog
            .get(&ToolId::parse("other/inspect").expect("valid id"))
            .is_none(),
        "a tool's last segment alone must not resolve it"
    );
}

#[test]
fn catalog_preserves_order_and_first_match_lookup() {
    let catalog = ToolCatalog::new([catalog_info("inspect"), catalog_info("summarize")])
        .expect("distinct identities build a catalog");

    assert_eq!(
        catalog
            .tools()
            .iter()
            .map(|tool| tool.id.to_string())
            .collect::<Vec<_>>(),
        ["fixtures/tools/inspect", "fixtures/tools/summarize"]
    );
    assert_eq!(catalog.tools().len(), 2);
    assert_eq!(
        catalog
            .get(&inspect_id())
            .expect("the identity should resolve")
            .description,
        "inspect",
    );
}

#[test]
fn catalog_rejects_duplicate_tool_ids() {
    let error = ToolCatalog::new([catalog_info("inspect"), catalog_info("inspect")])
        .expect_err("a repeated tool identity must be rejected at catalog construction");
    assert_eq!(
        error.to_string(),
        "duplicate tool identity `fixtures/tools/inspect` in the tool catalog",
        "the error must name the duplicated identity"
    );
}

const SEGMENT_COUNT: &str = "invalid tool id: a tool id has two or more segments, its Pack's name first, such as `web/fetch`";
const EMPTY: &str = "invalid tool id: segments must not be empty";
const CONTROL: &str = "invalid tool id: segments must not contain a control character";
const CHARSET: &str =
    "invalid tool id: segments may contain only lowercase ASCII letters, digits, '-', '_', '.'";

fn tool_id_error(input: &str) -> String {
    ToolId::parse(input)
        .expect_err("the input must be rejected")
        .to_string()
}

#[test]
fn a_two_segment_tool_id_parses_and_exposes_its_name() {
    let id = ToolId::parse("web/fetch").expect("a valid tool id");
    assert_eq!(id.name(), "fetch");
}

#[test]
fn a_tool_ids_pack_is_its_first_segment() {
    let id = ToolId::parse("web/fetch").expect("a valid tool id");
    assert_eq!(
        id.pack(),
        PackId::parse("web").expect("a valid Pack id"),
        "the first segment must yield the contributing Pack's local name"
    );
}

#[test]
fn a_deeper_tool_id_names_its_pack_first_and_its_tool_last() {
    let id = ToolId::parse("github/issues/create").expect("a valid tool id");
    assert_eq!(id.name(), "create");
    assert_eq!(id.pack(), PackId::parse("github").expect("a valid Pack id"));
}

#[test]
fn four_segments_parse_as_a_tool_id() {
    let id = ToolId::parse("github/issues/comments/create").expect("a valid tool id");
    assert_eq!(id.name(), "create");
    assert_eq!(id.to_string(), "github/issues/comments/create");
}

#[test]
fn a_single_segment_is_rejected_as_a_tool_id() {
    assert_eq!(tool_id_error("web"), SEGMENT_COUNT);
}

#[test]
fn an_empty_tool_id_is_rejected_as_an_empty_error() {
    assert_eq!(tool_id_error(""), EMPTY);
}

#[test]
fn an_empty_segment_is_rejected_as_an_empty_error() {
    assert_eq!(tool_id_error("web//fetch"), EMPTY);
}

#[test]
fn a_control_character_is_rejected_as_a_control_error() {
    assert_eq!(tool_id_error("we\tb/fetch"), CONTROL);
    assert_eq!(tool_id_error("web/fe\u{7f}tch"), CONTROL);
}

#[test]
fn an_uppercase_segment_is_rejected_as_a_charset_error_because_comparison_is_case_sensitive() {
    assert_eq!(tool_id_error("Web/fetch"), CHARSET);
}

#[test]
fn an_at_sign_or_non_ascii_segment_is_rejected_as_a_charset_error() {
    assert_eq!(tool_id_error("web@2/fetch"), CHARSET);
    assert_eq!(tool_id_error("web/f\u{e9}tch"), CHARSET);
}

#[test]
fn a_tool_id_from_str_parses_like_parse() {
    let parsed: ToolId = "web/fetch".parse().expect("a valid tool id parses");
    assert_eq!(parsed, ToolId::parse("web/fetch").expect("a valid tool id"));
    let error = "web"
        .parse::<ToolId>()
        .expect_err("one segment is rejected");
    assert_eq!(
        error,
        ToolId::parse("web").expect_err("one segment is rejected")
    );
    assert_eq!(error.to_string(), SEGMENT_COUNT);
}

#[test]
fn the_built_in_tool_ids_parse() {
    assert!(ToolId::parse("web/fetch").is_ok());
    assert!(ToolId::parse("web/search").is_ok());
    assert!(ToolId::parse("user-input/ask").is_ok());
}

#[test]
fn a_tool_id_serializes_as_its_global_name_string() {
    let id = ToolId::parse("web/fetch").expect("a valid tool id");
    assert_wire(&id, "\"web/fetch\"");
}

#[test]
fn deserializing_an_invalid_tool_id_is_a_data_error() {
    assert!(serde_json::from_str::<ToolId>("\"web_fetch\"").is_err());
    assert!(serde_json::from_str::<ToolId>("\"web//fetch\"").is_err());
}
