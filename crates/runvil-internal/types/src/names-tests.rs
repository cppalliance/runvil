//! Tests for `GlobalName` parsing and its rejection messages, and for the
//! alias grammar.

use super::{GlobalName, is_alias};

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
