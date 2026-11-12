//! Blocks to text.

use crate::parse::BLANK;
use crate::{Block, Kind};

/// Writes a list of blocks as Markdown: the source of each block in file
/// order, a section's before those of the blocks under it.
///
/// Nothing is rebuilt from a block's other fields, so the blocks that
/// [`parse`](fn@crate::parse) gave are written as the text they were read
/// from. Blocks put together any other way are written through a
/// [`Writer`], which keeps two of them from running into each other.
#[must_use]
pub fn render(blocks: &[Block]) -> String {
    let mut writer = Writer::new();
    write_blocks(&mut writer, blocks);
    writer.finish()
}

/// Writes `blocks` and the blocks under them to `writer`.
fn write_blocks(writer: &mut Writer, blocks: &[Block]) {
    for block in blocks {
        writer.write(block.kind().name(), block.source());
        if let Block::Section { blocks, .. } = block {
            write_blocks(writer, blocks);
        }
    }
}

/// Writes the sources of blocks one after another as one text.
///
/// A source is written as it is. The writer adds a line ending of its own
/// in two cases, neither of which occurs between two blocks of one parse,
/// so a parsed document is written exactly:
///
/// 1. Every block starts on a new line. When the text so far does not end
///    with a line ending, one is written first. Of parsed blocks, only
///    the last of a text can lack one.
/// 2. A blank line sits between two prose blocks, which would otherwise
///    read as one paragraph. When neither the text so far ends with a
///    blank line nor the block starts with one, one is written first.
///    Parsed prose is a maximal run, so no two prose blocks of one parse
///    are neighbors.
///
/// Whether any other two blocks need a blank line depends on their text,
/// and the writer does not decide that.
#[derive(Debug, Default)]
pub struct Writer {
    /// The text written so far.
    out: String,
    /// Whether the last block written was prose.
    after_prose: bool,
}

impl Writer {
    /// A writer that has written nothing.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Writes the source of one block whose `kind` field is `kind`. The
    /// blocks under a section are written after it, each by its own call.
    /// An empty source writes nothing.
    pub fn write(&mut self, kind: &str, source: &str) {
        if source.is_empty() {
            return;
        }
        let prose = kind == Kind::Prose.name();
        if !self.out.is_empty() {
            if !self.out.ends_with('\n') {
                self.out.push('\n');
            }
            let parted = is_blank(self.out.split_inclusive('\n').next_back())
                || is_blank(source.split_inclusive('\n').next());
            if prose && self.after_prose && !parted {
                self.out.push('\n');
            }
        }
        self.out.push_str(source);
        self.after_prose = prose;
    }

    /// The text written.
    #[must_use]
    pub fn finish(self) -> String {
        self.out
    }
}

/// Whether `line` is a blank line. No line is not one.
fn is_blank(line: Option<&str>) -> bool {
    line.is_some_and(|line| line.trim_matches(BLANK).is_empty())
}

#[cfg(test)]
#[path = "render-tests.rs"]
mod tests;
