//! Blocks to text.

use serde_json::Value as Json;

use crate::Block;

/// Writes a list of blocks as Markdown, each block in one fixed style.
///
/// Blocks are joined by one blank line, and the result has no trailing
/// newline. Nothing is escaped: a block's strings are written as they
/// are, so blocks that [`parse`](fn@crate::parse) produced read back as the
/// same blocks apart from their lines. The one exception is a title whose
/// text Markdown reads as markup.
///
/// Every list of blocks is written. Whether one can be read back is the
/// caller's to check: a level from 1 to 6, no line break in a title or an
/// info string, at least one item in a list, and frontmatter only as the
/// first block, with a mapping as its data.
#[must_use]
pub fn render(blocks: &[Block]) -> String {
    let mut out = String::new();
    write_blocks(&mut out, blocks, true);
    out
}

/// Writes `blocks` with one blank line between each two. `top` is whether
/// they are the list given to [`render`], whose first line opens the
/// text.
fn write_blocks(out: &mut String, blocks: &[Block], top: bool) {
    // The list before this block, when there is one right before it: its
    // type, and whether it took the second marker.
    let mut before: Option<(bool, bool)> = None;
    for (at, block) in blocks.iter().enumerate() {
        if at > 0 {
            out.push_str("\n\n");
        }
        let mut list = None;
        match block {
            Block::Frontmatter { data, .. } => write_frontmatter(out, data),
            Block::Section {
                level,
                title,
                blocks,
                ..
            } => write_section(out, *level, title, blocks),
            Block::Prose { text, .. } => out.push_str(text),
            Block::Fence {
                lang, info, text, ..
            } => write_fence(out, if info.is_empty() { lang } else { info }, text),
            Block::List { ordered, items, .. } => {
                // A list right after a list of the same type takes the
                // second marker, so the two stay separate lists.
                let second = before == Some((*ordered, false));
                write_list(out, *ordered, second, items);
                list = Some((*ordered, second));
            }
            // `***` at the very start, so the text never opens with a
            // line that could start frontmatter.
            Block::Rule { .. } => out.push_str(if top && at == 0 { "***" } else { "---" }),
        }
        before = list;
    }
}

/// Writes frontmatter: the data as YAML between two `---` lines. The
/// keys of a mapping are written in sorted order, the order a JSON object
/// keeps them in.
fn write_frontmatter(out: &mut String, data: &Json) {
    out.push_str("---\n");
    out.push_str(&serde_yaml_ng::to_string(data).unwrap_or_default());
    out.push_str("---");
}

/// Writes a section: its heading, then its blocks.
fn write_section(out: &mut String, level: u8, title: &str, blocks: &[Block]) {
    out.push_str(&"#".repeat(usize::from(level)));
    if !title.is_empty() {
        out.push(' ');
        out.push_str(title);
    }
    if !blocks.is_empty() {
        out.push_str("\n\n");
        write_blocks(out, blocks, false);
    }
}

/// Writes a fence around `text`, with `label` after the opening fence.
///
/// The fence is of backticks, one longer than the longest run of them in
/// `text` and at least three. A label that holds a backtick cannot follow
/// a backtick fence, so the fence is then of tildes.
fn write_fence(out: &mut String, label: &str, text: &str) {
    let mark = if label.contains('`') { '~' } else { '`' };
    let fence = mark.to_string().repeat(3.max(longest_run(text, mark) + 1));
    out.push_str(&fence);
    out.push_str(label);
    out.push('\n');
    if !text.is_empty() {
        out.push_str(text);
        out.push('\n');
    }
    out.push_str(&fence);
}

/// Returns the length of the longest run of `mark` in `text`.
fn longest_run(text: &str, mark: char) -> usize {
    let mut longest = 0;
    let mut run = 0;
    for c in text.chars() {
        run = if c == mark { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    longest
}

/// Writes a list, one item per marker: `-` or `1.`, or `*` or `1)` when
/// the list takes the `second` marker.
fn write_list(out: &mut String, ordered: bool, second: bool, items: &[String]) {
    for (at, item) in items.iter().enumerate() {
        if at > 0 {
            out.push('\n');
        }
        let marker = match (ordered, second) {
            (false, false) => "-".to_owned(),
            (false, true) => "*".to_owned(),
            (true, false) => format!("{}.", at + 1),
            (true, true) => format!("{})", at + 1),
        };
        write_item(out, &marker, item);
    }
}

/// Writes one item after its marker. Each later line is indented to sit
/// under the first, and an empty item is its marker alone.
fn write_item(out: &mut String, marker: &str, item: &str) {
    let indent = " ".repeat(marker.len() + 1);
    let mut lines = item.split('\n');
    out.push_str(marker);
    if let Some(first) = lines.next().filter(|first| !first.is_empty()) {
        out.push(' ');
        out.push_str(first);
    }
    for line in lines {
        out.push('\n');
        if !line.is_empty() {
            out.push_str(&indent);
            out.push_str(line);
        }
    }
}

#[cfg(test)]
#[path = "render-tests.rs"]
mod tests;
