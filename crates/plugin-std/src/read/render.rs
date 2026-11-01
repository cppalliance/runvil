//! The render: turns a window of lines into the text Read answers with.
//!
//! Each line prints as `{number:>6}|{text}`: a six-wide right-aligned
//! number, a pipe, and no space. The number is always on and widens past
//! six digits, and it is not part of the line. The numbered lines are
//! followed by the notes and then the trailer, each on its own line, with
//! no newline after the last one.

use crate::check::plural;
use crate::text::clip_to_boundary;

use super::window::Window;

/// The numbered prefix of line `number`.
fn prefix(number: usize) -> String {
    format!("{number:>6}|")
}

/// The text of the `window` over `lines`, with `notes` and a trailer when
/// the read stops early.
///
/// The numbered lines hold at most `cap` bytes, counting each prefix, the
/// text, and its newline, and not the notes or the trailer. A line that
/// would pass the cap is left out, and the trailer names the offset that
/// continues. When the window's first line alone passes the cap, it is
/// cut to fit and printed alone, and the trailer says so: this is the only
/// cut inside a line. A read that reaches the last line prints no trailer.
///
/// `window` lies inside `lines`.
pub(super) fn render(lines: &[&str], window: &Window, notes: &[String], cap: usize) -> String {
    let total = lines.len();
    let mut body = String::new();
    let mut used = 0usize;
    let mut shown = 0usize;
    for number in window.first..=window.last {
        let Some(text) = lines.get(number - 1) else {
            break;
        };
        let prefix = prefix(number);
        let cost = prefix.len() + text.len() + 1;
        if used + cost > cap {
            break;
        }
        body.push_str(&prefix);
        body.push_str(text);
        body.push('\n');
        used += cost;
        shown = number;
    }
    let trailer = if shown == 0 {
        Some(cut_first_line(lines, window.first, cap, &mut body))
    } else if shown < window.last {
        Some(format!(
            "[Showing lines {first}-{shown} of {total} ({cap}-byte limit). \
             Use offset={next} to continue.]",
            first = window.first,
            next = shown + 1,
        ))
    } else if window.last < total {
        Some(format!(
            "[Showing lines {first}-{last} of {total}. Use offset={next} to continue.]",
            first = window.first,
            last = window.last,
            next = window.last + 1,
        ))
    } else {
        None
    };
    let tail: Vec<&str> = notes
        .iter()
        .map(String::as_str)
        .chain(trailer.as_deref())
        .collect();
    body.push_str(&tail.join("\n"));
    body
}

/// Prints the first line of the window, number `number`, cut to fit
/// `cap` bytes, into `body`, and returns the trailer that says it was
/// cut.
fn cut_first_line(lines: &[&str], number: usize, cap: usize, body: &mut String) -> String {
    let text = lines.get(number - 1).copied().unwrap_or_default();
    let prefix = prefix(number);
    let allowance = cap.saturating_sub(prefix.len() + 1);
    let kept = clip_to_boundary(text, 0, allowance);
    body.push_str(&prefix);
    body.push_str(kept);
    body.push('\n');
    let next = if number < lines.len() {
        format!(" Use offset={} to continue.", number + 1)
    } else {
        String::new()
    };
    format!(
        "[Line {number} is {bytes} bytes and is cut after {cut} bytes. \
         No parameter reads the rest of a line.{next}]",
        bytes = text.len(),
        cut = kept.len(),
    )
}

/// The notes for a file that was decoded, in the order they print: the
/// UTF-16 note first, then the note that counts replaced bytes.
pub(super) fn notes(utf16: Option<&str>, invalid: usize) -> Vec<String> {
    let mut notes = Vec::new();
    if let Some(name) = utf16 {
        notes.push(format!(
            "[Note: the file is {name} and was decoded to text.]"
        ));
    }
    if invalid > 0 {
        notes.push(format!(
            "[Note: {} in this file were shown as U+FFFD.]",
            plural(invalid, "invalid UTF-8 sequence")
        ));
    }
    notes
}
