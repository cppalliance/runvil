//! Block constructors, sample texts, and seeded generated texts for this
//! crate's tests.

use serde_json::Value as Json;

use crate::Block;

/// A frontmatter block on line 1, with no source.
pub(crate) fn frontmatter(data: Json) -> Block {
    Block::Frontmatter {
        line: 1,
        source: String::new(),
        data,
    }
}

/// A section block, with no source.
pub(crate) fn section(line: usize, level: u8, title: &str, blocks: Vec<Block>) -> Block {
    Block::Section {
        line,
        source: String::new(),
        level,
        title: title.to_owned(),
        blocks,
    }
}

/// A prose block, with no source.
pub(crate) fn prose(line: usize, text: &str) -> Block {
    Block::Prose {
        line,
        source: String::new(),
        text: text.to_owned(),
    }
}

/// A fence block whose language is the first word of `info`, with no
/// source.
pub(crate) fn fence(line: usize, info: &str, text: &str) -> Block {
    Block::Fence {
        line,
        source: String::new(),
        lang: info
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_owned(),
        info: info.to_owned(),
        text: text.to_owned(),
    }
}

/// A bullet list block, with no source.
pub(crate) fn bullets(line: usize, items: &[&str]) -> Block {
    Block::List {
        line,
        source: String::new(),
        ordered: false,
        items: items.iter().map(|item| (*item).to_owned()).collect(),
    }
}

/// A numbered list block, with no source.
pub(crate) fn numbered(line: usize, items: &[&str]) -> Block {
    Block::List {
        line,
        source: String::new(),
        ordered: true,
        items: items.iter().map(|item| (*item).to_owned()).collect(),
    }
}

/// A rule block, with no source.
pub(crate) fn rule(line: usize) -> Block {
    Block::Rule {
        line,
        source: String::new(),
    }
}

/// `block` with `source` as its source.
pub(crate) fn sourced(mut block: Block, source: &str) -> Block {
    match &mut block {
        Block::Frontmatter { source: own, .. }
        | Block::Section { source: own, .. }
        | Block::Prose { source: own, .. }
        | Block::Fence { source: own, .. }
        | Block::List { source: own, .. }
        | Block::Rule { source: own, .. } => source.clone_into(own),
    }
    block
}

/// `blocks` with every source emptied, for a test that states a parse by
/// its other fields, as the constructors above build them.
pub(crate) fn without_sources(blocks: &[Block]) -> Vec<Block> {
    blocks
        .iter()
        .cloned()
        .map(|block| match sourced(block, "") {
            Block::Section {
                line,
                source,
                level,
                title,
                blocks,
            } => Block::Section {
                line,
                source,
                level,
                title,
                blocks: without_sources(&blocks),
            },
            leaf => leaf,
        })
        .collect()
}

/// Every block of `blocks` in file order, a section before the blocks
/// under it.
pub(crate) fn flat(blocks: &[Block]) -> Vec<&Block> {
    let mut found = Vec::new();
    for block in blocks {
        found.push(block);
        if let Block::Section { blocks, .. } = block {
            found.extend(flat(blocks));
        }
    }
    found
}

/// The seed every generated text in this crate's tests starts from. A
/// failure names it.
pub(crate) const SEED: u64 = 0x5EED;

/// A `SplitMix64` generator over a fixed seed, so a generated text is the
/// same on every run.
pub(crate) struct Seeded(u64);

impl Seeded {
    /// A generator that starts from `seed`.
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let z = (self.0 ^ (self.0 >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        let z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A draw in `0..n`.
    pub(crate) fn below(&mut self, n: usize) -> usize {
        let draw = self.next_u64() % u64::try_from(n).expect("a usize fits in a u64");
        usize::try_from(draw).expect("a draw below a usize is a usize")
    }

    /// A text of up to 24 lines drawn from [`LINES`], about one in three
    /// of them blank. One text in eight opens with frontmatter, and one
    /// in four has no final line ending.
    pub(crate) fn text(&mut self) -> String {
        let mut lines = Vec::new();
        if self.below(8) == 0 {
            lines.extend(["---", "a: 1", "---"]);
        }
        for _ in 0..self.below(25) {
            lines.push(if self.below(3) == 0 {
                ""
            } else {
                LINES[self.below(LINES.len())]
            });
        }
        let mut text = lines.join("\n");
        if !lines.is_empty() && self.below(4) != 0 {
            text.push('\n');
        }
        text
    }
}

/// The lines a generated text is made of. Between them they open,
/// continue, and close every construct the parser tells apart, and many
/// read differently after different lines: `---` is a rule, a heading's
/// underline, or a frontmatter delimiter, and an indented line is code,
/// a paragraph's next line, or part of a list item.
const LINES: &[&str] = &[
    "   ",
    "\t",
    "# One",
    "## Two ##",
    "### Three",
    "###### Six",
    "#",
    "  ## Indented",
    "#NotHeading",
    "## See [API](x) *now*",
    "Title",
    "=====",
    "---",
    "***",
    "___",
    " - - -",
    "A paragraph line.",
    "another line  ",
    "ends with a backslash\\",
    "h\u{e9}llo \u{2014} w\u{f6}rld",
    "\u{a0}",
    "```",
    "```lua",
    "~~~",
    "~~~~",
    "````md",
    "  ``` py",
    "    indented code",
    "- a",
    "- b",
    "* c",
    "+ d",
    "1. one",
    "2. two",
    "3) three",
    "10. ten",
    "  - nested",
    "   continued",
    "-",
    "- [ ] todo",
    "- # heading in an item",
    "> quote",
    "> # quoted heading",
    "> - quoted item",
    ">",
    "<div>",
    "</div>",
    "<!--",
    "-->",
    "[ref]: http://example.com",
    "| a | b |",
    "|---|---|",
    "a: b",
    "key: [1, 2]",
];

/// A document with every kind of block: frontmatter, prose before the
/// first heading, three levels of section, both kinds of list, fences
/// with and without extra info words, and a rule.
pub(crate) const GUIDE: &str = r#"---
title: Notes
tags: [a, b]
---

Preface paragraph.

# Guide

Intro text
on two lines.

- alpha
- beta

## Templates

Use these.

```lua
return 1
```

```markdown
# Hello
```

Then the fallback:

```json strict
{ "ok": true }
```

### Extras

---

```lua
return 2
```

## Usage

1. first
2. second
"#;

/// Texts that between them hold every construct the parser tells apart,
/// in the arrangements that are easy to get wrong. None has a `\r`, and
/// none opens with frontmatter.
pub(crate) const BODIES: &[&str] = &[
    "",
    "\n\n   \n",
    "one paragraph",
    "first\n\nsecond\n",
    "Title\n=====\n\ntext\n\nSub\n---\n\nmore\n",
    "Line one\nline two\n===\n\nbody\n",
    "## The *big* `foo` [link](http://x) API ##\n\ntext\n",
    "##\n\ntext\n\n#   \n\nmore\n",
    "> # Quoted\n\n- # Item heading\n- two\n\n# Real\n",
    "## A\n\n#### B\n\nb\n\n### C\n\n# D\n\n###### E\n",
    "### A\n\na\n\n# B\n\nb\n",
    "text\n- a\n- b\n",
    "text\n```\ncode\n```\nafter\n",
    "- a\n\n- b\n\n- c\n",
    "- first line\n  second line\n\n  second paragraph\n- next\n",
    "- a\n  - a1\n  - a2\n- b\n  1. b1\n",
    "1. step\n\n   ```sh\n   run\n   ```\n2. done\n",
    "-\n- b\n-\n  later\n",
    "5) five\n6) six\n",
    "- a\n- b\n\n* c\n* d\n\n1. e\n\n1) f\n",
    "  - a\n  - b\n",
    "- a\n\tcontinued\n- b\n",
    "- [ ] todo\n- [x] done\n",
    "    code line\n\ntext\n\n<div>\nraw\n</div>\n\n- x\n",
    "```\na\n```\n\n[ref]: http://example.com\n\n---\n",
    "> - a\n> - b\n>\n> ```\n> code\n> ```\n\nafter\n",
    "~~~\nplain\n~~~\n\n```\nbare\n```\n\n``` rust ignore \nfn x() {}\n```\n",
    "# A\n\n```lua\nreturn 1\n\n# not a heading\n",
    "  ```py\n  x = 1\n    y = 2\n  ```\n",
    "```json\n```\n\n```\n\n```\n",
    "````md\n```lua\nx\n```\n````\n",
    "```\na\n\n```\n",
    "***\n\n___\n\n- - -\n\ntext\n",
    "text\n---\n\nafter\n",
    "# H\n```\ncode\n```\n",
    "<!--\n# not a heading\n-->\n\ntext\n",
    "+ a\n+ b\n",
    "- a\n\n<!-- -->\n\n- b\n",
    "| a | b |\n|---|---|\n| 1 | 2 |\n",
    "- ---\n- x\n",
    "text  \nmore\\\nend\n",
    "1. a\n\n   b\n\n2. c\n",
    "- a\n\n  ```\n  unclosed in item\n\ntext after\n",
    "## A\n## B\n",
    "#NotHeading\n\n#\tTab heading\n",
    "```\n```\n```\n",
    "    indented code only\n",
    "[ref]: http://x\n",
    "- Title\n  ===\n",
    "***bold***\n\n* * *\n",
    "1. a\n1. b\n1. c\n",
    "- a\n- b\n\n\n- c\n",
    "\\# not heading\n",
    "  # indented heading\n",
    "     # five spaces\n",
    "```lua\nreturn 1\n```\ntrailing text\n",
    "text\n\n---\n\n- a\n\n---\n",
    "- a\n\n      code in item\n- b\n",
    "> quote\n\n- a\n\n> quote\n",
    "1. one\n- bullet\n2. two\n",
    "- a\n\n\n\ntext\n",
    "## Title ##\n\n### C#\n\n#### A #\n",
    "- [link]\n\n[link]: http://x\n",
    "text\n```\nfence\n```\n- list\n***\n# H\n",
    "- a\n   - odd indent\n  - b\n",
    "10. ten\n11. eleven\n",
    "-\n  item after blank\n-   \n",
    "```\n\n\n```\n",
    "~~~~\n~~~\n~~~~\n",
    "<div>\n\n# inside html?\n\n</div>\n",
    "~~~ a`b\nx\n~~~\n",
    "1. a\n2. b\n3. c\n4. d\n5. e\n6. f\n7. g\n8. h\n9. i\n10. j\n    more\n",
    "# A\n## A\n### B\n## B\n",
    "# Log\n\n## Entry\n\nfirst\n\n## Entry\n\nsecond\n",
];
