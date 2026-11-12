//! Tests of a parse's sources: which lines each block holds, that the
//! sources are the whole text, and that each is the text of its block
//! and of no other.

use serde_json::json;

use crate::parse::BLANK;
use crate::test_support::{
    BODIES, GUIDE, SEED, Seeded, bullets, fence, flat, frontmatter, numbered, prose, rule, section,
    sourced, without_sources,
};
use crate::{Block, Kind, parse, parse_body};

/// The source of every block of `text`, in file order.
fn sources(text: &str) -> Vec<String> {
    flat(&parse(text))
        .into_iter()
        .map(|block| block.source().to_owned())
        .collect()
}

#[test]
fn a_source_is_the_blocks_own_lines_and_the_blank_lines_after_them() {
    let text = "# Fruit\n\nSome words.\n\n- apples\n- pears\n\n## Citrus\n\n### Lemon\n";
    let lemon = sourced(section(10, 3, "Lemon", vec![]), "### Lemon\n");
    let citrus = sourced(section(8, 2, "Citrus", vec![lemon]), "## Citrus\n\n");
    let members = vec![
        sourced(prose(3, "Some words."), "Some words.\n\n"),
        sourced(bullets(5, &["apples", "pears"]), "- apples\n- pears\n\n"),
        citrus,
    ];
    assert_eq!(
        parse(text),
        [sourced(section(1, 1, "Fruit", members), "# Fruit\n\n")]
    );
}

#[test]
fn every_kind_of_block_takes_the_blank_lines_after_it() {
    let text = "---\na: 1\n---\n\n# H\n \nwords\n\n```\nx\n```\n\t\n- a\n\n***\n\n\nend";
    assert_eq!(
        sources(text),
        [
            "---\na: 1\n---\n\n",
            "# H\n \n",
            "words\n\n",
            "```\nx\n```\n\t\n",
            "- a\n\n",
            "***\n\n\n",
            "end",
        ]
    );
    // A block with no blank line after it holds its own lines alone.
    assert_eq!(
        sources("# H\nwords\n- a\n```\nx\n```\n***\nend\n"),
        [
            "# H\n",
            "words\n",
            "- a\n",
            "```\nx\n```\n",
            "***\n",
            "end\n"
        ]
    );
}

#[test]
fn the_blank_lines_between_two_headings_are_in_the_upper_headings_source() {
    assert_eq!(
        sources("# A\n\n\n## B\n \n# C"),
        ["# A\n\n\n", "## B\n \n", "# C"]
    );
    // No block is made for them, so the lower heading is still the first
    // member of the upper one.
    let blocks = parse("# A\n\n## B\n");
    assert_eq!(
        without_sources(&blocks),
        [section(1, 1, "A", vec![section(3, 2, "B", vec![])])]
    );
}

#[test]
fn the_blank_lines_at_the_top_are_in_the_first_blocks_source_and_its_line_is_its_own() {
    for (text, line) in [
        ("\n \n# H\n", 3),
        ("\n\nwords", 3),
        ("\t\n- a\n", 2),
        ("\n```\nx\n```\n", 2),
        ("\n\n\n***\n", 4),
    ] {
        let blocks = parse(text);
        let [block] = blocks.as_slice() else {
            panic!("{text:?} is one block");
        };
        assert_eq!((block.source(), block.line()), (text, line), "{text:?}");
    }
    // Only the first block's source starts with them.
    assert_eq!(sources("\n\n# A\n\nwords\n"), ["\n\n# A\n\n", "words\n"]);
    // A blank line above `---` leaves no frontmatter, so the rule holds it.
    assert_eq!(sources("\n---\na: 1\n---\n"), ["\n---\n", "a: 1\n---\n"]);
}

#[test]
fn text_of_blank_lines_alone_has_no_block_to_hold_them() {
    for text in ["", "\n", "\n\n  \n\t\n", "   "] {
        assert_eq!(parse(text), [], "{text:?}");
        assert_eq!(parse_body(text, 1), [], "{text:?}");
    }
    // After frontmatter they have a block above them.
    let text = "---\na: 1\n---\n\n \n";
    assert_eq!(parse(text), [sourced(frontmatter(json!({"a": 1})), text)]);
}

#[test]
fn the_last_block_has_no_line_ending_when_the_text_has_none() {
    assert_eq!(sources("# A\nwords"), ["# A\n", "words"]);
    assert_eq!(sources("# A"), ["# A"]);
    assert_eq!(sources("words  "), ["words  "]);
    assert_eq!(sources("- a\n- b"), ["- a\n- b"]);
    assert_eq!(sources("```\nx\n```"), ["```\nx\n```"]);
    assert_eq!(sources("---\na: 1\n---"), ["---\na: 1\n---"]);
    // Blank lines after it still end the text, with or without their own
    // line ending.
    assert_eq!(sources("words\n\n  "), ["words\n\n  "]);
}

#[test]
fn a_headings_source_is_every_line_of_the_heading() {
    assert_eq!(
        sources("Line one\nline two\n===\n\nbody\n"),
        ["Line one\nline two\n===\n\n", "body\n"]
    );
    // A paragraph line directly above `---` is a heading, and both lines
    // are its source.
    assert_eq!(sources("words\n---\nafter"), ["words\n---\n", "after"]);
    assert_eq!(sources("## Title ##  \n\nx"), ["## Title ##  \n\n", "x"]);
}

#[test]
fn a_source_starts_at_the_start_of_a_line_and_holds_the_whole_of_its_last_line() {
    // The indentation before a heading, a fence, a list, and a rule.
    assert_eq!(
        sources("  # H\n  ```\n  x\n  ```\n   - a\n  ***\n"),
        ["  # H\n", "  ```\n  x\n  ```\n", "   - a\n", "  ***\n"]
    );
    // The spaces after a closing fence and after a rule.
    assert_eq!(
        sources("```\nx\n```   \nafter\n***  \n"),
        ["```\nx\n```   \n", "after\n", "***  \n"]
    );
    // Prose keeps the indentation its text keeps.
    assert_eq!(
        sources("# H\n\n    code\n\n- a\n"),
        ["# H\n\n", "    code\n\n", "- a\n"]
    );
}

#[test]
fn a_fence_with_no_closing_line_holds_the_rest_of_the_text() {
    assert_eq!(
        sources("# A\n\n```lua\nreturn 1\n\n# not a heading\n\n"),
        ["# A\n\n", "```lua\nreturn 1\n\n# not a heading\n\n"]
    );
}

#[test]
fn the_source_keeps_what_the_other_fields_do_not() {
    // Each text is one block, whose fields read the same as those of a
    // block written another way.
    let cases = [
        ("3. x\n4. y\n", numbered(1, &["x", "y"])),
        ("5) x\n6) y\n", numbered(1, &["x", "y"])),
        ("1. x\n1. y\n", numbered(1, &["x", "y"])),
        ("* x\n* y\n", bullets(1, &["x", "y"])),
        ("+ x\n\n+ y\n", bullets(1, &["x", "y"])),
        ("Title\n=====\n", section(1, 1, "Title", vec![])),
        ("# Title #\n", section(1, 1, "Title", vec![])),
        (
            "## See [API](http://x) *now*\n",
            section(1, 2, "See API now", vec![]),
        ),
        (
            "---\n# a comment\nb: 2\na: 1\n---\n",
            frontmatter(json!({"a": 1, "b": 2})),
        ),
        (
            "---\n{ b: 2, a: 1 }\n---\n",
            frontmatter(json!({"a": 1, "b": 2})),
        ),
        ("~~~lua\nreturn 1\n~~~\n", fence(1, "lua", "return 1")),
        ("````lua\nreturn 1\n````\n", fence(1, "lua", "return 1")),
        ("___\n", rule(1)),
        ("- - -\n", rule(1)),
        ("*****\n", rule(1)),
    ];
    for (text, fields) in cases {
        assert_eq!(parse(text), [sourced(fields, text)], "{text:?}");
    }
}

#[test]
fn a_source_is_cut_from_the_text_as_it_was_read() {
    // Without the byte order mark, and with `\n` for every line ending.
    assert_eq!(
        sources("\u{feff}# T\r\n\r\nwords\r\n- a\r- b\r"),
        ["# T\n\n", "words\n", "- a\n- b\n"]
    );
    assert_eq!(
        sources("---\r\na: 1\r\n---\r\n\r\nwords"),
        ["---\na: 1\n---\n\n", "words"]
    );
}

#[test]
fn a_bodys_sources_are_the_body() {
    let own = "\n\nFill in this outline.\n\n```markdown\n# Title\n\n## Summary\n```";
    let found: Vec<String> = parse_body(own, 23)
        .iter()
        .map(|block| block.source().to_owned())
        .collect();
    assert_eq!(
        found,
        [
            "\n\nFill in this outline.\n\n",
            "```markdown\n# Title\n\n## Summary\n```"
        ]
    );
    // What opens like frontmatter is ordinary Markdown in a body.
    let found: Vec<String> = flat(&parse_body("---\na: b\n---\n\nx", 1))
        .into_iter()
        .map(|block| block.source().to_owned())
        .collect();
    assert_eq!(found, ["---\n", "a: b\n---\n\n", "x"]);
}

/// The bytes the leading blank lines of `source` take.
fn blank_above(source: &str) -> usize {
    source
        .split_inclusive('\n')
        .take_while(|line| line.trim_matches(BLANK).is_empty())
        .map(str::len)
        .sum()
}

/// `block` by its own fields alone: no line, no source, and no blocks
/// under it.
fn own_fields(block: &Block) -> Block {
    match sourced(block.clone(), "") {
        Block::Frontmatter { data, .. } => frontmatter(data),
        Block::Section { level, title, .. } => section(0, level, &title, vec![]),
        Block::Prose { text, .. } => prose(0, &text),
        Block::Fence {
            lang, info, text, ..
        } => Block::Fence {
            line: 0,
            source: String::new(),
            lang,
            info,
            text,
        },
        Block::List { ordered, items, .. } => Block::List {
            line: 0,
            source: String::new(),
            ordered,
            items,
        },
        Block::Rule { .. } => rule(0),
    }
}

/// Asserts everything a parse promises of its sources, for `blocks` read
/// from `text`, whose line endings are all `\n` and whose first line is
/// line `first_line`. `context` names the text in a failure.
#[track_caller]
fn assert_sources(text: &str, blocks: &[Block], first_line: usize, context: &str) {
    let found = flat(blocks);
    if text.trim_matches(BLANK).is_empty() {
        assert!(found.is_empty(), "{context}: blank text has no block");
        return;
    }
    // The sources are the text: nothing is missing, nothing is in two.
    let joined: String = found.iter().map(|block| block.source()).collect();
    assert_eq!(joined, text, "{context}: the sources are the text");
    let mut offset = 0;
    let mut before: Option<&Block> = None;
    for (at, block) in found.iter().copied().enumerate() {
        let source = block.source();
        let name = format!("{context}: block {at}, {source:?}");
        let start = offset;
        offset += source.len();
        let odd = before.is_some_and(|before| continues_a_paragraph(before, block));
        // The spacing a writer adds between two prose blocks never applies
        // inside one parse.
        let both =
            matches!(before, Some(Block::Prose { .. })) && matches!(block, Block::Prose { .. });
        assert!(!both, "{name} is prose right after prose");
        before = Some(block);
        // Each is whole lines, so each starts at the start of a line.
        if at + 1 < found.len() {
            assert!(source.ends_with('\n'), "{name} ends with a line ending");
        }
        let above = blank_above(source);
        assert!(above < source.len(), "{name} holds a line of its own");
        if odd {
            continue;
        }
        // Blank lines are above a block's own only at the top of the text.
        if at > 0 {
            assert_eq!(above, 0, "{name} starts with its own line");
        }
        // The block's line is the line its own first line is on.
        let line = first_line + text[..start + above].matches('\n').count();
        assert_eq!(block.line(), line, "{name} is on its line");
        // Prose is its source without the blank lines around it.
        if let Block::Prose { text: words, .. } = block {
            assert_eq!(words, source[above..].trim_end_matches(BLANK), "{name}");
        }
        // The source alone reads as this block and nothing else.
        assert_reads_alone(block, &name);
    }
}

/// Whether `block` is a heading whose text lines continue the paragraph
/// that `before` holds the opening of: an underlined heading right below
/// prose, with no blank line between. Only link definitions open a
/// paragraph without being part of what it becomes, and the library
/// reads what follows them in its own way: a line of spaces above `---`
/// is the text of a heading there. The caller checks of such a heading
/// only what holds for it.
fn continues_a_paragraph(before: &Block, block: &Block) -> bool {
    let underline = block
        .source()
        .lines()
        .rev()
        .map(|line| line.trim_matches(BLANK))
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    let underlined = ["=", "-"]
        .iter()
        .any(|mark| !underline.is_empty() && underline.replace(mark, "").is_empty());
    let parted = before
        .source()
        .split_inclusive('\n')
        .next_back()
        .is_some_and(|line| line.trim_matches(BLANK).is_empty());
    matches!(block, Block::Section { .. })
        && underlined
        && matches!(before, Block::Prose { .. })
        && !parted
}

/// Asserts that the source of `block`, read alone, is `block` and nothing
/// else, by every field but its line. `name` names the block in a failure.
///
/// A section's title is compared only when its source holds no `[`: a
/// title may use a link that the rest of the text defines. The caller
/// leaves out a heading that continues a paragraph, which
/// `a_heading_below_a_link_definition_is_the_one_block_its_source_alone_is_not`
/// states.
#[track_caller]
fn assert_reads_alone(block: &Block, name: &str) {
    let source = block.source();
    let alone = match block {
        Block::Frontmatter { .. } => parse(source),
        _ => parse_body(source, 1),
    };
    let alone = flat(&alone);
    let [only] = alone.as_slice() else {
        panic!("{name} reads alone as {} blocks", alone.len());
    };
    match (only, block) {
        (Block::Section { level: alone, .. }, Block::Section { level, .. })
            if source.contains('[') =>
        {
            assert_eq!(alone, level, "{name} reads alone");
        }
        _ => assert_eq!(own_fields(only), own_fields(block), "{name} reads alone"),
    }
}

#[test]
fn a_heading_below_a_link_definition_is_the_one_block_its_source_alone_is_not() {
    // A link definition opens a paragraph and is no part of what the
    // paragraph becomes, so the heading starts on the line after it. That
    // line continues a paragraph here, and alone it starts a list.
    let text = "[ref]: http://x\n10. ten\n=====\n";
    let blocks = parse(text);
    assert_eq!(
        blocks,
        [
            sourced(prose(1, "[ref]: http://x"), "[ref]: http://x\n"),
            sourced(section(2, 1, "10. ten", vec![]), "10. ten\n=====\n"),
        ]
    );
    assert_eq!(
        without_sources(&parse("10. ten\n=====\n")),
        [numbered(1, &["ten\n====="])]
    );
    // The two sources together read as the two blocks.
    let joined: String = blocks.iter().map(Block::source).collect();
    assert_eq!(parse(&joined), blocks);
}

#[test]
fn a_title_reads_a_link_that_the_rest_of_the_text_defines() {
    // With the definition in the text the brackets are a link, and with
    // the heading's source alone they are text.
    let definition = sourced(prose(3, "[ref]: http://x"), "[ref]: http://x\n");
    assert_eq!(
        parse("# See [ref]\n\n[ref]: http://x\n"),
        [sourced(
            section(1, 1, "See ref", vec![definition]),
            "# See [ref]\n\n"
        )]
    );
    assert_eq!(
        without_sources(&parse("# See [ref]\n\n")),
        [section(1, 1, "See [ref]", vec![])]
    );
}

#[test]
fn the_sources_of_every_sample_are_its_text_cut_into_whole_blocks() {
    assert_sources(GUIDE, &parse(GUIDE), 1, "the guide");
    for body in BODIES {
        assert_sources(body, &parse(body), 1, &format!("{body:?}"));
        assert_sources(body, &parse_body(body, 7), 7, &format!("body {body:?}"));
    }
}

#[test]
fn the_sources_of_generated_texts_are_each_text_cut_into_whole_blocks() {
    let mut rng = Seeded::new(SEED);
    let mut seen = [0_usize; Kind::ALL.len()];
    for number in 0..2000 {
        let text = rng.text();
        let context = format!("seed {SEED:#x}, text {number}, {text:?}");
        let parsed = parse(&text);
        for block in flat(&parsed) {
            seen[block.kind() as usize] += 1;
        }
        assert_sources(&text, &parsed, 1, &context);
        assert_sources(&text, &parse_body(&text, 7), 7, &format!("body, {context}"));
    }
    // The texts hold every kind of block, each many times over.
    for kind in Kind::ALL {
        let count = seen[kind as usize];
        assert!(
            count >= 100,
            "seed {SEED:#x}: {count} {} blocks",
            kind.name()
        );
    }
}
