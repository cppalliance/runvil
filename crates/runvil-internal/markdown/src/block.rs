//! The block model: what a parsed document is made of.

use serde_json::Value as Json;

/// One block of a parsed document.
///
/// A document is a list of blocks. A section holds the blocks under its
/// heading, subsections included, so the list is a tree. Every block
/// records the line it starts on: a 1-based line of the text that was
/// parsed.
///
/// Every block also holds its `source`: the lines it was read from,
/// exactly as written and each with its line ending, and the blank lines
/// after them. Blank lines at the very top of the text are in the first
/// block's source. A section's source is its heading's, since the blocks
/// under the heading hold their own. So no character is in two sources,
/// and the sources of a document, in file order, are the whole text.
#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    /// The YAML mapping that opens the text. It is only ever the first
    /// block of a document.
    Frontmatter {
        /// The line of the opening `---`.
        line: usize,
        /// Both delimiter lines, the lines between them, and the blank
        /// lines after.
        source: String,
        /// The mapping, as a JSON object.
        data: Json,
    },
    /// A heading and everything under it.
    Section {
        /// The heading's line.
        line: usize,
        /// The heading's lines and the blank lines after them.
        source: String,
        /// The heading's level, 1 to 6.
        level: u8,
        /// The heading's text with inline markup removed, trimmed.
        title: String,
        /// The blocks under the heading: the section's own content, then
        /// its subsections.
        blocks: Vec<Block>,
    },
    /// A run of Markdown that is no other kind: paragraphs, block quotes,
    /// indented code, HTML, and link definitions.
    Prose {
        /// The first non-blank line of the run.
        line: usize,
        /// The run's lines and the blank lines after them.
        source: String,
        /// The source without leading blank lines or trailing
        /// whitespace. It is never empty.
        text: String,
    },
    /// A fenced code block.
    Fence {
        /// The line of the opening fence.
        line: usize,
        /// Both fence lines, the lines between them, and the blank lines
        /// after.
        source: String,
        /// The first word of the info string, or empty.
        lang: String,
        /// The whole info string, trimmed.
        info: String,
        /// The lines between the two fence lines, without the final line
        /// ending.
        text: String,
    },
    /// A bullet or numbered list.
    List {
        /// The line of the first marker.
        line: usize,
        /// The list's lines, markers included, and the blank lines after
        /// them.
        source: String,
        /// Whether the list is numbered.
        ordered: bool,
        /// Each item's source with its marker removed.
        items: Vec<String>,
    },
    /// A thematic break.
    Rule {
        /// The break's line.
        line: usize,
        /// The break's line and the blank lines after it.
        source: String,
    },
}

impl Block {
    /// The block's kind.
    #[must_use]
    pub fn kind(&self) -> Kind {
        match self {
            Block::Frontmatter { .. } => Kind::Frontmatter,
            Block::Section { .. } => Kind::Section,
            Block::Prose { .. } => Kind::Prose,
            Block::Fence { .. } => Kind::Fence,
            Block::List { .. } => Kind::List,
            Block::Rule { .. } => Kind::Rule,
        }
    }

    /// The line the block starts on.
    #[must_use]
    pub fn line(&self) -> usize {
        match self {
            Block::Frontmatter { line, .. }
            | Block::Section { line, .. }
            | Block::Prose { line, .. }
            | Block::Fence { line, .. }
            | Block::List { line, .. }
            | Block::Rule { line, .. } => *line,
        }
    }

    /// The lines the block was read from, as written. A section's are
    /// its heading's.
    #[must_use]
    pub fn source(&self) -> &str {
        match self {
            Block::Frontmatter { source, .. }
            | Block::Section { source, .. }
            | Block::Prose { source, .. }
            | Block::Fence { source, .. }
            | Block::List { source, .. }
            | Block::Rule { source, .. } => source,
        }
    }
}

/// The kind of a block, which is also the word a selector names it by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The YAML mapping that opens the text.
    Frontmatter,
    /// A heading and everything under it.
    Section,
    /// A run of Markdown that is no other kind.
    Prose,
    /// A fenced code block.
    Fence,
    /// A bullet or numbered list.
    List,
    /// A thematic break.
    Rule,
}

impl Kind {
    /// Every kind, in the order messages list them.
    pub const ALL: [Kind; 6] = [
        Kind::Frontmatter,
        Kind::Section,
        Kind::Prose,
        Kind::Fence,
        Kind::List,
        Kind::Rule,
    ];

    /// The kind's name: the value of a block's `kind` field.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Kind::Frontmatter => "frontmatter",
            Kind::Section => "section",
            Kind::Prose => "prose",
            Kind::Fence => "fence",
            Kind::List => "list",
            Kind::Rule => "rule",
        }
    }

    /// The kind `name` names, or `None` for any other text.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|kind| kind.name() == name)
    }
}

#[cfg(test)]
mod tests {
    use super::{Block, Kind};

    #[test]
    fn every_kind_is_found_by_its_own_name_and_no_other_text_names_one() {
        for kind in Kind::ALL {
            assert_eq!(Kind::from_name(kind.name()), Some(kind));
        }
        for other in ["", "Fence", "fences", "break", "code"] {
            assert_eq!(Kind::from_name(other), None, "{other:?}");
        }
    }

    #[test]
    fn a_block_reports_its_kind_its_line_and_its_source() {
        let block = Block::Rule {
            line: 7,
            source: "---\n\n".to_owned(),
        };
        assert_eq!(
            (block.kind(), block.line(), block.source()),
            (Kind::Rule, 7, "---\n\n")
        );
        // A section's source is its heading's, not its members'.
        let block = Block::Section {
            line: 3,
            source: "## A\n".to_owned(),
            level: 2,
            title: "A".to_owned(),
            blocks: vec![block],
        };
        assert_eq!(
            (block.kind(), block.line(), block.source()),
            (Kind::Section, 3, "## A\n")
        );
    }
}
