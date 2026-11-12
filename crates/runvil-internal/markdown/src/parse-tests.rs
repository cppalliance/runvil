//! Tests of the parse as a whole: what each kind of text becomes. The
//! tests here state a block by its fields other than its source, which
//! `parse-tests-source.rs` tests.

use serde_json::json;

use super::events::marks;
use super::{parse, parse_body};
use crate::Block;
use crate::test_support::{
    BODIES, GUIDE, bullets, fence, frontmatter, numbered, prose, rule, section, without_sources,
};

#[path = "parse-tests-source.rs"]
mod source;

/// The blocks of `text`, without their sources.
fn fields(text: &str) -> Vec<Block> {
    without_sources(&parse(text))
}

#[test]
fn a_document_with_every_kind_of_block_parses_to_its_tree() {
    let expected = vec![
        frontmatter(json!({"title": "Notes", "tags": ["a", "b"]})),
        prose(6, "Preface paragraph."),
        section(
            8,
            1,
            "Guide",
            vec![
                prose(10, "Intro text\non two lines."),
                bullets(13, &["alpha", "beta"]),
                section(
                    16,
                    2,
                    "Templates",
                    vec![
                        prose(18, "Use these."),
                        fence(20, "lua", "return 1"),
                        fence(24, "markdown", "# Hello"),
                        prose(28, "Then the fallback:"),
                        fence(30, "json strict", "{ \"ok\": true }"),
                        section(
                            34,
                            3,
                            "Extras",
                            vec![rule(36), fence(38, "lua", "return 2")],
                        ),
                    ],
                ),
                section(42, 2, "Usage", vec![numbered(44, &["first", "second"])]),
            ],
        ),
    ];
    assert_eq!(fields(GUIDE), expected);
}

#[test]
fn empty_text_and_blank_lines_are_an_empty_document() {
    assert_eq!(fields(""), []);
    assert_eq!(fields("\n\n   \n\t\n"), []);
}

#[test]
fn prose_is_a_maximal_run() {
    // Two paragraphs separated by a blank line are one block.
    assert_eq!(fields("first\n\nsecond\n"), [prose(1, "first\n\nsecond")]);
    assert_eq!(fields("one paragraph"), [prose(1, "one paragraph")]);
    // A second prose block exists only when another block sits between.
    assert_eq!(
        fields("text\n- a\n- b\n"),
        [prose(1, "text"), bullets(2, &["a", "b"])]
    );
    assert_eq!(
        fields("text\n```\ncode\n```\nafter\n"),
        [prose(1, "text"), fence(2, "", "code"), prose(5, "after")]
    );
}

#[test]
fn what_follows_a_list_is_prose_from_its_first_line() {
    // A link definition right after a list is not an item's text, so it
    // is prose, with or without a paragraph after it.
    assert_eq!(
        fields("- a\n\n[ref]: http://x\ntext\n"),
        [bullets(1, &["a"]), prose(3, "[ref]: http://x\ntext")]
    );
    assert_eq!(
        fields("-\n[ref]: http://x\ntext\n"),
        [bullets(1, &[""]), prose(2, "[ref]: http://x\ntext")]
    );
    assert_eq!(
        fields("1. a\n\n[one]: http://x\n[two]: http://y\n"),
        [
            numbered(1, &["a"]),
            prose(3, "[one]: http://x\n[two]: http://y")
        ]
    );
    // A paragraph indented too little to be the item's keeps its
    // indentation, as prose after any other block does.
    assert_eq!(
        fields("- a\n\n text\n"),
        [bullets(1, &["a"]), prose(3, " text")]
    );
    assert_eq!(
        fields("-\n\n   text\n"),
        [bullets(1, &[""]), prose(3, "   text")]
    );
}

#[test]
fn everything_that_is_no_other_kind_is_prose() {
    for text in [
        "    code line\n\ntext",
        "<div>\nraw\n</div>",
        "| a | b |\n|---|---|\n| 1 | 2 |",
        "[ref]: http://example.com",
        "<!--\n# not a heading\n-->\n\ntext",
        "\\# not heading",
        "     # five spaces",
        "#NotHeading",
    ] {
        assert_eq!(fields(text), [prose(1, text)], "{text:?}");
    }
}

#[test]
fn a_construct_inside_a_block_quote_stays_in_the_prose() {
    for text in [
        "> # Quoted",
        "> - a\n> - b\n>\n> ```\n> code\n> ```",
        "> ---",
    ] {
        assert_eq!(fields(text), [prose(1, text)], "{text:?}");
    }
}

#[test]
fn a_heading_opens_a_section_and_its_title_has_no_markup() {
    assert_eq!(
        fields("## The *big* `foo` API ##\n\ntext\n"),
        [section(1, 2, "The big foo API", vec![prose(3, "text")])]
    );
    assert_eq!(
        fields("## Title ##\n\n### C#\n\n#### A #\n"),
        [section(
            1,
            2,
            "Title",
            vec![section(3, 3, "C#", vec![section(5, 4, "A", vec![])])]
        )]
    );
    assert_eq!(
        fields("  # indented\n"),
        [section(1, 1, "indented", vec![])]
    );
}

#[test]
fn a_heading_with_no_text_has_an_empty_title() {
    assert_eq!(
        fields("##\n\ntext\n\n#   \n"),
        [
            section(1, 2, "", vec![prose(3, "text")]),
            section(5, 1, "", vec![])
        ]
    );
}

#[test]
fn an_underlined_line_is_a_heading() {
    assert_eq!(
        fields("Title\n=====\n\ntext\n\nSub\n---\n\nmore\n"),
        [section(
            1,
            1,
            "Title",
            vec![
                prose(4, "text"),
                section(6, 2, "Sub", vec![prose(9, "more")])
            ]
        )]
    );
    // A line break inside a heading reads as a space.
    assert_eq!(
        fields("Line one\nline two\n===\n"),
        [section(1, 1, "Line one line two", vec![])]
    );
    // A paragraph line directly above `---` is a heading, not a rule.
    assert_eq!(
        fields("text\n---\n\nafter\n"),
        [section(1, 2, "text", vec![prose(4, "after")])]
    );
}

#[test]
fn a_section_runs_to_the_next_heading_of_its_level_or_a_smaller_one() {
    // A heading that skips levels is a member of the section above it.
    assert_eq!(
        fields("## A\n\n#### B\n\nb\n\n### C\n\n# D\n\n###### E\n"),
        [
            section(
                1,
                2,
                "A",
                vec![
                    section(3, 4, "B", vec![prose(5, "b")]),
                    section(7, 3, "C", vec![])
                ]
            ),
            section(9, 1, "D", vec![section(11, 6, "E", vec![])]),
        ]
    );
    // A shallower heading after a deeper one starts a section of its own.
    assert_eq!(
        fields("### A\n\na\n\n# B\n\nb\n"),
        [
            section(1, 3, "A", vec![prose(3, "a")]),
            section(5, 1, "B", vec![prose(7, "b")])
        ]
    );
    // Parsed text may repeat a title among siblings.
    assert_eq!(
        fields("# Log\n\n## Entry\n\nfirst\n\n## Entry\n\nsecond\n"),
        [section(
            1,
            1,
            "Log",
            vec![
                section(3, 2, "Entry", vec![prose(5, "first")]),
                section(7, 2, "Entry", vec![prose(9, "second")]),
            ]
        )]
    );
}

#[test]
fn a_fence_keeps_its_info_string_and_the_lines_between_its_fence_lines() {
    assert_eq!(
        fields("~~~\nplain\n~~~\n\n```\nbare\n```\n\n``` rust ignore \nfn x() {}\n```\n"),
        [
            fence(1, "", "plain"),
            fence(5, "", "bare"),
            fence(9, "rust ignore", "fn x() {}"),
        ]
    );
    // The fence's own indentation is removed from its text.
    assert_eq!(
        fields("  ```py\n  x = 1\n    y = 2\n  ```\n"),
        [fence(1, "py", "x = 1\n  y = 2")]
    );
    // A longer fence holds a shorter one as text.
    assert_eq!(
        fields("````md\n```lua\nx\n```\n````\n"),
        [fence(1, "md", "```lua\nx\n```")]
    );
}

#[test]
fn a_fence_with_no_lines_inside_has_empty_text() {
    assert_eq!(fields("```json\n```\n"), [fence(1, "json", "")]);
    // The final line ending is not part of the text, so one blank line
    // inside reads as empty text too, and two as one line ending.
    assert_eq!(fields("```\n\n```\n"), [fence(1, "", "")]);
    assert_eq!(fields("```\n\n\n```\n"), [fence(1, "", "\n")]);
    assert_eq!(fields("```\na\n\n```\n"), [fence(1, "", "a\n")]);
}

#[test]
fn a_fence_is_opaque_and_one_with_no_closing_line_runs_to_the_end() {
    assert_eq!(
        fields("# A\n\n```lua\nreturn 1\n\n# not a heading\n"),
        [section(
            1,
            1,
            "A",
            vec![fence(3, "lua", "return 1\n\n# not a heading")]
        )]
    );
}

#[test]
fn a_list_is_one_block_and_an_item_is_its_source_without_the_marker() {
    assert_eq!(fields("+ a\n+ b\n"), [bullets(1, &["a", "b"])]);
    assert_eq!(fields("  - a\n  - b\n"), [bullets(1, &["a", "b"])]);
    assert_eq!(
        fields("- a\n\n- b\n\n- c\n"),
        [bullets(1, &["a", "b", "c"])]
    );
    assert_eq!(
        fields("- [ ] todo\n- [x] done\n"),
        [bullets(1, &["[ ] todo", "[x] done"])]
    );
    // The start number is not kept.
    assert_eq!(fields("5) five\n6) six\n"), [numbered(1, &["five", "six"])]);
    assert_eq!(
        fields("10. ten\n11. eleven\n"),
        [numbered(1, &["ten", "eleven"])]
    );
}

#[test]
fn an_item_with_no_content_is_the_empty_string() {
    assert_eq!(
        fields("-\n- b\n-\n  later\n"),
        [bullets(1, &["", "b", "later"])]
    );
    assert_eq!(fields("- a\n-   \n"), [bullets(1, &["a", ""])]);
}

#[test]
fn an_item_that_spans_lines_keeps_them_as_markdown() {
    assert_eq!(
        fields("- first line\n  second line\n\n  second paragraph\n- next\n"),
        [bullets(
            1,
            &["first line\nsecond line\n\nsecond paragraph", "next"]
        )]
    );
    // A nested list, a heading, a rule, and a fence stay in the item.
    assert_eq!(
        fields("- a\n  - a1\n  - a2\n- b\n  1. b1\n"),
        [bullets(1, &["a\n- a1\n- a2", "b\n1. b1"])]
    );
    assert_eq!(
        fields("- # Item heading\n- one\n\n  ---\n- two\n"),
        [bullets(1, &["# Item heading", "one\n\n---", "two"])]
    );
    assert_eq!(
        fields("1. step\n\n   ```sh\n   run\n   ```\n2. done\n"),
        [numbered(1, &["step\n\n```sh\nrun\n```", "done"])]
    );
    // An item's text parses again as Markdown.
    assert_eq!(
        fields("a\n- a1\n- a2"),
        [prose(1, "a"), bullets(2, &["a1", "a2"])]
    );
}

#[test]
fn a_change_of_marker_starts_a_new_list() {
    assert_eq!(
        fields("- a\n- b\n\n* c\n* d\n\n1. e\n\n1) f\n"),
        [
            bullets(1, &["a", "b"]),
            bullets(4, &["c", "d"]),
            numbered(7, &["e"]),
            numbered(9, &["f"]),
        ]
    );
    assert_eq!(
        fields("1. one\n- bullet\n2. two\n"),
        [
            numbered(1, &["one"]),
            bullets(2, &["bullet"]),
            numbered(3, &["two"])
        ]
    );
}

#[test]
fn a_rule_is_a_block_of_its_own() {
    assert_eq!(
        fields("***\n\n___\n\n- - -\n\ntext\n"),
        [rule(1), rule(3), rule(5), prose(7, "text")]
    );
}

#[test]
fn frontmatter_is_the_first_block_and_its_lines_are_counted() {
    assert_eq!(
        fields("---\ntitle: Notes\nnested: { a: [1, 2], b: null }\n---\nbody\n"),
        [
            frontmatter(json!({"title": "Notes", "nested": {"a": [1, 2], "b": null}})),
            prose(5, "body"),
        ]
    );
    assert_eq!(fields("---\na: 1\n---"), [frontmatter(json!({"a": 1}))]);
    // A second `---` pair after the frontmatter is ordinary Markdown.
    assert_eq!(
        fields("---\ntitle: x\n---\n---\n\ntext\n"),
        [
            frontmatter(json!({"title": "x"})),
            rule(4),
            prose(6, "text")
        ]
    );
}

#[test]
fn text_that_only_looks_like_frontmatter_is_ordinary_markdown() {
    // No closing delimiter: the first line is a rule.
    assert_eq!(
        fields("---\na: 1\n\ntext\n"),
        [rule(1), prose(2, "a: 1\n\ntext")]
    );
    // The lines between are not a mapping.
    assert_eq!(
        fields("---\njust words\n---\n\ntext\n"),
        [rule(1), section(2, 2, "just words", vec![prose(5, "text")])]
    );
    assert_eq!(
        fields("---\n---\n\ntext\n"),
        [rule(1), rule(2), prose(4, "text")]
    );
    // Not at the very start.
    assert_eq!(
        fields("\n---\na: 1\n---\n"),
        [rule(2), section(3, 2, "a: 1", vec![])]
    );
}

#[test]
fn line_endings_are_read_as_newlines_and_a_byte_order_mark_is_dropped() {
    assert_eq!(
        fields("# T\r\n\r\npara one\r\npara two\r\n\r\n```\r\ncode\r\n```\r\n\r\n- a\r\n- b\r\n"),
        [section(
            1,
            1,
            "T",
            vec![
                prose(3, "para one\npara two"),
                fence(6, "", "code"),
                bullets(10, &["a", "b"]),
            ]
        )]
    );
    assert_eq!(
        fields("# T\r\rtext\r"),
        [section(1, 1, "T", vec![prose(3, "text")])]
    );
    assert_eq!(
        fields("\u{feff}---\na: 1\n---\n\n# T\n"),
        [frontmatter(json!({"a": 1})), section(5, 1, "T", vec![])]
    );
}

#[test]
fn a_body_has_no_frontmatter_and_its_lines_count_from_the_line_given() {
    let own = "Fill in this outline.\n\n```markdown\n# Title\n\n## Summary\n```";
    assert_eq!(
        without_sources(&parse_body(own, 23)),
        [
            prose(23, "Fill in this outline."),
            fence(25, "markdown", "# Title\n\n## Summary")
        ]
    );
    assert_eq!(
        without_sources(&parse_body("---\na: b\n---\n", 1)),
        [rule(1), section(2, 2, "a: b", vec![])]
    );
    assert_eq!(parse_body("", 7), []);
}

/// The number of non-blank lines of `text`.
fn non_blank(text: &str) -> usize {
    text.split('\n')
        .filter(|line| !line.trim_matches([' ', '\t']).is_empty())
        .count()
}

/// Adds the prose text of `blocks`, at every depth, to `out`.
fn prose_texts<'a>(blocks: &'a [Block], out: &mut Vec<&'a str>) {
    for block in blocks {
        match block {
            Block::Prose { text, .. } => out.push(text),
            Block::Section { blocks, .. } => prose_texts(blocks, out),
            _ => {}
        }
    }
}

/// Adds the line of each of `blocks`, at every depth and in file order,
/// to `out`.
fn lines(blocks: &[Block], out: &mut Vec<usize>) {
    for block in blocks {
        out.push(block.line());
        if let Block::Section { blocks, .. } = block {
            lines(blocks, out);
        }
    }
}

#[test]
fn every_non_blank_line_belongs_to_exactly_one_block() {
    for body in BODIES {
        // A line is in a heading, fence, list, or rule when a mark covers
        // it, and otherwise it has to be in a prose block.
        let marked: usize = marks(body)
            .iter()
            .map(|mark| non_blank(&body[mark.range.clone()]))
            .sum();
        let blocks = parse_body(body, 1);
        let mut texts = Vec::new();
        prose_texts(&blocks, &mut texts);
        let in_prose: usize = texts.iter().map(|text| non_blank(text)).sum();
        assert_eq!(marked + in_prose, non_blank(body), "{body:?}");
    }
}

#[test]
fn blocks_are_in_file_order_and_each_starts_on_a_non_blank_line() {
    for body in BODIES {
        let mut found = Vec::new();
        lines(&parse_body(body, 1), &mut found);
        assert!(found.is_sorted_by(|a, b| a < b), "{body:?}: {found:?}");
        let source: Vec<&str> = body.split('\n').collect();
        for line in found {
            assert!(!source[line - 1].trim().is_empty(), "{body:?}: line {line}");
        }
    }
}

#[test]
fn no_block_is_empty_where_the_model_says_it_cannot_be() {
    for body in BODIES {
        let blocks = parse_body(body, 1);
        let mut texts = Vec::new();
        prose_texts(&blocks, &mut texts);
        for text in texts {
            assert!(!text.is_empty(), "{body:?}");
            assert_eq!(text, text.trim_end(), "{body:?}");
            assert!(!text.starts_with('\n'), "{body:?}");
        }
    }
}
