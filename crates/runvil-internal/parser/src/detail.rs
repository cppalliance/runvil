//! Operations on the parser's facade-visible types that only
//! `runvil-engine` performs.
//!
//! The `runvil` facade never re-exports this module, so only Engine
//! crates reach it.

use crate::{
    Block, Error, Frontmatter, MaxToolIterations, ModelKeyword, ParseError, ParseErrorKind, Prompt,
    Section,
};

/// Unwraps a parse failure into the internal error it classifies, so the
/// Engine can map it onto its own internal error type variant for variant.
#[must_use]
pub fn parse_error_into_inner(error: ParseError) -> Error {
    error.into_inner()
}

/// Classifies an internal error back into the parse failure that wraps
/// it, the inverse of [`parse_error_into_inner`].
#[must_use]
pub fn parse_error_from_inner(inner: Error) -> ParseError {
    ParseError::from_inner(inner)
}

/// Builds a parse failure with a stable classification and no source span,
/// for a failure the Engine raises about a parsed prompt.
#[must_use]
pub fn parse_error(kind: ParseErrorKind, message: impl Into<String>) -> ParseError {
    ParseError::from_inner(Error::parse(kind, message))
}

/// Stamps the prompt's frontmatter name onto a spanless structured parse
/// failure. A failure with a span passes through unchanged, because
/// locating a span needs the prompt body.
#[must_use]
pub fn with_prompt_name(error: ParseError, name: &str) -> ParseError {
    if error.span().is_some() {
        return error;
    }
    ParseError::from_inner(error.into_inner().with_prompt_context(name, "", 0))
}

/// Returns the ordered live Lua and prose blocks from the H1.
#[must_use]
pub fn h1_blocks(prompt: &Prompt) -> &[Block] {
    prompt.h1_blocks()
}

/// Returns the top-level H2 sections in file order.
#[must_use]
pub fn sections(prompt: &Prompt) -> &[Section] {
    prompt.sections()
}

/// The entry-point section: the first top-level section in file order.
#[must_use]
pub fn entry(prompt: &Prompt) -> Option<&Section> {
    prompt.entry()
}

/// Returns the frontmatter's tool-loop cap in the form the Engine resolves
/// against its own default.
#[must_use]
pub fn max_tool_iterations(frontmatter: &Frontmatter) -> MaxToolIterations {
    frontmatter.max_tool_iterations
}

/// Returns the keyword's kebab-case spelling, the one the frontmatter
/// writes.
#[must_use]
pub fn keyword_name(keyword: ModelKeyword) -> &'static str {
    match keyword {
        ModelKeyword::Thinking => "thinking",
        ModelKeyword::NoThinking => "no-thinking",
        ModelKeyword::Frontier => "frontier",
        ModelKeyword::Fast => "fast",
        ModelKeyword::Small => "small",
        ModelKeyword::Creative => "creative",
        ModelKeyword::Chat => "chat",
    }
}
