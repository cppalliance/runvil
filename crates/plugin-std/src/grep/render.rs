//! The render: turns what a search found into the text Grep answers with.
//!
//! There is no envelope. The answer is the body, and then, when there is
//! anything to add, a blank line and the extra lines: the replacement
//! note, the skip line, and the paging or limit notice. The extra lines
//! never count against the body cap.

use runvil_plugin::ToolOutput;

use super::args::{Mode, Request};
use super::found::{Found, Limits, Reason, Stop};
use crate::check::plural;

/// The line printed between two groups of one file that are not next to
/// each other, when context is on.
const GAP: &str = "  --";

/// What a gap line costs against the cap, with its newline.
pub(super) const GAP_COST: usize = GAP.len() + 1;

/// The line that tells a model nothing was filtered by ignore rules.
const IGNORE_NOTE: &str =
    "[No ignore rules ran: .gitignore is not applied, and only entries named .git are skipped.]";

/// The line that says what `{`, `[`, and `?` do in a glob.
const GLOB_HINT: &str = "{, [ and ? in glob match themselves; there is no alternation or \
                         character class. Call Grep once per alternative, or filter with type.";

/// What the body cap costs a line: its text and its newline.
fn line_cost(text_len: usize) -> usize {
    text_len + 1
}

/// What a `content` line costs against the cap: two spaces, the number,
/// the `:` or `-`, the text, and the newline.
pub(super) fn row_cost(number: usize, text: &str) -> usize {
    line_cost(2 + number.to_string().len() + 1 + text.len())
}

/// What a `files_with_matches` or `count` line costs against the cap.
pub(super) fn summary_cost(mode: Mode, path: &str, count: usize) -> usize {
    match mode {
        Mode::Count => line_cost(path.len() + 1 + count.to_string().len()),
        _ => line_cost(path.len()),
    }
}

/// What kind of line a row is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A file's path, which opens its group in `content` mode.
    Path,
    /// The empty line between two files.
    Blank,
    /// The gap line.
    Gap,
    /// A matching line.
    Match,
    /// A context line.
    Context,
    /// A path, or a path and its count, in the other two modes.
    Summary,
}

/// One line of the body.
#[derive(Debug)]
struct Out {
    /// The line, without its newline.
    text: String,
    /// What the line is.
    kind: Kind,
    /// The line number of a matching or context line, and 0 for others.
    line: usize,
}

/// The body: the lines that fit under the cap.
#[derive(Debug)]
struct Body {
    /// The lines joined with `\n`.
    text: String,
    /// How many entries the lines show.
    shown: usize,
    /// Whether the cap cut the body short.
    cut: bool,
}

/// The answer to a search that found `found`.
pub(super) fn render(found: &Found, request: &Request<'_>, limits: &Limits) -> ToolOutput {
    let note = replacement_note(found);
    let skip = skip_line(found, limits);
    let stopped = found.stop.map(|stop| limit_notice(stop, limits));
    if found.seen == 0 {
        let text = format!(
            "No matches found. Searched {}.",
            plural(found.searched, "file")
        );
        let trusted = note.is_none() && skip.is_none();
        let mut extras: Vec<String> = [note, skip].into_iter().flatten().collect();
        extras.push(IGNORE_NOTE.to_owned());
        if request
            .glob
            .as_ref()
            .is_some_and(|(glob, _)| glob.contains(['{', '[', '?']))
        {
            extras.push(GLOB_HINT.to_owned());
        }
        extras.extend(stopped);
        let text = assemble(text, &extras);
        return if trusted {
            ToolOutput::trusted(text)
        } else {
            ToolOutput::untrusted(text)
        };
    }
    if found.collected == 0 {
        let extras: Vec<String> = stopped.into_iter().collect();
        let text = format!("No more results at offset={}.", request.offset);
        return ToolOutput::untrusted(assemble(text, &extras));
    }
    let body = body(found, request, limits.body_cap);
    let capped = body.cut || found.capped;
    let text = if body.shown == 0 {
        format!(
            "[Output stopped at {} bytes before the first result fit. Narrow the search with \
             path, glob or type.]",
            limits.body_cap
        )
    } else {
        body.text
    };
    let tail = if body.shown == 0 {
        stopped
    } else {
        stopped.or_else(|| paging_notice(found, request, body.shown, capped, limits))
    };
    let extras: Vec<String> = [note, skip, tail].into_iter().flatten().collect();
    ToolOutput::untrusted(assemble(text, &extras))
}

/// `body`, then a blank line and the `extras` on their own lines, when
/// there are any.
fn assemble(mut body: String, extras: &[String]) -> String {
    if !extras.is_empty() {
        body.push_str("\n\n");
        body.push_str(&extras.join("\n"));
    }
    body
}

/// The body for `found`, with at most `cap` bytes of lines.
fn body(found: &Found, request: &Request<'_>, cap: usize) -> Body {
    let rows = match request.mode {
        Mode::Content => content_rows(found, request.before > 0 || request.after > 0),
        Mode::FilesWithMatches => found
            .hits
            .iter()
            .map(|hit| Out {
                text: hit.path.clone(),
                kind: Kind::Summary,
                line: 0,
            })
            .collect(),
        Mode::Count => found
            .hits
            .iter()
            .map(|hit| Out {
                text: format!("{}:{}", hit.path, hit.count),
                kind: Kind::Summary,
                line: 0,
            })
            .collect(),
    };
    cut(rows, cap, request)
}

/// The `content` lines of every file that kept one: its path, then its
/// lines in order, with a gap line between two groups that are not next
/// to each other when `gaps` is on, and a blank line between files.
fn content_rows(found: &Found, gaps: bool) -> Vec<Out> {
    let mut rows = Vec::new();
    for hit in found.hits.iter().filter(|hit| !hit.rows.is_empty()) {
        if !rows.is_empty() {
            rows.push(Out {
                text: String::new(),
                kind: Kind::Blank,
                line: 0,
            });
        }
        rows.push(Out {
            text: hit.path.clone(),
            kind: Kind::Path,
            line: 0,
        });
        let mut previous: Option<usize> = None;
        for (number, row) in &hit.rows {
            if gaps && previous.is_some_and(|previous| *number > previous + 1) {
                rows.push(Out {
                    text: GAP.to_owned(),
                    kind: Kind::Gap,
                    line: 0,
                });
            }
            previous = Some(*number);
            let mark = if row.is_match { ':' } else { '-' };
            rows.push(Out {
                text: format!("  {number}{mark}{}", row.text),
                kind: if row.is_match {
                    Kind::Match
                } else {
                    Kind::Context
                },
                line: *number,
            });
        }
    }
    rows
}

/// `rows` cut before the line that would pass `cap`.
///
/// A cut that falls inside a file's lines leaves nothing dangling: the
/// body ends at the last matching line it shows, or at the last of that
/// line's own trailing context lines, never at a path, a gap, or the
/// leading context of a match that did not fit.
fn cut(mut rows: Vec<Out>, cap: usize, request: &Request<'_>) -> Body {
    let mut used = 0;
    let mut keep = rows.len();
    for (index, row) in rows.iter().enumerate() {
        let cost = line_cost(row.text.len());
        if used + cost > cap {
            keep = index;
            break;
        }
        used += cost;
    }
    let cut = keep < rows.len();
    rows.truncate(keep);
    if cut && request.mode == Mode::Content {
        let end = rows
            .iter()
            .rposition(|row| row.kind == Kind::Match)
            .map_or(0, |last| {
                let limit = rows[last].line.saturating_add(request.after);
                let trailing = rows[last + 1..]
                    .iter()
                    .take_while(|row| row.kind == Kind::Context && row.line <= limit)
                    .count();
                last + 1 + trailing
            });
        rows.truncate(end);
    }
    let shown = rows
        .iter()
        .filter(|row| matches!(row.kind, Kind::Match | Kind::Summary))
        .count();
    let text = rows
        .iter()
        .map(|row| row.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    Body { text, shown, cut }
}

/// The paging notice, or `None` when there is nothing to say.
///
/// There is no cache, so each call runs from the start. A cut says where
/// to continue. A page after the first that reaches the end gives an
/// exact total. A first page that nothing cut gets no notice.
fn paging_notice(
    found: &Found,
    request: &Request<'_>,
    shown: usize,
    capped: bool,
    limits: &Limits,
) -> Option<String> {
    let offset = request.offset;
    let unit = if request.mode == Mode::Content {
        "matching lines"
    } else {
        "files"
    };
    let cut = capped || found.extra;
    if !cut && offset == 0 {
        return None;
    }
    let first = offset + 1;
    let last = offset + shown;
    let total = offset + found.collected;
    if !cut {
        return Some(format!("[Showing {first}-{last} of {total} {unit}.]"));
    }
    let stopped = if capped {
        format!(" Output stopped at {} bytes.", limits.body_cap)
    } else {
        String::new()
    };
    Some(format!(
        "[Showing {first}-{last} of at least {total} {unit}.{stopped} Use offset={last} to see \
         more, or narrow the search.]"
    ))
}

/// The note that names the files whose invalid UTF-8 was replaced.
fn replacement_note(found: &Found) -> Option<String> {
    let replaced = &found.replaced;
    if replaced.count == 0 {
        return None;
    }
    let mut paths = replaced.first.join(", ");
    if replaced.count > replaced.first.len() {
        paths = format!(
            "{paths}, and {} more",
            replaced.count - replaced.first.len()
        );
    }
    Some(format!(
        "[Note: invalid UTF-8 in {} was shown as U+FFFD: {paths}]",
        plural(replaced.count, "file")
    ))
}

/// The line that counts the skipped entries by reason and names the first
/// few of them.
fn skip_line(found: &Found, limits: &Limits) -> Option<String> {
    let skips = &found.skips;
    let total = skips.total();
    if total == 0 {
        return None;
    }
    let phrases: Vec<String> = Reason::ALL
        .iter()
        .filter_map(|reason| {
            let n = skips.count(*reason);
            (n > 0).then(|| phrase(*reason, n, limits))
        })
        .collect();
    let first = if skips.first().is_empty() {
        String::new()
    } else {
        format!(" First: {}", skips.first().join(", "))
    };
    Some(format!(
        "[Skipped {}: {}.{first}]",
        plural(total, "entry"),
        phrases.join(", ")
    ))
}

/// The phrase that says `n` entries were skipped for `reason`.
fn phrase(reason: Reason, n: usize, limits: &Limits) -> String {
    match reason {
        Reason::Binary => plural(n, "binary file"),
        Reason::UnreadableDirectory => {
            format!("{} that could not be read", plural(n, "directory"))
        }
        Reason::UnreadableFile => format!("{} that could not be read", plural(n, "file")),
        Reason::OverSize => format!("{} over {} bytes", plural(n, "file"), limits.max_file_size),
        Reason::SymbolicLink => plural(n, "symbolic link"),
        Reason::ControlName => format!("{} with a control character", plural(n, "name")),
        Reason::Conflict => format!("{} held by another call in this round", plural(n, "entry")),
        Reason::Denied => format!("{} denied by policy", plural(n, "entry")),
    }
}

/// The notice for a search a limit ended early. It offers no `offset`,
/// because a re-run stops at the same place.
fn limit_notice(stop: Stop, limits: &Limits) -> String {
    match stop {
        Stop::Files => format!(
            "[Search stopped after considering {} files. Narrow path, glob or type to search the \
             rest.]",
            limits.max_files
        ),
        Stop::Bytes => format!(
            "[Search stopped after reading {} bytes. Narrow path, glob or type to search the \
             rest.]",
            limits.max_bytes
        ),
    }
}
