//! Builds the parser's failures as the universal [`Error`].
//!
//! Every parsing module returns its failure through [`Result`], built by
//! the constructors here: the code names what failed and the message is
//! the full diagnostic. [`with_prompt_context`] runs once on every failure
//! [`Prompt::parse`](crate::Prompt::parse) returns: it names the file the
//! failure is in, the prompt a failure in the body belongs to, and writes
//! the prose.

use std::num::NonZeroU32;
use std::ops::Range;

use runvil_types::error::{Error, ErrorCode, FactKey, Location, coordinates};
use runvil_types::packs::PackId;

/// The parser's result type over the universal [`Error`].
pub(crate) type Result<T> = std::result::Result<T, Error>;

/// Builds a failure with no location, whose prose is the code's default
/// until [`with_prompt_context`] writes it.
pub(crate) fn failure(code: ErrorCode, message: impl Into<String>) -> Error {
    Error::new(code, message, code.prose())
}

/// Builds a failure at `span`, the byte range of the offending region
/// within `body` (after the frontmatter and a leading BOM, CRLF normalized
/// to LF), which starts in the file after `frontmatter_lines` lines.
///
/// The location runs from the span's start to its end, each a 1-based file
/// line and byte column. It is `None` when either end is out of bounds or
/// its arithmetic overflows - an invariant break that must not replace the
/// original failure.
pub(crate) fn failure_at(
    code: ErrorCode,
    message: impl Into<String>,
    body: &str,
    span: Range<usize>,
    frontmatter_lines: u32,
) -> Error {
    let mut error = failure(code, message);
    error.at = span_location(body, span, frontmatter_lines);
    error
}

/// Builds the failure for a broken parser invariant: a state the
/// surrounding code has already guaranteed cannot occur.
pub(crate) fn internal(message: &str) -> Error {
    failure(
        ErrorCode::Internal,
        format!("internal invariant violated: {message}"),
    )
}

/// Builds the frontmatter failure for a refused `uses:` list: `reason` is
/// its `Reason` fact and the refused Pack id its `Name` fact.
pub(crate) fn pack_refusal(reason: &str, pack: &PackId, message: String) -> Error {
    let mut error = failure(ErrorCode::FrontmatterInvalid, message);
    error.facts.insert(FactKey::Reason, reason.into());
    error.facts.insert(FactKey::Name, pack.to_string().into());
    error
}

/// Builds the failure for frontmatter that did not decode: it was not
/// valid YAML, or a key or value broke the frontmatter schema.
///
/// The location is the file position the YAML parser reported: its line
/// and column when it reported both, or the whole line when the column is
/// unknown.
pub(crate) fn frontmatter_decode(cause: &serde_yaml_ng::Error) -> Error {
    let mut error = failure(
        ErrorCode::FrontmatterInvalid,
        format!("invalid frontmatter: {cause}"),
    );
    error.at = cause.location().and_then(|location| {
        // The location is relative to the frontmatter block, which starts
        // on file line 2 (line 1 is the opening `---` delimiter).
        let line = u32::try_from(location.line())
            .ok()
            .and_then(|line| line.checked_add(1))
            .and_then(NonZeroU32::new)?;
        match u32::try_from(location.column())
            .ok()
            .and_then(NonZeroU32::new)
        {
            Some(column) => Location::new(String::new(), line, column, line, column),
            None => Some(Location::line(String::new(), line)),
        }
    });
    error
}

/// Builds the failure for a Lua block that did not compile, or for a
/// compiler that could not start, which is internal: the compiler's code
/// and message.
///
/// A compile failure is located on the file line its diagnostic names, as
/// the compiler located it.
pub(crate) fn lua(cause: &Error) -> Error {
    let mut error = failure(cause.code, cause.message.clone());
    error.at.clone_from(&cause.at);
    error
}

/// Gives a failure the parse returns its file, its prompt name, and its
/// prose; the Engine reaches it through [`crate::detail::with_prompt_context`]
/// for a failure it raises about a parsed prompt.
///
/// `file` becomes the file of the failure's location. `name`, the prompt's
/// frontmatter name, is `None` for a failure found before the name is
/// known; otherwise it becomes the `Name` fact of every failure but an
/// internal one. The prose of an internal failure is the code's default.
/// Every other prose names the file, the start line when the failure has
/// a location, the message, and the default:
/// `{file}:{line}: {message}. {default}`, or without a location
/// `{file}: {message}. {default}`.
pub(crate) fn with_prompt_context(mut error: Error, name: Option<&str>, file: &str) -> Error {
    let code = error.code;
    if let Some(name) = name
        && code != ErrorCode::Internal
    {
        error.facts.insert(FactKey::Name, name.into());
    }
    if let Some(at) = &mut error.at {
        file.clone_into(&mut at.file);
    }
    error.prose = match (code, &error.at) {
        (ErrorCode::Internal, _) => code.prose().to_owned(),
        (_, Some(at)) => format!("{}: {}. {}", coordinates(at), error.message, code.prose()),
        (_, None) => format!("{file}: {}. {}", error.message, code.prose()),
    };
    error
}

/// Returns the location of `span` within `body`, offset past the
/// frontmatter lines, or `None` when either end cannot be located.
fn span_location(body: &str, span: Range<usize>, frontmatter_lines: u32) -> Option<Location> {
    let (start_line, start_column) = body_line_column(body, span.start, frontmatter_lines)?;
    let (end_line, end_column) = body_line_column(body, span.end, frontmatter_lines)?;
    Location::new(
        String::new(),
        start_line,
        start_column,
        end_line,
        end_column,
    )
}

/// Returns the 1-based file line and byte column of `byte_offset` within
/// `body`, offset past the frontmatter lines.
///
/// `None` when the offset is out of bounds or either value's arithmetic
/// overflows.
fn body_line_column(
    body: &str,
    byte_offset: usize,
    frontmatter_lines: u32,
) -> Option<(NonZeroU32, NonZeroU32)> {
    let prefix = body.get(..byte_offset)?;
    let line = u32::try_from(prefix.matches('\n').count())
        .ok()
        .and_then(|newlines| NonZeroU32::MIN.checked_add(newlines))
        .and_then(|line| line.checked_add(frontmatter_lines))?;
    let column = u32::try_from(prefix.len() - prefix.rfind('\n').map_or(0, |index| index + 1))
        .ok()
        .and_then(|offset| NonZeroU32::MIN.checked_add(offset))?;
    Some((line, column))
}

#[cfg(test)]
#[path = "error-tests.rs"]
mod tests;
