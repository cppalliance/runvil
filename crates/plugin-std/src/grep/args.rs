//! Grep's arguments: what the call asks for, and what is refused before
//! any path is looked at.

use regex::{Regex, RegexBuilder};
use runvil_plugin::Error;
use serde_json::Value;

use super::types;
use crate::check::{invalid_arguments, invalid_pattern, refuse_blank, refuse_unknown};
use crate::matcher::Matcher;
use crate::spec::Definition;

/// What Grep prints for each match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    /// The matching lines, with their context.
    Content,
    /// The paths of the files that match.
    FilesWithMatches,
    /// Each matching file's path and its count of matching lines.
    Count,
}

/// What a call asks Grep for.
#[derive(Debug)]
pub(super) struct Request<'a> {
    /// The file or directory to search, as the model wrote it. `None`
    /// means the filesystem root.
    pub(super) path: Option<&'a str>,
    /// The `glob` filter as the model wrote it, and compiled.
    pub(super) glob: Option<(&'a str, Matcher)>,
    /// The extensions the `type` filter keeps.
    pub(super) kind: Option<&'static [&'static str]>,
    /// What to print for each match.
    pub(super) mode: Mode,
    /// The context lines shown before each match, in `content` mode.
    pub(super) before: usize,
    /// The context lines shown after each match, in `content` mode.
    pub(super) after: usize,
    /// The most entries to show, or `None` for no limit.
    pub(super) limit: Option<usize>,
    /// The entries to skip first.
    pub(super) offset: usize,
    /// Whether a match may span lines, so the whole text is one subject.
    pub(super) multiline: bool,
    /// The compiled pattern.
    pub(super) regex: Regex,
}

/// The one key a call may pass that the schema does not declare: Cursor's
/// `-n`, which a model may still send, and which is accepted when it is
/// `true`.
const ACCEPTED_UNDECLARED: &str = "-n";

/// Reads the call's arguments after the shared schema check.
///
/// The order is fixed, so a call that breaks two rules always hears about
/// the same one: an undeclared key (the one undeclared key `-n` is
/// accepted), the `-n` value, an empty pattern, a negative `-A`, `-B`, or
/// `-C`, a negative `head_limit` or `offset`, an unknown `type`, an
/// invalid `glob`, a blank `path`, and last the pattern itself, which is
/// compiled before any filesystem call.
///
/// # Errors
/// Returns an invalid-arguments [`Error`] naming the parameter.
pub(super) fn parse<'a>(definition: &Definition, args: &'a Value) -> Result<Request<'a>, Error> {
    let tool = definition.wire_name.as_str();
    let mut declared = args.clone();
    if let Some(object) = declared.as_object_mut() {
        object.remove(ACCEPTED_UNDECLARED);
    }
    refuse_unknown(tool, &definition.parameters, &declared)?;
    refuse_line_numbers_off(tool, args)?;
    let pattern = args
        .get("pattern")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if pattern.is_empty() {
        return Err(invalid_arguments(
            tool,
            "`pattern` is empty. Pass \".\" to match every line.",
        ));
    }
    let after = number(tool, args, "-A", "context lines")?;
    let before = number(tool, args, "-B", "context lines")?;
    let both = number(tool, args, "-C", "context lines")?;
    let limit = number(tool, args, "head_limit", "")?.map(|value| count(value, true));
    let offset = number(tool, args, "offset", "")?.map(|value| count(value, false));
    let kind = kind(tool, args)?;
    let glob = glob(tool, args)?;
    let path = path(tool, args)?;
    let mode = mode(args);
    let (before, after) = if mode == Mode::Content {
        (
            before.or(both).map_or(0, |value| count(value, true)),
            after.or(both).map_or(0, |value| count(value, true)),
        )
    } else {
        (0, 0)
    };
    let multiline = flag(args, "multiline");
    let regex = compile(tool, pattern, args, multiline)?;
    Ok(Request {
        path,
        glob,
        kind,
        mode,
        before,
        after,
        limit: limit.filter(|limit| *limit > 0),
        offset: offset.unwrap_or_default(),
        multiline,
        regex,
    })
}

/// Refuses `-n` unless it is `true` or left out, because Grep always shows
/// line numbers.
fn refuse_line_numbers_off(tool: &str, args: &Value) -> Result<(), Error> {
    match args.get(ACCEPTED_UNDECLARED) {
        None | Some(Value::Null | Value::Bool(true)) => Ok(()),
        Some(_) => Err(invalid_arguments(
            tool,
            "`-n` must be true or left out, because line numbers are always shown.",
        )),
    }
}

/// The number under `name`, or `None` when the call left it out or passed
/// `null`, refused when it is negative.
///
/// `unit` names what a context line count counts, and is empty for
/// `head_limit` and `offset`, whose refusal is shorter.
fn number(tool: &str, args: &Value, name: &str, unit: &str) -> Result<Option<f64>, Error> {
    let Some(value) = args.get(name).filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let Some(number) = value.as_f64() else {
        return Ok(None);
    };
    if number < 0.0 {
        let ask = if unit.is_empty() {
            "Pass 0 or more.".to_owned()
        } else {
            format!("Pass 0 or more {unit}.")
        };
        return Err(invalid_arguments(
            tool,
            format!("`{name}` is {value}. {ask}"),
        ));
    }
    Ok(Some(number))
}

/// The whole number `value` rounds to, up or down. A value past the range
/// of `usize` is the largest `usize`.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is not negative, and a float to integer cast saturates at the largest usize"
)]
fn count(value: f64, up: bool) -> usize {
    let whole = if up { value.ceil() } else { value.floor() };
    whole as usize
}

/// The extensions of the `type` the call names, or `None` when it names
/// none. An empty string names none.
fn kind(tool: &str, args: &Value) -> Result<Option<&'static [&'static str]>, Error> {
    let Some(name) = text(args, "type") else {
        return Ok(None);
    };
    types::extensions(name).map(Some).ok_or_else(|| {
        invalid_arguments(
            tool,
            format!(
                "Unrecognized file type \"{name}\". Use a type such as js, ts, py, rust, go or \
                 java, or filter with glob."
            ),
        )
    })
}

/// The `glob` the call names, compiled, or `None` when it names none.
fn glob<'a>(tool: &str, args: &'a Value) -> Result<Option<(&'a str, Matcher)>, Error> {
    let Some(pattern) = text(args, "glob") else {
        return Ok(None);
    };
    Matcher::new(pattern)
        .map(|matcher| Some((pattern, matcher)))
        .map_err(|error| invalid_pattern(tool, "glob", pattern, error))
}

/// The `path` the call names, or `None` when it names none, refused when
/// it is only whitespace.
fn path<'a>(tool: &str, args: &'a Value) -> Result<Option<&'a str>, Error> {
    let Some(path) = text(args, "path") else {
        return Ok(None);
    };
    refuse_blank(tool, "path", path)?;
    Ok(Some(path))
}

/// The string under `name`, or `None` when the call left it out, passed
/// `null`, or passed an empty string.
fn text<'a>(args: &'a Value, name: &str) -> Option<&'a str> {
    args.get(name)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
}

/// The output mode: `content` unless the call names another.
fn mode(args: &Value) -> Mode {
    match args.get("output_mode").and_then(Value::as_str) {
        Some("files_with_matches") => Mode::FilesWithMatches,
        Some("count") => Mode::Count,
        _ => Mode::Content,
    }
}

/// The boolean under `name`, `false` when the call left it out.
fn flag(args: &Value, name: &str) -> bool {
    args.get(name).and_then(Value::as_bool).unwrap_or(false)
}

/// Compiles the pattern with the call's flags.
///
/// Matching is case-sensitive unless `-i` is set, and `^` and `$` match at
/// each line. With `multiline`, `.` also matches a line break and a line
/// break is `\r\n` or `\n`. `literal` matches the pattern as plain text.
fn compile(tool: &str, pattern: &str, args: &Value, multiline: bool) -> Result<Regex, Error> {
    let source = if flag(args, "literal") {
        regex::escape(pattern)
    } else {
        pattern.to_owned()
    };
    RegexBuilder::new(&source)
        .case_insensitive(flag(args, "-i"))
        .multi_line(true)
        .dot_matches_new_line(multiline)
        .crlf(multiline)
        .build()
        .map_err(|error| {
            // The crate's syntax errors echo the pattern and then end with
            // `error: <reason>`, so the reason is what follows the last one.
            let message = error.to_string();
            let reason = message
                .rsplit_once("error: ")
                .map_or(message.trim(), |(_, reason)| reason.trim());
            invalid_arguments(
                tool,
                format!(
                    "Invalid regular expression \"{pattern}\": {reason}. Grep matches with Rust \
                     regex syntax, which has no look-around and no back-references. To match the \
                     text literally, set literal to true."
                ),
            )
        })
}
