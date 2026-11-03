//! Source positions: 1-based line arithmetic, and the location of a stretch of
//! the shared prompt body. The fence splitter, the section walker, and the
//! parser all use these, so the splitter does not depend on the walker.

use std::num::NonZeroU32;
use std::sync::Arc;

use crate::{BodyText, Result, error};

/// Wraps a computed 1-based source line for the Lua compiler.
///
/// Line numbers computed during parsing are always at least 1, so a zero means
/// a broken source-position invariant.
///
/// # Errors
/// Returns an `Internal` failure when `line` is zero.
pub(crate) fn nz_source_line(line: u32) -> Result<NonZeroU32> {
    NonZeroU32::new(line)
        .ok_or_else(|| error::internal("parser: computed 1-based source line was zero"))
}

/// Adds two line components, at most one of them 1-based, with overflow
/// checking.
///
/// # Errors
/// Returns an `Internal` failure when the sum overflows `u32`, rather than
/// saturating and hiding a broken position.
pub(crate) fn line_add(a: u32, b: u32) -> Result<u32> {
    a.checked_add(b)
        .ok_or_else(|| error::internal("parser: source line arithmetic overflowed"))
}

/// Locates `part` inside `shared`, the text it was sliced from.
///
/// # Errors
/// Returns an `Internal` failure when `part` is not a slice of `shared`.
pub(crate) fn body_text(shared: &Arc<str>, part: &str) -> Result<BodyText> {
    BodyText::within(shared, part)
        .ok_or_else(|| error::internal("parser: section text is not a slice of the prompt body"))
}

/// Counts the number of `\n` characters in `text[..byte_offset]`.
///
/// # Errors
/// Returns an `Internal` failure when `byte_offset` is out of bounds or not
/// a UTF-8 boundary, or when the newline count exceeds `u32`.
pub(crate) fn newlines_before(text: &str, byte_offset: usize) -> Result<u32> {
    let prefix = text
        .get(..byte_offset)
        .ok_or_else(|| error::internal("parser: source byte offset invariant broken"))?;
    u32::try_from(prefix.matches('\n').count())
        .map_err(|_| error::internal("parser: newline count exceeded u32 range"))
}

#[cfg(test)]
mod tests;
