//! Block constructors and sample texts for this crate's tests.

use serde_json::Value as Json;

use crate::Block;

/// A frontmatter block on line 1.
pub(crate) fn frontmatter(data: Json) -> Block {
    Block::Frontmatter { line: 1, data }
}

/// A section block.
pub(crate) fn section(line: usize, level: u8, title: &str, blocks: Vec<Block>) -> Block {
    Block::Section {
        line,
        level,
        title: title.to_owned(),
        blocks,
    }
}

/// A prose block.
pub(crate) fn prose(line: usize, text: &str) -> Block {
    Block::Prose {
        line,
        text: text.to_owned(),
    }
}

/// A fence block whose language is the first word of `info`.
pub(crate) fn fence(line: usize, info: &str, text: &str) -> Block {
    Block::Fence {
        line,
        lang: info
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_owned(),
        info: info.to_owned(),
        text: text.to_owned(),
    }
}

/// A bullet list block.
pub(crate) fn bullets(line: usize, items: &[&str]) -> Block {
    Block::List {
        line,
        ordered: false,
        items: items.iter().map(|item| (*item).to_owned()).collect(),
    }
}

/// A numbered list block.
pub(crate) fn numbered(line: usize, items: &[&str]) -> Block {
    Block::List {
        line,
        ordered: true,
        items: items.iter().map(|item| (*item).to_owned()).collect(),
    }
}

/// A rule block.
pub(crate) fn rule(line: usize) -> Block {
    Block::Rule { line }
}

/// `blocks` with every line set to zero, for comparing two parses whose
/// lines differ.
pub(crate) fn without_lines(blocks: &[Block]) -> Vec<Block> {
    blocks
        .iter()
        .cloned()
        .map(|block| match block {
            Block::Frontmatter { data, .. } => Block::Frontmatter { line: 0, data },
            Block::Section {
                level,
                title,
                blocks,
                ..
            } => Block::Section {
                line: 0,
                level,
                title,
                blocks: without_lines(&blocks),
            },
            Block::Prose { text, .. } => Block::Prose { line: 0, text },
            Block::Fence {
                lang, info, text, ..
            } => Block::Fence {
                line: 0,
                lang,
                info,
                text,
            },
            Block::List { ordered, items, .. } => Block::List {
                line: 0,
                ordered,
                items,
            },
            Block::Rule { .. } => Block::Rule { line: 0 },
        })
        .collect()
}

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
