//! Layer 2: a body's blocks in file order, before sections nest.
//!
//! [`flatten`] takes the marks of layer 1 and gives the intermediate form
//! the tree is folded from: each heading, and each leaf block as it will
//! appear in the result. The text between two marks is prose, which is
//! what keeps every line the Markdown library reports nothing for, such
//! as a link definition. Byte ranges end here: everything this layer
//! hands on is counted in lines.

use std::ops::Range;

use super::events::{Item, Mark, What};
use crate::Block;

/// One entry of a body's flat form.
pub(super) enum Flat {
    /// A heading, which opens a section.
    Heading {
        /// The heading's line.
        line: usize,
        /// The heading's level, 1 to 6.
        level: u8,
        /// The heading's text, trimmed.
        title: String,
    },
    /// A block that holds no other block: prose, a fence, a list, or a
    /// rule.
    Leaf(Block),
}

/// Returns the flat form of `body`, whose top-level constructs are
/// `marks`, in file order. `first_line` is the line number of the body's
/// first line.
pub(super) fn flatten(body: &str, marks: Vec<Mark>, first_line: usize) -> Vec<Flat> {
    let mut lines = Lines::new(body, first_line);
    let mut flat = Vec::with_capacity(marks.len() * 2 + 1);
    let mut cursor = 0;
    for mark in marks {
        flat.extend(prose(body, cursor..mark.range.start, &mut lines));
        cursor = cursor.max(mark.range.end);
        let line = lines.at(mark.range.start);
        flat.push(match mark.what {
            What::Heading { level, title } => Flat::Heading {
                line,
                level,
                title: title.trim().to_owned(),
            },
            What::Fence { info, text } => Flat::Leaf(fence(line, &info, text)),
            What::List { ordered, items } => Flat::Leaf(Block::List {
                line,
                ordered,
                items: items.iter().map(|item| item_text(body, item)).collect(),
            }),
            What::Rule => Flat::Leaf(Block::Rule { line }),
        });
    }
    flat.extend(prose(body, cursor..body.len(), &mut lines));
    flat
}

/// Counts lines for offsets that only move forward, so a whole body is
/// counted once.
struct Lines<'a> {
    /// The body.
    body: &'a str,
    /// The offset counted up to.
    counted: usize,
    /// The line number at `counted`.
    line: usize,
}

impl<'a> Lines<'a> {
    /// Starts at the front of `body`, on line `first_line`.
    fn new(body: &'a str, first_line: usize) -> Self {
        Self {
            body,
            counted: 0,
            line: first_line,
        }
    }

    /// Returns the line number of the byte at `offset`, which is at or
    /// past every offset asked for before.
    fn at(&mut self, offset: usize) -> usize {
        if let Some(passed) = self.body.get(self.counted..offset) {
            self.line += passed.matches('\n').count();
            self.counted = offset;
        }
        self.line
    }
}

/// The characters that leave a Markdown line blank.
const BLANK: [char; 3] = [' ', '\t', '\n'];

/// Returns the prose block for the text of `gap`, the bytes between two
/// marks, or `None` when the gap holds only blank lines.
///
/// The text is the source as written, without leading blank lines or
/// trailing whitespace, so its first line keeps its indentation.
fn prose(body: &str, gap: Range<usize>, lines: &mut Lines<'_>) -> Option<Flat> {
    let text = body.get(gap.clone())?;
    let blank: usize = text
        .split_inclusive('\n')
        .take_while(|line| line.trim_matches(BLANK).is_empty())
        .map(str::len)
        .sum();
    let text = text.get(blank..)?.trim_end_matches(BLANK);
    if text.is_empty() {
        return None;
    }
    Some(Flat::Leaf(Block::Prose {
        line: lines.at(gap.start + blank),
        text: text.to_owned(),
    }))
}

/// Returns the fence block on `line` for an info string and the lines
/// between the fence lines.
fn fence(line: usize, info: &str, mut text: String) -> Block {
    if text.ends_with('\n') {
        text.pop();
    }
    let info = info.trim();
    Block::Fence {
        line,
        lang: info
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_owned(),
        info: info.to_owned(),
        text,
    }
}

/// Returns an item's source with its marker removed.
///
/// The text starts where the item's content starts. Each later line loses
/// as much leading whitespace as the marker gave the first, so nested
/// content keeps its own shape and parses again as Markdown.
fn item_text(body: &str, item: &Item) -> String {
    let Some(start) = item.content else {
        return String::new();
    };
    let (Some(before), Some(source)) = (body.get(..start), body.get(start..item.range.end)) else {
        return String::new();
    };
    let width = start - before.rfind('\n').map_or(0, |at| at + 1);
    let mut lines = source.split('\n');
    let mut text = lines.next().unwrap_or_default().to_owned();
    for line in lines {
        let indent = line
            .bytes()
            .take(width)
            .take_while(|byte| matches!(byte, b' ' | b'\t'))
            .count();
        text.push('\n');
        text.push_str(line.get(indent..).unwrap_or_default());
    }
    text.truncate(text.trim_end_matches(BLANK).len());
    text
}

#[cfg(test)]
mod tests {
    use super::super::events::marks;
    use super::{Flat, flatten};
    use crate::Block;

    /// The flat form of `body`, each entry as a short word and its line.
    fn outline(body: &str, first_line: usize) -> Vec<(String, usize)> {
        flatten(body, marks(body), first_line)
            .into_iter()
            .map(|entry| match entry {
                Flat::Heading { line, level, title } => (format!("h{level} {title}"), line),
                Flat::Leaf(block) => (block.kind().name().to_owned(), block.line()),
            })
            .collect()
    }

    #[test]
    fn the_text_between_two_marks_is_prose_and_blank_gaps_are_nothing() {
        let body = "\n\nIntro.\n\n# Title\n\n\n- a\n[ref]: http://x\n\n---\ntail";
        assert_eq!(
            outline(body, 1),
            [
                ("prose".to_owned(), 3),
                ("h1 Title".to_owned(), 5),
                ("list".to_owned(), 8),
                ("rule".to_owned(), 11),
                ("prose".to_owned(), 12),
            ]
        );
    }

    #[test]
    fn lines_count_from_the_first_line_given() {
        assert_eq!(
            outline("text\n\n```\nx\n```\n", 40),
            [("prose".to_owned(), 40), ("fence".to_owned(), 42)]
        );
    }

    #[test]
    fn prose_keeps_its_source_without_leading_blank_lines_or_trailing_whitespace() {
        let body = "  \n\t\n    indented code\n\nA paragraph.  \n\n \n# H\n";
        let flat = flatten(body, marks(body), 1);
        let Some(Flat::Leaf(Block::Prose { line, text })) = flat.first() else {
            panic!("the body opens with prose");
        };
        assert_eq!(
            (*line, text.as_str()),
            (3, "    indented code\n\nA paragraph.")
        );
    }

    #[test]
    fn a_line_of_other_whitespace_is_not_blank() {
        let body = "\u{a0}\n\n# H\n";
        let flat = flatten(body, marks(body), 1);
        assert!(matches!(
            flat.first(),
            Some(Flat::Leaf(Block::Prose { line: 1, text })) if text == "\u{a0}"
        ));
    }
}
