//! Tests for `GlobalName` parsing and its rejection messages, for the alias
//! grammar, and for the error every id type and catalog rejects with.

use std::num::NonZeroU32;

use serde_json::{Value, json};

use super::{GlobalName, is_alias};
use crate::error::{Error, ErrorCode, ErrorFact};
use crate::ids::{ChainId, TaskId};
use crate::models::{ModelCatalog, ModelId, ModelInfo, ThinkingMode};
use crate::packs::PackId;
use crate::tools::{ToolCatalog, ToolId, ToolInfo};

/// Asserts `error` is an invalid value that names `rejected` and keeps
/// `message` as its message.
fn assert_invalid_id(error: &Error, rejected: &str, message: &str) {
    assert_eq!(error.code, ErrorCode::InvalidValue, "{error:?}");
    assert_eq!(
        error.facts.get(&ErrorFact::Name),
        Some(&Value::from(rejected)),
        "{error:?}"
    );
    assert_eq!(error.message, message, "{error:?}");
    assert_eq!(error.prose, ErrorCode::InvalidValue.prose(), "{error:?}");
    assert_eq!(error.at, None, "{error:?}");
}

#[test]
fn every_id_type_rejects_with_an_invalid_value_naming_the_input() {
    let rejected = |result: Result<(), Error>| result.expect_err("the id must be rejected");
    assert_invalid_id(
        &rejected(GlobalName::parse("Web").map(drop)),
        "Web",
        "invalid global name: segments may contain only lowercase ASCII letters, digits, '-', \
         '_', '.'",
    );
    assert_invalid_id(
        &rejected(PackId::parse("a/b").map(drop)),
        "a/b",
        "invalid Pack id: a Pack id is one segment, such as `web`, with no '/'",
    );
    assert_invalid_id(
        &rejected(ToolId::parse("web").map(drop)),
        "web",
        "invalid tool id: a tool id has two or more segments, its Pack's name first, such as \
         `web/fetch`",
    );
    assert_invalid_id(
        &rejected("0.".parse::<ChainId>().map(drop)),
        "0.",
        "invalid chain id `0.`: required a dot-separated path of decimal components",
    );
    assert_invalid_id(
        &rejected("x".parse::<TaskId>().map(drop)),
        "x",
        "invalid chain id `x`: required a dot-separated path of decimal components",
    );
    assert_invalid_id(
        &rejected(ModelId::gateway("").map(drop)),
        "",
        "invalid model id: name must not be empty",
    );
}

#[test]
fn a_catalog_that_repeats_an_id_rejects_with_an_invalid_value_naming_it() {
    let tool = || {
        ToolInfo::new(
            ToolId::parse("web/fetch").expect("a valid tool id"),
            "Fetches a page.",
            json!({ "type": "object" }),
        )
    };
    assert_invalid_id(
        &ToolCatalog::new([tool(), tool()]).expect_err("a repeated tool id is refused"),
        "web/fetch",
        "duplicate tool identity `web/fetch` in the tool catalog",
    );
    let model = || {
        ModelInfo::new(
            ModelId::gateway("small").expect("a valid model id"),
            "A small model.",
            NonZeroU32::MIN,
            ThinkingMode::Never,
        )
    };
    assert_invalid_id(
        &ModelCatalog::new([model(), model()]).expect_err("a repeated model id is refused"),
        "gateway/small",
        "duplicate model identity in catalog: gateway/small",
    );
}

const EMPTY: &str = "invalid global name: segments must not be empty";
const CONTROL: &str = "invalid global name: segments must not contain a control character";
const CHARSET: &str =
    "invalid global name: segments may contain only lowercase ASCII letters, digits, '-', '_', '.'";

fn message_of(input: &str) -> String {
    GlobalName::parse(input)
        .expect_err("the input must be rejected")
        .to_string()
}

#[test]
fn a_one_segment_name_parses() {
    let name = GlobalName::parse("web").expect("a valid one-segment name");
    assert_eq!(name.segments(), ["web"]);
}

#[test]
fn any_number_of_segments_parses() {
    let name = GlobalName::parse("github/issues/create/v2").expect("a valid four-segment name");
    assert_eq!(name.segments(), ["github", "issues", "create", "v2"]);
}

#[test]
fn display_round_trips_a_one_segment_name() {
    let name = GlobalName::parse("web").expect("a valid name");
    assert_eq!(name.to_string(), "web");
    assert_eq!(
        GlobalName::parse(&name.to_string()).expect("the display form re-parses"),
        name
    );
}

#[test]
fn display_round_trips_a_multi_segment_name() {
    let name = GlobalName::parse("web/fetch").expect("a valid name");
    assert_eq!(name.to_string(), "web/fetch");
    assert_eq!(
        GlobalName::parse(&name.to_string()).expect("the display form re-parses"),
        name
    );
}

#[test]
fn the_first_segment_is_borrowed_from_the_name() {
    let name = GlobalName::parse("web/fetch/deep").expect("a valid name");
    assert_eq!(name.first(), "web");
}

#[test]
fn dashes_underscores_and_dots_are_legal_segment_characters() {
    GlobalName::parse("org.rustalliance/my-pack/v1_2.tool").expect("the charset allows - _ .");
}

#[test]
fn an_empty_string_is_rejected_as_an_empty_error() {
    assert_eq!(message_of(""), EMPTY);
}

#[test]
fn an_empty_middle_segment_is_rejected_as_an_empty_error() {
    assert_eq!(message_of("runvil//web"), EMPTY);
}

#[test]
fn a_leading_separator_is_rejected_as_an_empty_error() {
    assert_eq!(message_of("/web"), EMPTY);
}

#[test]
fn a_trailing_separator_is_rejected_as_an_empty_error() {
    assert_eq!(message_of("web/"), EMPTY);
}

#[test]
fn uppercase_is_rejected_as_a_charset_error_because_comparison_is_case_sensitive() {
    assert_eq!(message_of("Web"), CHARSET);
    assert_eq!(message_of("web/Fetch"), CHARSET);
}

#[test]
fn an_at_sign_is_rejected_as_a_charset_error_because_v1_is_unversioned() {
    assert_eq!(message_of("web@2"), CHARSET);
}

#[test]
fn a_control_character_is_rejected_as_a_control_error() {
    assert_eq!(message_of("we\tb"), CONTROL);
    assert_eq!(message_of("web\u{7f}"), CONTROL);
    // A space is the first byte past the control range.
    assert_eq!(message_of("we b"), CHARSET);
}

#[test]
fn non_ascii_is_rejected_as_a_charset_error() {
    assert_eq!(message_of("w\u{e9}b"), CHARSET);
}

#[test]
fn from_str_parses_like_parse() {
    let parsed: GlobalName = "web/fetch".parse().expect("a valid name parses");
    assert_eq!(
        parsed,
        GlobalName::parse("web/fetch").expect("a valid name")
    );
    let error = "Web"
        .parse::<GlobalName>()
        .expect_err("uppercase is rejected");
    assert_eq!(
        error,
        GlobalName::parse("Web").expect_err("uppercase is rejected")
    );
    assert_eq!(error.to_string(), CHARSET);
}

#[test]
fn is_alias_follows_the_alias_grammar() {
    for name in [
        "a",
        "A",
        "search",
        "web_fetch",
        "web-fetch2",
        "a_b-1",
        &"z".repeat(64),
    ] {
        assert!(is_alias(name), "{name:?} must be an alias");
    }
    for name in [
        "",
        "1search",
        "_search",
        "-search",
        "web fetch",
        "web.fetch",
        "web/fetch",
        "\u{e9}",
        &"a".repeat(65),
    ] {
        assert!(!is_alias(name), "{name:?} must not be an alias");
    }
}
