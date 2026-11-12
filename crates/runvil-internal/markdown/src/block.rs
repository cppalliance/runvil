//! The block model: what a parsed document is made of.

use serde_json::Value as Json;

/// One block of a parsed document.
///
/// A document is a list of blocks. A section holds the blocks under its
/// heading, subsections included, so the list is a tree. Every block
/// records the line it starts on: a 1-based line of the text that was
/// parsed.
#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    /// The YAML mapping that opens the text. It is only ever the first
    /// block of a document.
    Frontmatter {
        /// The line of the opening `---`.
        line: usize,
        /// The mapping, as a JSON object.
        data: Json,
    },
    /// A heading and everything under it.
    Section {
        /// The heading's line.
        line: usize,
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
        /// The source as written, without leading blank lines or trailing
        /// whitespace. It is never empty.
        text: String,
    },
    /// A fenced code block.
    Fence {
        /// The line of the opening fence.
        line: usize,
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
        /// Whether the list is numbered.
        ordered: bool,
        /// Each item's source with its marker removed.
        items: Vec<String>,
    },
    /// A thematic break.
    Rule {
        /// The break's line.
        line: usize,
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
            | Block::Rule { line } => *line,
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
    fn a_block_reports_its_kind_and_its_line() {
        let block = Block::Rule { line: 7 };
        assert_eq!((block.kind(), block.line()), (Kind::Rule, 7));
        let block = Block::Section {
            line: 3,
            level: 2,
            title: "A".to_owned(),
            blocks: Vec::new(),
        };
        assert_eq!((block.kind(), block.line()), (Kind::Section, 3));
    }
}
