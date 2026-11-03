//! The one global naming grammar.
//!
//! A name is one or more segments separated by `/`. The id types built on
//! it add their own segment count: a [`PackId`](crate::packs::PackId)
//! is one segment, the second segment of a Pack's fully qualified name
//! (`web` for `runvil/web`), and a [`ToolId`](crate::tools::ToolId) is
//! two or more, its Pack's id first (`web/fetch`). Segments are lowercase ASCII
//! alphanumeric plus `-`, `_`, `.`, and comparison is case-sensitive. A
//! name carries no version, so a `@` is a parse error.
//!
//! [`is_alias`] holds a second grammar, for prompt-local aliases, which are
//! never global names.
//!
//! Every id type and every catalog of ids rejects an id with an
//! [`InvalidValue`](ErrorCode::InvalidValue) error, whether the id is built
//! on a name or not.

use std::fmt;
use std::str::FromStr;

use serde_json::Value;

use crate::error::{Error, ErrorCode, FactKey};

#[cfg(test)]
#[path = "names-tests.rs"]
mod tests;

/// A validated slash-separated name, such as `web` or `web/fetch`.
///
/// A name has one or more segments separated by `/`. [`PackId`] and
/// [`ToolId`] are built on it and each adds its own segment count.
///
/// Every value comes from [`GlobalName::parse`], so each segment always
/// consists of one or more lowercase ASCII letters, digits, `-`, `_`, and `.`.
///
/// [`PackId`]: crate::packs::PackId
/// [`ToolId`]: crate::tools::ToolId
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GlobalName {
    /// The `/`-separated segments: one or more.
    segments: Vec<String>,
}

impl GlobalName {
    /// Parses a string into a global name.
    ///
    /// The string must have one or more segments separated by `/`. Each
    /// segment must consist of one or more lowercase ASCII letters, digits,
    /// `-`, `_`, and `.`.
    ///
    /// # Errors
    ///
    /// Returns an [`InvalidValue`](ErrorCode::InvalidValue) error when:
    ///
    /// - a segment is empty, including the whole string being empty;
    /// - a segment contains a control byte;
    /// - a segment contains any other character outside the set.
    pub fn parse(s: &str) -> Result<GlobalName, Error> {
        GlobalName::validate(s)
            .map_err(|reason| invalid_id(format!("invalid global name: {reason}"), s))
    }

    /// Parses `s` as [`GlobalName::parse`] does, but returns only the
    /// reason it is invalid, so each id type built on a name can name
    /// itself in its own error.
    pub(crate) fn validate(s: &str) -> Result<GlobalName, &'static str> {
        let segments: Vec<&str> = s.split('/').collect();
        for segment in &segments {
            validate_segment(segment)?;
        }
        Ok(GlobalName {
            segments: segments.iter().map(|s| (*s).to_owned()).collect(),
        })
    }

    /// Builds a one-segment name from a segment already validated as part
    /// of another name.
    ///
    /// Crate-internal: backs [`crate::tools::ToolId::pack`].
    pub(crate) fn from_validated_segment(segment: &str) -> GlobalName {
        GlobalName {
            segments: vec![segment.to_owned()],
        }
    }

    /// Returns the segments (one or more by construction).
    ///
    /// Crate-internal: the id newtypes count and index segments.
    pub(crate) fn segments(&self) -> &[String] {
        &self.segments
    }

    /// Returns the first segment.
    ///
    /// Crate-internal: backs [`crate::tools::ToolId::pack`] and
    /// [`crate::packs::PackId::contains`].
    pub(crate) fn first(&self) -> &str {
        &self.segments[0]
    }
}

impl FromStr for GlobalName {
    type Err = Error;

    fn from_str(s: &str) -> Result<GlobalName, Error> {
        GlobalName::parse(s)
    }
}

impl fmt::Display for GlobalName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (position, segment) in self.segments.iter().enumerate() {
            if position > 0 {
                f.write_str("/")?;
            }
            f.write_str(segment)?;
        }
        Ok(())
    }
}

/// Validates one segment against the charset rule.
///
/// A segment must be non-empty and contain only lowercase ASCII
/// alphanumeric characters plus `-`, `_`, `.`. A control byte has its own
/// reason; anything else outside the set - including uppercase
/// (comparison is case-sensitive), `@`, and non-ASCII - shares the
/// charset reason.
fn validate_segment(segment: &str) -> Result<(), &'static str> {
    if segment.is_empty() {
        return Err("segments must not be empty");
    }
    for byte in segment.bytes() {
        if byte < 0x20 || byte == 0x7f {
            return Err("segments must not contain a control character");
        }
        if !matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.') {
            return Err("segments may contain only lowercase ASCII letters, digits, '-', '_', '.'");
        }
    }
    Ok(())
}

/// Returns whether `name` follows the prompt-local alias grammar
/// `[A-Za-z][A-Za-z0-9_-]{0,63}`: 1 to 64 bytes, an ASCII letter first, then
/// ASCII letters, digits, `_`, or `-`. An alias, such as an arg name or an
/// adhoc tool name, is never a global name.
#[must_use]
pub fn is_alias(name: &str) -> bool {
    let bytes = name.as_bytes();
    (1..=64).contains(&bytes.len())
        && bytes[0].is_ascii_alphabetic()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

/// The error for an invalid id, or for a catalog that repeats one: an
/// [`InvalidValue`](ErrorCode::InvalidValue) whose message says what was
/// rejected and why, such as `invalid Pack id: segments must not be empty`,
/// with the rejected text as its `Name` fact.
pub(crate) fn invalid_id(message: String, rejected: &str) -> Error {
    let mut error = Error::new(
        ErrorCode::InvalidValue,
        message,
        ErrorCode::InvalidValue.prose(),
    );
    error.facts.insert(FactKey::Name, Value::from(rejected));
    error
}
