//! Text to blocks.
//!
//! The work is done in layers. Each is a child module whose items only
//! this module and the layer above it use:
//!
//! 1. `events` runs `pulldown-cmark` and reports each top-level heading,
//!    fence, list, and rule by its byte range. It is the only module that
//!    names the library, and it knows nothing of lines or blocks.
//! 2. `flat` turns those marks into the body's blocks in file order: the
//!    text between two marks is prose, and every line number is counted
//!    here. Nothing above it sees a byte range.
//! 3. `tree` folds that sequence into sections.
//!
//! `frontmatter` splits the YAML mapping off the front before the layers
//! see the text.

mod events;
mod flat;
mod frontmatter;
mod tree;

use std::borrow::Cow;

use crate::Block;

/// Parses Markdown text into blocks.
///
/// The text is read as CommonMark with no extensions. A byte order mark
/// at the very start is dropped, and `\r\n` and a lone `\r` are read as
/// `\n`. Only a top-level heading, fence, list, or rule becomes a block of
/// its kind; everything else is prose, so every non-blank line belongs to
/// exactly one block. Any text is a document: nothing here fails.
#[must_use]
pub fn parse(text: &str) -> Vec<Block> {
    let text = normalize(text);
    match frontmatter::split(&text) {
        Some(opening) => {
            let root = vec![Block::Frontmatter {
                line: 1,
                data: opening.data,
            }];
            let rest = text.get(opening.end..).unwrap_or_default();
            body(rest, opening.lines + 1, root)
        }
        None => body(&text, 1, Vec::new()),
    }
}

/// Parses text that has no frontmatter, such as one section's own
/// content, into blocks.
///
/// `first_line` is the line number of the text's first line, so each
/// block's line counts in the file the text was cut from.
#[must_use]
pub fn parse_body(text: &str, first_line: usize) -> Vec<Block> {
    body(&normalize(text), first_line, Vec::new())
}

/// Runs the three layers over `text`, adding its blocks after `root`.
fn body(text: &str, first_line: usize, root: Vec<Block>) -> Vec<Block> {
    tree::fold(flat::flatten(text, events::marks(text), first_line), root)
}

/// Drops a leading byte order mark and reads `\r\n` and `\r` as `\n`.
fn normalize(text: &str) -> Cow<'_, str> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if text.contains('\r') {
        Cow::Owned(text.replace("\r\n", "\n").replace('\r', "\n"))
    } else {
        Cow::Borrowed(text)
    }
}

#[cfg(test)]
#[path = "parse-tests.rs"]
mod tests;
