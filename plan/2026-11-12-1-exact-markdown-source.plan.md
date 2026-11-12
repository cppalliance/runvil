---
name: Exact Markdown Source
overview: Every parsed block keeps the exact text it came from, the blank lines after it included, so joining the blocks gives back the document. `markdown.render` becomes that join plus two spacing rules, the writer that rebuilt text in a fixed style is deleted, and the guide and design are brought in line.
todos:
  - id: plan-copy
    content: "runvil: on `master`, copy this plan to `plan/2026-11-12-1-exact-markdown-source.plan.md` before any other change"
    status: completed
  - id: markdown-crate
    content: "runvil-markdown: `source` on every block, blank lines given to the block above, the `*` selector, the fixed-style writer replaced by a writer with two spacing rules; corpus, generated, edge, spacing, and regression tests"
    status: completed
  - id: lua-global
    content: "runvil-lua: tables carry `source`, `markdown.render` feeds the crate's writer, `*` in the lookups and the prompt reads, the field reader deleted; tests updated and added"
    status: completed
  - id: runtime-tests
    content: "runvil-runtime: a stored file parses and renders back equal, also through `var`"
    status: completed
  - id: verify
    content: "runvil: run every check in AGENTS.md"
    status: completed
  - id: design-docs
    content: "runvil-design: bring the design document and the language spec in line, one commit"
    status: completed
  - id: guide
    content: "runvil-docs: bring the guide's markdown section, selector table, and quick reference in line with examples run against the build, one commit"
    status: completed
  - id: commit
    content: "runvil: mark the plan copy's todos done, record in it what the build found, and make the one commit on `master` with the `Plan:` trailer"
    status: completed
isProject: false
---

# Exact source for parsed Markdown

## The design

- **`source` on every block.** It holds the exact lines the block was parsed from, each with its line ending.
- **Blank lines belong to the block above them.** A block's `source` runs from its first line to the first line of the next block. So a section's `source` is its heading and the blank lines after it. Blank lines at the very top of the text belong to the first block. A blank line holds only spaces and tabs.
- **The mapping is total.** Joining every `source` in file order, a section before the blocks under it, gives back the text. So `markdown.render(markdown.parse(text)) == text`. The one exception is text that holds nothing but blank lines: it has no block, so it parses to an empty list, as today.
- **`markdown.render` writes each block's `source`** and rebuilds nothing. The writer that rebuilt text from `title`, `items`, and `text` is deleted. Renumbered lists and the review finding about fences go with it.
- **The flat view is `markdown.select(blocks, "*")`.** `*` matches every block. It returns a new array of the same block tables, in file order.
- **Plain, editable data, as today.** `kind`, `line`, `title`, `text`, `items`, `lang`, `info`, and `data` do not change, and no kind is added.

````lua
-- "# Fruit\n\nSome words.\n\n- apples\n- pears\n\n## Citrus\n\n### Lemon\n"
{ kind = "section", title = "Fruit", source = "# Fruit\n\n", blocks = {
  { kind = "prose", text = "Some words.", source = "Some words.\n\n" },
  { kind = "list", items = { "apples", "pears" }, source = "- apples\n- pears\n\n" },
  { kind = "section", title = "Citrus", source = "## Citrus\n\n", blocks = {
    { kind = "section", title = "Lemon", source = "### Lemon\n", blocks = {} },
  } },
} }
````

Decisions I made where you left room:

- **Byte order mark and line endings stay as today:** the mark is dropped and `\r\n` and `\r` are read as `\n`. The join is the text with those two changes. This needs no new code and keeps every string in a parse `\n`-only.
- **`line` keeps its meaning:** the heading line, the opening fence line, the first marker, the first non-blank line of prose. That holds for a first block with blank lines above it too.

## The spacing `render` adds

`render` writes a line ending of its own in two cases. Neither can occur in an untouched parse, so `render(parse(text))` stays exact.

1. **Every block starts on a new line.** When what is written so far does not end with a line ending, one is written before the next block. In a parse only the last block can lack one.
2. **A blank line sits between two prose blocks.** When a prose block follows a prose block and what is written so far does not end with a blank line, one is written. In a parse two prose blocks are never neighbors, because prose is a maximal run.

These cover the last block of a text placed before another block, a `source` edited without its line ending, and two paragraphs that would otherwise read as one.

`render` does not decide in general whether two blocks need a blank line, because that depends on their text. A paragraph directly above `- a` needs none. Directly above `2. b` it swallows the list, and directly above `---` it becomes a heading. Adding a blank line everywhere would change text written without one, such as a sentence directly above its list, which is common. So these stay the author's to get right:

- An edited `source` that drops its blank line before a `---` rule, before a list that does not start at 1, or after a list.
- Two lists with the same marker, which read as one list with or without a blank line between them.

A later version could ask the parser about each such pair. That would be a change to `render` alone, with no change to the data.

## What this changes for an author

- `markdown.render(section.blocks)` is the section's body from its first line of content, with the blank lines that followed it in the text, such as `"Short and plain.\n\n"`. A block's `text` is still trimmed.
- Editing `title`, `items`, or `text` changes nothing that is written. Editing `source` does.
- A block built by hand needs `source`. Without it, `render` raises `markdown.render: blocks[2].source must be a string, got nil`.
- A section's `source` holds the blank lines after its heading, so a replacement heading is written `"## New\n\n"`.
- Counting by position does not change, because no block is added for blank lines.
- `table.concat` over the sources of the flat view is the raw join. `render` differs from it only where it adds a line ending.
- `markdown.render(markdown.select(md, "*"))` writes nested blocks twice, because `render` goes into each section itself. `markdown.render(md)` is the one-call form.
- A parse holds each block's text twice, in `source` and in its fields.

## The plan copy

- First thing, on `master` of `runvil`, this plan is copied to `plan/2026-11-12-1-exact-markdown-source.plan.md`, named as that directory's other plans are.
- The copy is part of the one commit. A later commit could not update it, so before the commit its todos are marked done and it gains a section on what the build found, if anything differed from this plan.
- The commit message ends with `Plan: /plan/2026-11-12-1-exact-markdown-source.plan.md`, the trailer `runvil`'s commits use.

## Code: one commit in `runvil`, on #210's branch

`runvil-markdown`:

- [block.rs](../crates/runvil-internal/markdown/src/block.rs): `source: String` on every variant, and `Block::source()`.
- [parse/flat.rs](../crates/runvil-internal/markdown/src/parse/flat.rs): the one real change in the parser. A block starts at the first line of its construct, or at the first non-blank line of the prose between two constructs, and its `source` runs to the start of the next block. The first block also takes the blank lines above it. The ranges are consecutive, so the mapping is total by construction.
- [parse/frontmatter.rs](../crates/runvil-internal/markdown/src/parse/frontmatter.rs), [parse/tree.rs](../crates/runvil-internal/markdown/src/parse/tree.rs), [parse.rs](../crates/runvil-internal/markdown/src/parse.rs): the frontmatter's range grows over the blank lines after it, and a heading's `source` is carried into its section.
- [selector.rs](../crates/runvil-internal/markdown/src/selector.rs): `Selector::Any` for `*`. The unknown-kind message names `*`.
- [render.rs](../crates/runvil-internal/markdown/src/render.rs): rewritten, from about 150 lines to about 40. It holds a small writer that takes one block's kind and `source` at a time and applies the two spacing rules, and `render` over blocks built on it. The Lua global feeds the same writer, so the rules live in one place.
- [lib.rs](../crates/runvil-internal/markdown/src/lib.rs) states the invariant: the sources of a parse join back into the text.

`runvil-lua`:

- [markdown/blocks.rs](../crates/runvil-internal/lua/src/markdown/blocks.rs): each table gets `source`. The reader that checked every field for the old writer is deleted, about 130 lines.
- [markdown/lookup.rs](../crates/runvil-internal/lua/src/markdown/lookup.rs) and [markdown.rs](../crates/runvil-internal/lua/src/markdown.rs): `render` walks the tables under the same depth, cycle, and size limits as the lookups, and feeds each block's `kind` and `source` to the crate's writer. `*` matches a block whatever its `kind`.
- [prompt_reads.rs](../crates/runvil-internal/lua/src/prompt_reads.rs): no logic change. The block `prompt.section(path, selector)` returns carries `source`, and `*` works there.

## Tests

`runvil-markdown`:

- **The join, over the corpus.** For the guide sample and all 75 texts of `BODIES`, each also with `\r\n` endings and with a byte order mark, `render` of the parse is the text. Checked for `parse` and `parse_body`. Text of only blank lines is checked as the stated exception.
- **The join, over generated text.** 2,000 documents assembled from line fragments by a fixed seed that a failure prints: headings of both forms, blank runs with spaces and tabs, paragraphs, open and closed fences, lists of every marker and start number, rules, block quotes, HTML, and frontmatter look-alikes.
- **The shape rules, on both sets.** Every `source` is whole lines. Only the first block's `source` can start with a blank line. Every block but the last ends with a line ending, and no two prose blocks are neighbors. Those last two facts are what make the spacing rules safe.
- **One case per edge:** blank lines after each kind of block, after a heading, between two headings, at the top, at the end, only blank lines, no final line ending, and after frontmatter.
- **The spacing rules.** Each fires where it should: the last block of a text before another block, a `source` with no line ending, two prose blocks with no blank line between, and a section whose last block is prose followed by prose. Neither fires where it should not: two prose blocks that already have a blank line, prose directly above a list, a heading directly above prose, and two lists.
- **What the fields lose stays in `source`:** `3.` and `4.`, `5)`, `*` bullets, `~~~`, an underlined heading, `___`, frontmatter comments and key order, and `## See [API](x)`.
- **The review finding:** a `~~~` fence whose label is a tilde and a backtick, followed by a heading and a paragraph, joins back and the later blocks survive.
- **Selectors:** `*` and the messages.

`runvil-lua` and `runvil-runtime`:

- `markdown.render(markdown.parse(text)) == text` for each sample, and `table.concat` over the sources of `markdown.select(md, "*")` gives the same string.
- `render` reads `source`, and `kind` only for the spacing rules: an edit of `source` is written, an edit of `title` or `items` is not, a hand-built block with `source` is written, and a missing `source` raises and names the nested block.
- The spacing rules through the Lua global: a document with its sections reordered, a selection of prose blocks, and an edited `source`.
- The join survives `var` and a `json.encode` and `json.decode` round trip.
- The refusals that remain: not a list, a section given as a list, an item that is no table, a list that contains itself, too deep, too large.
- In a running prompt: a stored file is parsed and rendered back equal to the file, in the same section and in a later one through `var`.
- Existing tests that pin JSON shapes or the old writer are updated or removed with it. Tests that index by position keep their indices.

## Documents: one commit each

They are written once the code passes its checks, and committed in their own repositories before the `runvil` commit, so the plan copy can record them as done. They are pushed after it.

- **`runvil-design`:** [the design](https://github.com/cppalliance/runvil-design/blob/master/design/2026-11-11-1-markdown-and-prompt-reads.md) (block model, parsing rules, worked example, edge cases, selectors, `render`, identity, bytes copied, tests to carry over) and [the language spec](https://github.com/cppalliance/runvil-design/blob/master/design/architect-runvil-prompting-language.md). The design is a dated record, so the fixed-style writer and a separate kind for blank lines go under rejected alternatives with their reasons, and the general spacing rule goes under open questions. The spec describes the language as it is.
- **`runvil-docs`:** the `markdown` section of [05-lua-environment.md](https://github.com/cppalliance/runvil-docs/blob/master/src/language/05-lua-environment.md), the selector table in [03-blocks-and-prose.md](https://github.com/cppalliance/runvil-docs/blob/master/src/language/03-blocks-and-prose.md), and two rows of [17-quick-reference.md](https://github.com/cppalliance/runvil-docs/blob/master/src/language/17-quick-reference.md). Every changed example value comes from a run of the built code.

## Git and pull requests

These steps follow the commit, in this order. They are not todos, because the plan copy is committed before they happen.

1. The commit goes on `master` of your fork, which is the head of [#210](https://github.com/cppalliance/runvil/pull/210), and is pushed there.
2. `bash-pack` is rebased onto it, the Workshop checks run again, and it is pushed with `--force-with-lease`, which updates [#211](https://github.com/cppalliance/runvil/pull/211). Your checkout is left on `bash-pack`, as it is now.
3. `runvil-docs` and `runvil-design` are pushed to `master`. In `runvil-design` that also publishes your unpushed commit `cc16539`, "Add Why Runvil two pagers".
4. I watch CI on both pull requests, then reply on [the review thread](https://github.com/cppalliance/runvil/pull/211#discussion_r4549082619) with the commit and resolve it.

If CI on #210 finds something, the fix is amended into the same commit and `master` is pushed with `--force-with-lease`, so #210 still gains one commit. `bash-pack` is then rebased and pushed again.

## Verification

The commands in `runvil`'s `AGENTS.md`, all of them, before the commit: format, both clippy sets, both nextest sets, docs, `cargo test -p build-xtask`, the wording check, `xtask api --check`, and dylint.

## What the build found

The design held. These points differ from the plan above, or add to it.

- **A list ends with its last item.** `pulldown-cmark` ends a list where the next block starts. That is past any link definition that follows the list, and past the indentation of the next paragraph. It ends a list item where the item ends. Before this change a link definition after a list was in no field of any block, and with sources it made the prose after a list start in the middle of a line. The range of a list is now cut to the end of its last item, in `parse/events.rs`. So `- a`, a blank line, `[ref]: http://x`, and `text` parse to a list and one prose block that holds the last two lines, where the prose held only `text` before. This changes what the fields hold, and the plan did not name it.
- **Each source is checked alone.** The plan listed the shape rules. The tests check one more thing: a block's `source`, parsed alone, is that one block again. That check is what found the list ranges. Two cases are left out of it, and each has a test that states it. A title may use a link that the rest of the text defines. And an underlined heading right below a link definition continues a paragraph, so its lines alone can be another block. In that same place the library reads a line of spaces above `---` as the heading's text, so that one `source` starts with a blank line. The cases were found by running the checks once over generated texts by the million. The final checks passed over 1.5 million texts from three seeds, and the committed tests run 2,000 from one.
- **The second spacing rule looks at both sides.** A blank line is added between two prose blocks only when neither the text so far ends with one nor the block starts with one. The plan named only the first.
- **A block with an empty `source` writes nothing.** The blocks under a section with an empty `source` are still written, so such a section groups blocks.
- **`render` asks little of a block.** It needs `source` to be a string of valid UTF-8. A section with no `blocks` field holds nothing, as it does for a lookup, and a block with no `kind` is written like any other.
- **`*` matches a block with no `kind`.** A lookup used to skip a block whose `kind` is not a string. It now reads such a block as one of no kind, which only `*` matches, so the flat view holds every block `render` writes.
- **The unknown-kind message** now ends `kinds are frontmatter, section, prose, fence, list, rule, and * selects every block`.
- **Sizes.** The sample texts are the guide and 74 bodies, 75 in all, where the plan said 75 bodies. `render.rs` is 98 lines with its comments, where the plan said about 40. The field reader removed from `runvil-lua` was about 140 lines.
- **Verification.** Every check listed passed. One Workshop test, `gateway::tests::recovery::successful_publication_disarms_late_child_cleanup`, failed once in the first run of its set, then passed alone and in a second run of the set. It starts a child process, and this change touches nothing it uses.
- **Documents.** `runvil-design` commit `f1b91cf` and `runvil-docs` commit `8129065` hold the document changes.
