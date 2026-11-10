//! Operations on the parser's facade-visible types that only
//! `runvil-runtime` performs.
//!
//! The `runvil-engine` facade never re-exports this module, so only Engine
//! crates reach it.

use std::num::NonZeroU32;

use runvil_types::error::Error;

use crate::{Block, Prompt, Section};

/// Gives a parse failure the Engine raises about a parsed prompt the
/// prompt's frontmatter `name` as its `Name` fact, its place in the
/// prompt's `file`, and the prose a failure the parse returns has.
#[must_use]
pub fn with_prompt_context(error: Error, name: &str, file: &str) -> Error {
    crate::error::with_prompt_context(error, Some(name), file)
}

/// Returns the ordered live Lua and prose blocks from the H1.
#[must_use]
pub fn h1_blocks(prompt: &Prompt) -> &[Block] {
    &prompt.h1_blocks
}

/// Returns the top-level H2 sections in file order.
#[must_use]
pub fn sections(prompt: &Prompt) -> &[Section] {
    &prompt.sections
}

/// The entry-point section: the first top-level section in file order.
#[must_use]
pub fn entry(prompt: &Prompt) -> Option<&Section> {
    prompt.entry()
}

/// Returns the file line of the frontmatter's top-level `key`, such as
/// `uses`, when exactly one line declares it at column 0.
///
/// The line comes from a scan of the text, not the YAML parse, so it is
/// meaningful only for a key that decoded to a value.
#[must_use]
pub fn key_line(prompt: &Prompt, key: &str) -> Option<NonZeroU32> {
    prompt.key_lines.get(key).copied()
}
