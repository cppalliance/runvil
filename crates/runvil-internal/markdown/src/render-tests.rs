//! Tests of the writer: that it writes sources and nothing else, the two
//! line endings it adds, and that parsed text is written back as it was.

use super::{Writer, render};
use crate::test_support::{
    BODIES, GUIDE, SEED, Seeded, bullets, fence, flat, numbered, prose, rule, section, sourced,
};
use crate::{Block, parse, parse_body};

/// A prose block whose source is `source`.
fn words(source: &str) -> Block {
    sourced(prose(0, source.trim()), source)
}

#[test]
fn an_empty_list_is_empty_text() {
    assert_eq!(render(&[]), "");
    assert_eq!(Writer::new().finish(), "");
}

#[test]
fn a_block_is_written_as_its_source_and_a_section_before_its_blocks() {
    let inner = sourced(section(0, 3, "B", vec![words("b\n")]), "### B\n\n");
    let outer = sourced(
        section(0, 2, "A", vec![sourced(rule(0), "***\n\n"), inner]),
        "## A\n\n",
    );
    let after = sourced(bullets(0, &["after"]), "- after\n");
    assert_eq!(
        render(&[outer.clone(), after]),
        "## A\n\n***\n\n### B\n\nb\n- after\n"
    );
    // A section's blocks are written without its heading.
    let Block::Section { blocks, .. } = &outer else {
        panic!("the outer block is a section");
    };
    assert_eq!(render(blocks), "***\n\n### B\n\nb\n");
}

#[test]
fn no_field_but_the_source_is_written() {
    // Each block's other fields say something else than its source.
    let blocks = [
        sourced(section(9, 4, "Other", vec![]), "Usage\n-----\n\n"),
        sourced(numbered(9, &["x"]), "3) first\n4) second\n\n"),
        sourced(bullets(9, &["y", "z"]), "* only\n"),
        sourced(fence(9, "lua", "return 2"), "~~~ ~`\nreturn 1\n~~~\n"),
        sourced(rule(9), "___\n"),
        sourced(prose(9, "other"), "  kept as written  \n"),
    ];
    assert_eq!(
        render(&blocks),
        "Usage\n-----\n\n3) first\n4) second\n\n* only\n~~~ ~`\nreturn 1\n~~~\n___\n  kept as written  \n"
    );
}

#[test]
fn a_block_with_an_empty_source_writes_nothing() {
    assert_eq!(render(&[rule(0), prose(0, "x"), fence(0, "lua", "y")]), "");
    // The blocks under a section with no source are still written.
    let group = section(0, 1, "A", vec![words("a\n"), sourced(rule(0), "---\n")]);
    assert_eq!(render(&[group]), "a\n---\n");
}

#[test]
fn every_block_starts_on_a_new_line() {
    // Each of the first three sources lacks its line ending.
    let blocks = [
        sourced(section(0, 1, "A", vec![sourced(rule(0), "---")]), "# A"),
        sourced(fence(0, "", "x"), "```\nx\n```"),
        sourced(bullets(0, &["a"]), "- a\n"),
        sourced(numbered(0, &["b"]), "1. b"),
    ];
    assert_eq!(render(&blocks), "# A\n---\n```\nx\n```\n- a\n1. b");
    // Nothing is added after the last block, or before the first.
    assert_eq!(render(&[sourced(rule(0), "---")]), "---");
    // The line ending goes before a block that starts with a blank line too.
    assert_eq!(
        render(&[sourced(rule(0), "---"), sourced(rule(0), "\n***\n")]),
        "---\n\n***\n"
    );
}

#[test]
fn a_blank_line_sits_between_two_prose_blocks() {
    assert_eq!(render(&[words("one\n"), words("two\n")]), "one\n\ntwo\n");
    // With no line ending on the first, both are added.
    assert_eq!(render(&[words("one"), words("two")]), "one\n\ntwo");
    assert_eq!(
        render(&[words("one\n"), words("two\n"), words("three")]),
        "one\n\ntwo\n\nthree"
    );
    // The last prose block of a section and the block after the section.
    let section = sourced(section(0, 1, "A", vec![words("in\n")]), "# A\n");
    assert_eq!(render(&[section, words("out\n")]), "# A\nin\n\nout\n");
    // A block with no source between them does not part them.
    assert_eq!(
        render(&[words("one\n"), rule(0), words("two\n")]),
        "one\n\ntwo\n"
    );
}

#[test]
fn no_blank_line_is_added_where_one_already_parts_two_prose_blocks() {
    for (first, second, written) in [
        ("one\n\n", "two\n", "one\n\ntwo\n"),
        ("one\n\n\n", "two\n", "one\n\n\ntwo\n"),
        // A line of spaces and tabs is a blank line.
        ("one\n \t\n", "two\n", "one\n \t\ntwo\n"),
        // The second block may start with it.
        ("one\n", "\ntwo\n", "one\n\ntwo\n"),
        ("one", "  \ntwo", "one\n  \ntwo"),
    ] {
        assert_eq!(render(&[words(first), words(second)]), written);
    }
}

#[test]
fn no_blank_line_is_added_between_any_other_two_blocks() {
    let heading = || sourced(section(0, 1, "H", vec![]), "# H\n");
    let list = || sourced(bullets(0, &["a"]), "- a\n");
    let code = || sourced(fence(0, "", "x"), "```\nx\n```\n");
    let line = || sourced(rule(0), "---\n");
    for (blocks, written) in [
        // Prose right above a list is common, and is two blocks as written.
        (vec![words("see:\n"), list()], "see:\n- a\n"),
        (vec![list(), words("after\n")], "- a\nafter\n"),
        (vec![heading(), words("text\n")], "# H\ntext\n"),
        (vec![words("text\n"), heading()], "text\n# H\n"),
        (vec![list(), list()], "- a\n- a\n"),
        (vec![code(), words("text\n")], "```\nx\n```\ntext\n"),
        (vec![words("text\n"), code()], "text\n```\nx\n```\n"),
        (vec![line(), words("text\n")], "---\ntext\n"),
        // A block of another kind between two prose blocks parts them.
        (
            vec![words("one\n"), code(), words("two\n")],
            "one\n```\nx\n```\ntwo\n",
        ),
    ] {
        assert_eq!(render(&blocks), written);
    }
}

#[test]
fn a_writer_takes_a_kind_by_its_name_and_only_prose_is_parted() {
    let mut writer = Writer::new();
    writer.write("prose", "one");
    writer.write("prose", "two\n");
    // A kind the crate does not know is written like any other.
    writer.write("note", "three");
    writer.write("", "four\n");
    writer.write("Prose", "five\n");
    writer.write("prose", "six");
    writer.write("prose", "");
    assert_eq!(writer.finish(), "one\n\ntwo\nthree\nfour\nfive\nsix");
}

#[test]
fn what_the_writer_adds_keeps_blocks_of_different_texts_apart() {
    // The last block of a text has no line ending when the text has none.
    let mixed = [parse("# A\nfirst"), parse("second\n\n# B")].concat();
    // Joined as they are, the two paragraphs share a line.
    let joined: String = flat(&mixed).into_iter().map(Block::source).collect();
    assert_eq!(joined, "# A\nfirstsecond\n\n# B");
    assert_eq!(render(&mixed), "# A\nfirst\n\nsecond\n\n# B");
    // Written twice over, the first heading of the second copy would
    // share a line with the last heading of the first.
    let written = render(&[mixed.clone(), mixed].concat());
    assert_eq!(
        written,
        "# A\nfirst\n\nsecond\n\n# B\n# A\nfirst\n\nsecond\n\n# B"
    );
    let read: Vec<(&str, usize)> = flat(&parse(&written))
        .into_iter()
        .map(|block| (block.kind().name(), block.line()))
        .collect();
    assert_eq!(
        read,
        [
            ("section", 1),
            ("prose", 2),
            ("section", 6),
            ("section", 7),
            ("prose", 8),
            ("section", 12)
        ]
    );
}

/// `text` with `\r\n` for every line ending.
fn with_crlf(text: &str) -> String {
    text.replace('\n', "\r\n")
}

/// Asserts that `text`, whose line endings are all `\n`, is written back
/// as it is, whichever way it was read. Text of blank lines alone has no
/// block, so it is written as nothing.
#[track_caller]
fn assert_written_back(text: &str, context: &str) {
    let want = if text.trim_matches([' ', '\t', '\n']).is_empty() {
        ""
    } else {
        text
    };
    assert_eq!(render(&parse(text)), want, "{context}");
    assert_eq!(render(&parse_body(text, 5)), want, "body, {context}");
    // A byte order mark and the other line endings are not written back.
    assert_eq!(render(&parse(&with_crlf(text))), want, "crlf, {context}");
    let marked = format!("\u{feff}{text}");
    assert_eq!(render(&parse(&marked)), want, "marked, {context}");
}

#[test]
fn every_sample_is_written_back_as_it_was_read() {
    assert_written_back(GUIDE, "the guide");
    for body in BODIES {
        assert_written_back(body, &format!("{body:?}"));
    }
}

#[test]
fn generated_texts_are_written_back_as_they_were_read() {
    let mut rng = Seeded::new(SEED);
    for number in 0..2000 {
        let text = rng.text();
        assert_written_back(&text, &format!("seed {SEED:#x}, text {number}, {text:?}"));
    }
}

#[test]
fn text_of_blank_lines_alone_is_the_one_text_not_written_back() {
    for text in ["\n", "\n\n", "  \n\t\n", "   "] {
        assert_eq!(render(&parse(text)), "", "{text:?}");
    }
    // With a block above them, they are written.
    for text in ["x\n\n\n", "---\na: 1\n---\n\n\n", "# H\n \n"] {
        assert_eq!(render(&parse(text)), text, "{text:?}");
    }
}

#[test]
fn what_a_rewriting_writer_changed_is_written_back_unchanged() {
    for text in [
        // A list that does not start at 1, and one cut in two by a fence.
        "3. third\n4. fourth\n",
        "1. Install it\n\n```sh\ncargo install x\n```\n\n2. Run it\n",
        "1. a\n1. b\n1. c\n",
        "5) five\n6) six\n",
        // Markers, fences, rules, and headings in every spelling.
        "* a\n* b\n\n+ c\n",
        "~~~lua\nreturn 1\n~~~\n",
        "Title\n=====\n\nSub\n---\n",
        "___\n\n***\n\n- - -\n",
        "## See [API](http://x) and `code` ##\n",
        // Frontmatter with comments, its own key order, and flow style.
        "---\n# why\nz: 1\na: { b: [1, 2] }\n---\n\n\n\nbody\n",
        // Blank lines in any number, and a text with no final line ending.
        "# A\n\n\n\nx\n\n\n\n# B",
    ] {
        assert_eq!(render(&parse(text)), text, "{text:?}");
    }
}

#[test]
fn a_tilde_fence_whose_label_starts_with_a_tilde_keeps_the_blocks_after_it() {
    // Written with the fence and its label run together, the fence would
    // lose its closing line and take the rest of the text.
    let text = "~~~ ~`\nx\n~~~\n\n# After\n\ntail\n";
    let blocks = parse(text);
    let kinds: Vec<&str> = flat(&blocks)
        .into_iter()
        .map(|block| block.kind().name())
        .collect();
    assert_eq!(kinds, ["fence", "section", "prose"]);
    let written = render(&blocks);
    assert_eq!(written, text);
    assert_eq!(parse(&written), blocks);
}
