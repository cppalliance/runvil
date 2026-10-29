//! The store's read bodies: whole-file and line-range reads, plain or numbered.

use std::fmt::Write as _;

use runvil_vfs::{Access, VfsError};

/// Whether a store read prefixes each line with its absolute line number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineNumbers {
    /// The file's text verbatim.
    Omitted,
    /// Each line right-aligned behind its number and `"| "`.
    Prefixed,
}

/// Reads the file at `path` under the optional line bounds `store.read`
/// and `store.read_numbered` share, numbering lines as `numbers` says.
///
/// No `start` reads the whole file; a present `start` slices a 1-based
/// inclusive line range. A negative bound converts to 0, which the range
/// validation rejects with the same error a zero bound produces, and an
/// `end` without a `start` is refused rather than silently ignored.
fn read_store_bounded(
    view: &Access,
    path: &str,
    start: Option<i64>,
    end: Option<i64>,
    numbers: LineNumbers,
) -> std::result::Result<String, VfsError> {
    let render: fn(&[&str], usize) -> String = match numbers {
        LineNumbers::Prefixed => number_lines_from,
        LineNumbers::Omitted => |lines, _| lines.join("\n"),
    };
    match start {
        None if end.is_some() => Err(VfsError::InvalidRange {
            path: path.to_owned(),
            reason: "start is required when end is given",
        }),
        None if numbers == LineNumbers::Omitted => view.read_string(path),
        None => with_read_range(view, path, 1, None, render),
        Some(start) => {
            let start = usize::try_from(start).unwrap_or(0);
            let end = end.map(|line| usize::try_from(line).unwrap_or(0));
            with_read_range(view, path, start, end, render)
        }
    }
}

/// Runs the `store.read` operation both store paths share.
pub(super) fn read_store(
    view: &Access,
    path: &str,
    start: Option<i64>,
    end: Option<i64>,
) -> std::result::Result<String, VfsError> {
    read_store_bounded(view, path, start, end, LineNumbers::Omitted)
}

/// Runs the `store.read_numbered` operation both store paths share.
pub(super) fn read_store_numbered(
    view: &Access,
    path: &str,
    start: Option<i64>,
    end: Option<i64>,
) -> std::result::Result<String, VfsError> {
    read_store_bounded(view, path, start, end, LineNumbers::Prefixed)
}

/// Reads and resolves one line range while its owned contents remain live.
fn with_read_range(
    view: &Access,
    path: &str,
    start: usize,
    end: Option<usize>,
    render: impl FnOnce(&[&str], usize) -> String,
) -> std::result::Result<String, VfsError> {
    let contents = view.read_string(path)?;
    let lines: Vec<&str> = contents.lines().collect();
    let Some((start, end)) = resolve_line_range(path, lines.len(), start, end)? else {
        return Ok(String::new());
    };
    Ok(render(&lines[start - 1..end], start))
}

/// Resolves 1-based inclusive bounds against `line_count` into the
/// effective `(start, end)`, or `None` when the range falls entirely past
/// the last line. Evaluation order is fixed: a `start` below 1 is an
/// error; a `start` past the last line reads as empty; an omitted `end`
/// means the last line, and a given `end` clamps down to it; an `end`
/// before `start` at that point is an error.
fn resolve_line_range(
    path: &str,
    line_count: usize,
    start: usize,
    end: Option<usize>,
) -> std::result::Result<Option<(usize, usize)>, VfsError> {
    if start == 0 {
        return Err(VfsError::InvalidRange {
            path: path.to_owned(),
            reason: "start must be at least 1",
        });
    }
    if start > line_count {
        return Ok(None);
    }
    let end = end.unwrap_or(line_count).min(line_count);
    if end < start {
        return Err(VfsError::InvalidRange {
            path: path.to_owned(),
            reason: "end must not be before start",
        });
    }
    Ok(Some((start, end)))
}

/// Renders `lines` numbered absolutely from `start`, each number
/// right-aligned to the width of the largest emitted number, followed by
/// `"| "`; lines are joined with `"\n"` and there is no trailing newline.
fn number_lines_from(lines: &[&str], start: usize) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let last = start + lines.len() - 1;
    let width = last.to_string().len();
    let mut out = String::new();
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let number = start + index;
        // Writing to a String is infallible, so the result is discarded.
        let _ = write!(out, "{number:>width$}| {line}");
    }
    out
}
