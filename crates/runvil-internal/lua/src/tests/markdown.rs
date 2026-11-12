//! The `markdown` global: `markdown.parse` over text, `markdown.section`
//! and `markdown.section_or` over a path and a selector,
//! `markdown.select`, `markdown.render`, and the same functions over
//! blocks read back from `var`.

use super::*;

mod parse;
mod render;
mod section;
mod select;
mod views;

/// A document with every kind of block: frontmatter, prose before the
/// first heading, three levels of section, both kinds of list, fences
/// with and without extra info words, and a rule.
const GUIDE: &str = r#"---
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

/// A document whose two `Entry` sections share a parent and a title.
const LOG: &str = "# Log\n\n## Entry\n\nfirst\n\n## Entry\n\nsecond\n";

/// A document whose two `Items` sections sit under different parents.
const ITEMS: &str = "# H1\n\n## H2\n\n### Items\n\n- a\n- b\n\n## H3\n\n### Items\n\n- c\n";

/// A document whose outer and inner section share the title `A`, and
/// whose two `B` sections sit at different depths.
const NESTED: &str = "# A\n## A\n### B\n## B\n";

/// A test VM that binds each sample's parse: `md` for [`GUIDE`], with
/// `guide`, `templates`, and `usage` naming its sections, `log` for
/// [`LOG`], `items` for [`ITEMS`], and `nested` for [`NESTED`].
#[track_caller]
fn markdown_vm() -> BuiltVm {
    let vm = TestVm::new().shims().build();
    bind_samples(&vm);
    vm
}

/// Binds the samples of [`markdown_vm`] on `vm`.
#[track_caller]
fn bind_samples(vm: &BuiltVm) {
    let globals = vm.lua().globals();
    for (name, text) in [
        ("guide_text", GUIDE),
        ("log_text", LOG),
        ("items_text", ITEMS),
        ("nested_text", NESTED),
    ] {
        globals.raw_set(name, text).expect("the sample binds");
    }
    vm.exec(
        "md = markdown.parse(guide_text)\n\
         guide = md[3]\n\
         templates = guide.blocks[3]\n\
         usage = guide.blocks[4]\n\
         log = markdown.parse(log_text)\n\
         items = markdown.parse(items_text)\n\
         nested = markdown.parse(nested_text)",
    );
}

/// Asserts that each Lua expression gives the value its JSON text names,
/// where `nil` stands for a nil result.
#[track_caller]
fn assert_values(vm: &BuiltVm, cases: &[(&str, &str)]) {
    for (expression, want) in cases {
        let got: String = vm.eval(&format!(
            "local value = {expression}\n\
             if value == nil then return 'nil' end\n\
             return json.encode(value)"
        ));
        assert_eq!(got, *want, "{expression}");
    }
}

/// Asserts that each Lua expression raises a `lua` error whose text is
/// the message beside it.
#[track_caller]
fn assert_raises(vm: &BuiltVm, cases: &[(&str, &str)]) {
    for (expression, message) in cases {
        assert_eq!(caught(vm, expression), lua_error(message), "{expression}");
    }
}
