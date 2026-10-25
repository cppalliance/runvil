//! Tests for the Pack identity vocabulary.

use super::{PackId, PackIdErrorKind, RESERVED_PACK_IDS};
use crate::test_support::assert_wire;
use crate::tools::ToolId;

#[test]
fn pack_id_requires_exactly_one_segment() {
    let id = PackId::parse("web").expect("one segment parses");
    assert_eq!(id.to_string(), "web");

    let two = PackId::parse("runvil/web")
        .expect_err("a vendor/name pair is a fully qualified name, not a Pack id");
    assert_eq!(two.kind(), PackIdErrorKind::SegmentCount);

    let tool = PackId::parse("web/fetch").expect_err("two segments are a tool id");
    assert_eq!(tool.kind(), PackIdErrorKind::SegmentCount);
}

#[test]
fn pack_id_rejects_empty_and_control_segments() {
    let empty = PackId::parse("").expect_err("an empty id is rejected");
    assert_eq!(empty.kind(), PackIdErrorKind::Empty);

    let upper = PackId::parse("Web")
        .expect_err("comparison is case-sensitive: uppercase is outside the charset");
    assert_eq!(upper.kind(), PackIdErrorKind::Charset);

    let versioned = PackId::parse("web@2").expect_err("v1 is unversioned: '@' is a parse error");
    assert_eq!(versioned.kind(), PackIdErrorKind::Charset);

    let accented = PackId::parse("w\u{e9}b").expect_err("non-ASCII is outside the charset");
    assert_eq!(accented.kind(), PackIdErrorKind::Charset);

    let control = PackId::parse("we\tb").expect_err("a control byte is rejected");
    assert_eq!(control.kind(), PackIdErrorKind::Control);
}

#[test]
fn pack_id_from_str_parses_like_parse() {
    let parsed: PackId = "web".parse().expect("one segment parses");
    assert_eq!(parsed, PackId::parse("web").expect("one segment parses"));
    let error = "web/fetch"
        .parse::<PackId>()
        .expect_err("two segments are a tool id");
    assert_eq!(error.kind(), PackIdErrorKind::SegmentCount);
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
