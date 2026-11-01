//! Tests for the Pack identity vocabulary.

use super::{PackId, RESERVED_PACK_IDS};
use crate::test_support::assert_wire;
use crate::tools::ToolId;

const SEGMENT_COUNT: &str = "invalid Pack id: a Pack id is one segment, such as `web`, with no '/'";
const EMPTY: &str = "invalid Pack id: segments must not be empty";
const CONTROL: &str = "invalid Pack id: segments must not contain a control character";
const CHARSET: &str =
    "invalid Pack id: segments may contain only lowercase ASCII letters, digits, '-', '_', '.'";

fn message_of(input: &str, why: &str) -> String {
    PackId::parse(input).expect_err(why).to_string()
}

#[test]
fn pack_id_requires_exactly_one_segment() {
    let id = PackId::parse("web").expect("one segment parses");
    assert_eq!(id.to_string(), "web");

    assert_eq!(
        message_of(
            "runvil/web",
            "a vendor/name pair is a fully qualified name, not a Pack id"
        ),
        SEGMENT_COUNT
    );
    assert_eq!(
        message_of("web/fetch", "two segments are a tool id"),
        SEGMENT_COUNT
    );
}

#[test]
fn pack_id_rejects_empty_and_control_segments() {
    assert_eq!(message_of("", "an empty id is rejected"), EMPTY);
    assert_eq!(
        message_of(
            "Web",
            "comparison is case-sensitive: uppercase is outside the charset"
        ),
        CHARSET
    );
    assert_eq!(
        message_of("web@2", "v1 is unversioned: '@' is a parse error"),
        CHARSET
    );
    assert_eq!(
        message_of("w\u{e9}b", "non-ASCII is outside the charset"),
        CHARSET
    );
    assert_eq!(message_of("we\tb", "a control byte is rejected"), CONTROL);
}

#[test]
fn pack_id_from_str_parses_like_parse() {
    let parsed: PackId = "web".parse().expect("one segment parses");
    assert_eq!(parsed, PackId::parse("web").expect("one segment parses"));
    let error = "web/fetch"
        .parse::<PackId>()
        .expect_err("two segments are a tool id");
    assert_eq!(
        error,
        PackId::parse("web/fetch").expect_err("two segments are a tool id")
    );
    assert_eq!(error.to_string(), SEGMENT_COUNT);
}

#[test]
fn pack_id_contains_exactly_the_tools_whose_first_segment_it_is() {
    let web = PackId::parse("web").expect("a static valid id");
    let fetch = ToolId::parse("web/fetch").expect("a static valid id");
    assert!(web.contains(&fetch));
    let deep = ToolId::parse("web/fetch/raw").expect("a static valid id");
    assert!(
        web.contains(&deep),
        "every segment past the first is the Pack's own"
    );
    let stray = ToolId::parse("other/web").expect("a static valid id");
    assert!(
        !web.contains(&stray),
        "only the first segment names the Pack"
    );
    // Containment is by identity, not by prefix text: a Pack whose name
    // merely extends this one is not contained.
    let extended = ToolId::parse("web2/fetch").expect("a static valid id");
    assert!(!web.contains(&extended));
}

#[test]
fn exactly_adhoc_and_dynamic_are_reserved_and_each_is_a_valid_pack_id() {
    assert_eq!(RESERVED_PACK_IDS, ["adhoc", "dynamic"]);
    for reserved in RESERVED_PACK_IDS {
        let id = PackId::parse(reserved).expect("a reserved id is a valid Pack id");
        assert!(id.is_reserved(), "{reserved}");
    }
    let search = ToolId::parse("dynamic/search").expect("a static valid id");
    assert!(
        search.pack().is_reserved(),
        "a tool's Pack id is reserved as a parsed one is"
    );
    for free in ["web", "subagents", "adhoc2", "dynamics", "dyn", "x-adhoc"] {
        let id = PackId::parse(free).expect("a static valid id");
        assert!(!id.is_reserved(), "{free} is not reserved");
    }
}

#[test]
fn pack_id_serializes_as_its_string_form() {
    let id = PackId::parse("user-input").expect("a static valid id");
    assert_wire(&id, "\"user-input\"");
    assert!(
        serde_json::from_str::<PackId>("\"runvil/user-input\"").is_err(),
        "a 2-segment string is never a Pack id"
    );
}
