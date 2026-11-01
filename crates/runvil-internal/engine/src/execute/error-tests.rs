//! The stable tag of each run-error kind: the variant's name, which parses
//! back to the kind.

use super::RunErrorKind;

/// Every kind, in declaration order.
const KINDS: [RunErrorKind; 12] = [
    RunErrorKind::Parse,
    RunErrorKind::Version,
    RunErrorKind::Binding,
    RunErrorKind::Completion,
    RunErrorKind::Tool,
    RunErrorKind::Vfs,
    RunErrorKind::Determinism,
    RunErrorKind::Lua,
    RunErrorKind::Quota,
    RunErrorKind::ContextExhausted,
    RunErrorKind::Internal,
    RunErrorKind::RequirementsUnmet,
];

#[test]
fn each_kinds_tag_is_its_debug_name_and_parses_back_to_the_kind() {
    for kind in KINDS {
        assert_eq!(kind.as_str(), format!("{kind:?}"));
        assert_eq!(RunErrorKind::from_tag(kind.as_str()), Some(kind));
    }
}

#[test]
fn a_tag_no_kind_has_parses_to_none() {
    for tag in ["Input", "parse", "", "Internal "] {
        assert_eq!(RunErrorKind::from_tag(tag), None, "{tag:?}");
    }
}
