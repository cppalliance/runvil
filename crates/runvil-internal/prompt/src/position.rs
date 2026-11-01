//! Source positions: 1-based line arithmetic, and the location of a stretch of
//! the shared prompt body. The fence splitter, the section walker, and the
//! parser all use these, so none of them has to name another for them.

use std::num::NonZeroU32;
use std::sync::Arc;

use crate::{BodyText, Error, Result};

/// Wraps a computed 1-based source line for the Lua compiler.
///
/// Line numbers computed during parsing are always at least 1. A zero here means
/// a broken source-position invariant; rather than silently coercing it to line
/// one, this reports a structured internal parse error.
///
/// # Errors
/// Returns [`Error::Internal`] when `line` is zero.
pub(crate) fn nz_source_line(line: u32) -> Result<NonZeroU32> {
    NonZeroU32::new(line).ok_or(Error::Internal(
        "parser: computed 1-based source line was zero",
    ))
}

/// Adds two 1-based line components with overflow checking.
///
/// # Errors
/// Returns [`Error::Internal`] when the sum overflows `u32`, rather than
/// saturating and hiding a broken position.
pub(crate) fn line_add(a: u32, b: u32) -> Result<u32> {
    a.checked_add(b)
        .ok_or(Error::Internal("parser: source line arithmetic overflowed"))
}

/// Locates `part` inside the shared `text` it was sliced from.
///
/// # Errors
/// Returns [`Error::Internal`] when `part` is not a slice of `text`.
pub(crate) fn body_text(text: &Arc<str>, part: &str) -> Result<BodyText> {
    BodyText::within(text, part).ok_or(Error::Internal(
        "parser: section text is not a slice of the prompt body",
    ))
}

/// Counts the number of `\n` characters in `text[..byte_offset]`.
///
/// # Errors
/// Returns [`Error::Internal`] when `byte_offset` is out of bounds or not a
/// UTF-8 boundary, or when the newline count exceeds `u32`.
pub(crate) fn newlines_before(text: &str, byte_offset: usize) -> Result<u32> {
    let prefix = text.get(..byte_offset).ok_or(Error::Internal(
        "parser: source byte offset invariant broken",
    ))?;
    u32::try_from(prefix.matches('\n').count())
        .map_err(|_| Error::Internal("parser: newline count exceeded u32 range"))
}
