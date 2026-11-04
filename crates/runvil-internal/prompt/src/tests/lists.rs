//! List section tests: list detection and the bullet parser.

use super::*;
use crate::list::bullet_items;

#[test]
fn mixed_prose_with_one_bullet_is_not_a_list() {
    // An incidental bullet line in ordinary prose must not force strict
    // list parsing; the section stays prose.
    let body = "Here is context.\n- one incidental bullet\nMore prose follows.";
    let prompt = parse_ok(&PromptSource::new().prose("S", body).build());
    let section = &prompt.sections[0];
    assert!(!section.is_list(), "mixed prose is not a list");
    assert!(section.items().is_empty());
    assert!(section.prose().contains("incidental bullet"));
}

#[test]
fn pure_list_section_parses_items() {
    let body = "- alpha\n- beta\n3. gamma";
    let prompt = parse_ok(&PromptSource::new().prose("S", body).build());
    let section = &prompt.sections[0];
    assert!(section.is_list());
    assert_eq!(section.items(), ["alpha", "beta", "gamma"]);
}

#[test]
fn all_marker_list_with_empty_item_is_rejected() {
    // Every nonblank line is a marker, so it is a list; the empty marker is
    // then a hard error rather than a detector miss.
    let body = "- alpha\n1.\n- beta";
    let error = parse_err(&PromptSource::new().prose("S", body).build());
    assert_eq!(error.code, ErrorCode::ListEmptyItem);
}

#[test]
fn an_empty_marker_before_a_prose_line_stays_prose() {
    let prompt = parse_ok(&PromptSource::new().prose("S", "- alpha\n-\nmore").build());
    let section = &prompt.sections[0];
    assert!(section.items().is_empty());
    assert_eq!(section.prose(), "- alpha\n-\nmore");
}

#[test]
fn list_error_kind_does_not_depend_on_the_section_name() {
    for section in ["frontmatter", "fence"] {
        let error = parse_err(&PromptSource::new().prose(section, "- alpha\n1.").build());
        assert_eq!(error.code, ErrorCode::ListEmptyItem);
    }
}

/// The items of `prose`, which has no empty marker.
#[track_caller]
fn items(prose: &str) -> Option<Vec<&str>> {
    bullet_items(prose, |marker| panic!("no empty marker, got {marker:?}"))
        .expect("a prose with no empty marker is never refused")
}

#[test]
fn bullet_parser_strips_unordered_markers() {
    assert_eq!(
        items("- alpha\n* beta\n- gamma"),
        Some(vec!["alpha", "beta", "gamma"])
    );
}

#[test]
fn bullet_parser_strips_ordered_markers() {
    assert_eq!(
        items("1. first\n2. second\n3) third"),
        Some(vec!["first", "second", "third"])
    );
}

#[test]
fn bullet_parser_ignores_blank_lines() {
    assert_eq!(
        items("- alpha\n\n- beta\n  \n- gamma"),
        Some(vec!["alpha", "beta", "gamma"])
    );
}

#[test]
fn bullet_parser_fails_with_the_failure_built_from_the_first_empty_marker() {
    let prose = "- alpha\n  - \n1.\n- gamma";
    let mut seen = None;
    let error = bullet_items(prose, |marker| {
        seen = Some(marker);
        Error::new(ErrorCode::ListEmptyItem, "empty item", "empty item")
    })
    .expect_err("an empty item must be refused");
    assert_eq!(error.code, ErrorCode::ListEmptyItem, "the caller's failure");
    assert_eq!(seen, Some("-"));
    assert_eq!(
        seen.map(str::as_ptr),
        Some(prose[10..].as_ptr()),
        "the trimmed marker of the first empty item, sliced from the prose"
    );
}

#[test]
fn list_h3_parses_items_at_load_time() {
    let src = PromptSource::new().prose("Parent", "p");
    let p = parse_ok(&src.prose("### Items", "- alpha\n- beta").build());
    let items_section = &p.sections[0].children[0];
    assert_eq!(items_section.name, "Items");
    assert_eq!(items_section.items, vec!["alpha", "beta"]);
}

#[test]
fn non_list_h3_has_empty_items() {
    let worker = "```lua\nreturn item\n```\n\nDo work on {{ item }}.";
    let src = PromptSource::new().prose("Parent", "p");
    let p = parse_ok(&src.prose("### Worker", worker).build());
    let worker = &p.sections[0].children[0];
    assert_eq!(worker.name, "Worker");
    assert!(worker.items.is_empty());
}
