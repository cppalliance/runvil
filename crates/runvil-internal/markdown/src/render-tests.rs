//! Tests of the writer: each block's style, and that parsed text reads
//! back as the same blocks.

use serde_json::json;

use super::render;
use crate::test_support::{
    BODIES, GUIDE, bullets, fence, frontmatter, numbered, prose, rule, section, without_lines,
};
use crate::{Block, parse};

#[test]
fn an_empty_list_is_empty_text_and_blocks_are_joined_by_one_blank_line() {
    assert_eq!(render(&[]), "");
    assert_eq!(
        render(&[prose(0, "one"), prose(0, "two"), rule(0)]),
        "one\n\ntwo\n\n---"
    );
}

#[test]
fn a_section_is_its_heading_then_its_blocks() {
    let blocks = vec![numbered(0, &["first", "second"])];
    // A section's blocks render without its heading.
    assert_eq!(render(&blocks), "1. first\n2. second");
    assert_eq!(
        render(&[section(0, 2, "Usage", blocks)]),
        "## Usage\n\n1. first\n2. second"
    );
    assert_eq!(render(&[section(0, 6, "Deep", vec![])]), "###### Deep");
    // An empty title leaves the markers alone.
    assert_eq!(
        render(&[section(0, 2, "", vec![prose(0, "text")])]),
        "##\n\ntext"
    );
    assert_eq!(
        render(&[section(
            0,
            1,
            "A",
            vec![section(0, 2, "B", vec![prose(0, "b")])]
        )]),
        "# A\n\n## B\n\nb"
    );
}

#[test]
fn a_fence_is_longer_than_any_run_of_backticks_in_its_text() {
    assert_eq!(
        render(&[fence(0, "lua", "return 1")]),
        "```lua\nreturn 1\n```"
    );
    assert_eq!(
        render(&[fence(0, "md", "```lua\nx\n```")]),
        "````md\n```lua\nx\n```\n````"
    );
    assert_eq!(
        render(&[fence(0, "", "a ````` b")]),
        "``````\na ````` b\n``````"
    );
    // The whole info string is written, not only its first word.
    assert_eq!(
        render(&[fence(0, "json strict", "{}")]),
        "```json strict\n{}\n```"
    );
    // An empty text puts no line between the two fences.
    assert_eq!(render(&[fence(0, "json", "")]), "```json\n```");
}

#[test]
fn a_fence_with_no_info_string_is_labeled_by_its_language() {
    let block = Block::Fence {
        line: 0,
        lang: "lua".to_owned(),
        info: String::new(),
        text: "x".to_owned(),
    };
    assert_eq!(render(&[block]), "```lua\nx\n```");
}

#[test]
fn a_fence_whose_info_string_holds_a_backtick_is_written_with_tildes() {
    assert_eq!(render(&[fence(0, "a`b", "x")]), "~~~a`b\nx\n~~~");
    assert_eq!(
        render(&[fence(0, "a`b", "~~~\n```")]),
        "~~~~a`b\n~~~\n```\n~~~~"
    );
}

#[test]
fn a_list_is_one_marker_per_item_with_later_lines_under_the_first() {
    assert_eq!(render(&[bullets(0, &["alpha", "beta"])]), "- alpha\n- beta");
    assert_eq!(
        render(&[bullets(0, &["a\n- a1\n- a2", "b\n\npara", ""])]),
        "- a\n  - a1\n  - a2\n- b\n\n  para\n-"
    );
    // A numbered list counts from 1, and a wider number indents further.
    let ten: Vec<String> = (1..=10).map(|n| format!("i{n}\nmore")).collect();
    let ten: Vec<&str> = ten.iter().map(String::as_str).collect();
    let text = render(&[numbered(0, &ten)]);
    assert!(text.starts_with("1. i1\n   more\n2. i2\n"), "{text}");
    assert!(
        text.ends_with("9. i9\n   more\n10. i10\n    more"),
        "{text}"
    );
}

#[test]
fn a_list_right_after_a_list_of_the_same_type_takes_the_other_marker() {
    assert_eq!(
        render(&[
            bullets(0, &["a"]),
            bullets(0, &["b"]),
            bullets(0, &["c"]),
            numbered(0, &["d"]),
            numbered(0, &["e", "f"]),
            bullets(0, &["g"]),
        ]),
        "- a\n\n* b\n\n- c\n\n1. d\n\n1) e\n2) f\n\n- g"
    );
    // Lists with a block between them both take the first marker.
    assert_eq!(
        render(&[bullets(0, &["a"]), prose(0, "text"), bullets(0, &["b"])]),
        "- a\n\ntext\n\n- b"
    );
}

#[test]
fn a_rule_that_opens_the_text_is_written_with_stars() {
    assert_eq!(render(&[rule(0), rule(0)]), "***\n\n---");
    assert_eq!(render(&[prose(0, "a"), rule(0)]), "a\n\n---");
    // Only the first line of the text could start frontmatter.
    assert_eq!(render(&[section(0, 1, "A", vec![rule(0)])]), "# A\n\n---");
    let lookalike = [rule(0), section(0, 2, "a: b", vec![])];
    assert_eq!(without_lines(&parse(&render(&lookalike))), lookalike);
}

#[test]
fn frontmatter_is_yaml_between_two_delimiters_with_its_keys_sorted() {
    let data = json!({"title": "Notes", "tags": ["a", "b"], "n": null, "count": 2});
    assert_eq!(
        render(&[frontmatter(data), prose(0, "text")]),
        "---\ncount: 2\nn: null\ntags:\n- a\n- b\ntitle: Notes\n---\n\ntext"
    );
    assert_eq!(render(&[frontmatter(json!({}))]), "---\n{}\n---");
}

#[test]
fn text_is_written_as_it_is() {
    assert_eq!(
        render(&[prose(0, "  a *b*\n\n> c"), section(0, 1, "x *y*", vec![])]),
        "  a *b*\n\n> c\n\n# x *y*"
    );
}

/// Asserts that the blocks of `text` are written as text that parses to
/// the same blocks, apart from their lines.
#[track_caller]
fn assert_reads_back(text: &str) {
    let blocks = parse(text);
    let written = render(&blocks);
    assert_eq!(
        without_lines(&parse(&written)),
        without_lines(&blocks),
        "{text:?} written as {written:?}"
    );
}

#[test]
fn parsed_text_is_written_as_text_that_parses_to_the_same_blocks() {
    assert_reads_back(GUIDE);
    for body in BODIES {
        assert_reads_back(body);
    }
    for text in [
        "---\ntitle: Notes\ntags: [a, b]\nn: null\n---\n\n# T\n\nx\n",
        "---\n1: one\ntrue: yes\nwhen: 2026-11-11\nbig: .inf\n---\n",
        "---\ntitle: x\n---\n---\n\ntext\n",
        "---\n\na: b\n\n---\n\ntext\n",
        "# T\r\n\r\npara\r\n",
    ] {
        assert_reads_back(text);
    }
}

#[test]
fn a_title_that_markdown_reads_as_markup_does_not_read_back() {
    let blocks = parse("## \\*x\\*\n");
    assert_eq!(blocks, [section(1, 2, "*x*", vec![])]);
    assert_eq!(render(&blocks), "## *x*");
    assert_eq!(parse("## *x*"), [section(1, 2, "x", vec![])]);
}
