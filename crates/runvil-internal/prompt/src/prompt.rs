//! Holds the parsed prompt tree: [`Prompt`], its [`Section`]s, and their [`Block`]s.
//!
//! Code outside the Engine reads a prompt's title and frontmatter; the tree
//! below them is the Engine's, reached through [`crate::detail`].
//!
//! Construction happens in the parsing modules; this module holds the value
//! types and their accessors.

use std::num::NonZeroU32;

use crate::{BodyText, Frontmatter};
use runvil_lua::LuaProgram;

/// One executable block inside a section: a compiled Lua fence or prose.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Block {
    /// An exact `lua` fence compiled at parse time.
    Lua(LuaProgram),
    /// Author prose: the pending Markdown accumulated since the nearest
    /// preceding heading, `lua` fence, or thematic break.
    ///
    /// The executor installs it as the following Lua block's lazy `prose`
    /// template.
    #[non_exhaustive]
    Prose {
        /// Captured Markdown, trimmed of leading and trailing whitespace.
        text: BodyText,
        /// The 1-based line of the prompt file that `text` starts on.
        line: NonZeroU32,
    },
}

/// One section of a prompt: a heading, ordered blocks, and children.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// The heading text (the section's address).
    pub(crate) name: String,
    /// The heading level, 2 through 6.
    pub(crate) level: u8,
    /// Ordered lua/prose blocks for this section.
    pub(crate) blocks: Vec<Block>,
    /// Child sections nested under this one (deeper heading levels).
    pub(crate) children: Vec<Section>,
    /// The list items of a list section, which has at least one; empty for
    /// every other section.
    pub(crate) items: Vec<BodyText>,
    /// The section's own raw Markdown, without its heading line or children.
    pub(crate) markdown: BodyText,
}

impl Section {
    /// Returns the heading text (the section's address).
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the heading level (2 through 6).
    #[must_use]
    pub fn level(&self) -> u8 {
        self.level
    }

    /// Returns the ordered Lua and prose blocks of this section.
    #[must_use]
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// Returns the child sections nested under this one.
    #[must_use]
    pub fn children(&self) -> &[Section] {
        &self.children
    }

    /// Returns the list items of a list section; empty for every other section.
    #[must_use]
    pub fn items(&self) -> &[BodyText] {
        &self.items
    }

    /// The section's own raw Markdown: from just after its heading line to the next
    /// heading of any level, leading blank lines and trailing whitespace trimmed.
    #[must_use]
    pub fn markdown(&self) -> &str {
        self.markdown.as_str()
    }

    /// The prologue: the first block, when it is Lua.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn prologue(&self) -> Option<&LuaProgram> {
        match self.blocks.first() {
            Some(Block::Lua(program)) => Some(program),
            _ => None,
        }
    }

    /// Text of the last prose block, or `""` when the section has none.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn prose(&self) -> &str {
        self.blocks
            .iter()
            .rev()
            .find_map(|block| match block {
                Block::Prose { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .unwrap_or("")
    }

    /// The epilog: the last block, when it is Lua and not the only block.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn epilog(&self) -> Option<&LuaProgram> {
        match self.blocks.as_slice() {
            [Block::Lua(_)] => None,
            [.., Block::Lua(program)] => Some(program),
            _ => None,
        }
    }

    /// Returns whether this is a list section, which has at least one item.
    #[must_use]
    pub fn is_list(&self) -> bool {
        !self.items.is_empty()
    }
}

/// A fully parsed prompt file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    /// The file the source came from, as the caller of
    /// [`Prompt::parse`](crate::Prompt::parse) named it.
    pub(crate) file: String,
    /// The parsed YAML frontmatter.
    pub(crate) frontmatter: Frontmatter,
    /// The required H1 title.
    pub(crate) title: String,
    /// Ordered live Lua and prose blocks from the H1.
    pub(crate) h1_blocks: Vec<Block>,
    /// Top-level sections (H2s) in file order.
    pub(crate) sections: Vec<Section>,
}

impl Prompt {
    /// Returns the file the source came from, such as
    /// `agents/research.md`, or `prompt` when it came from no file.
    #[must_use]
    pub fn file(&self) -> &str {
        &self.file
    }

    /// Returns the parsed frontmatter.
    #[must_use]
    pub fn frontmatter(&self) -> &Frontmatter {
        &self.frontmatter
    }

    /// Returns the prompt's title, the text of its required H1 heading.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The entry-point section: the first top-level section in file order;
    /// the Engine reaches it through [`crate::detail::entry`].
    #[must_use]
    pub(crate) fn entry(&self) -> Option<&Section> {
        self.sections.first()
    }
}
