//! The two cuts Grep makes inside a line: a window around a match, and
//! the head of a context line.
//!
//! Both cuts go through `clip_to_boundary`, so neither splits a code
//! point, and both say how many bytes they left out, so a model can tell
//! that a line is not whole.

use crate::text::clip_to_boundary;

/// How much of a long line Grep shows.
#[derive(Debug, Clone, Copy)]
pub(super) struct Window {
    /// The most bytes of a line shown: a longer line is cut.
    pub(super) width: usize,
    /// How many bytes before the first match a window starts.
    pub(super) lead: usize,
}

impl Default for Window {
    /// 1,000 bytes, starting 200 bytes before the first match.
    fn default() -> Window {
        Window {
            width: 1000,
            lead: 200,
        }
    }
}

/// The marker that says how many bytes of a line are not shown.
fn omitted(bytes: usize) -> String {
    format!("[... {bytes} bytes omitted]")
}

/// The text shown for a matching `line`, whose first match starts at byte
/// `first`.
///
/// A line within the width is shown whole. A longer line becomes a window
/// that starts up to `lead` bytes before the match and holds at most
/// `width` bytes, with a marker directly before it when the window does
/// not start at the line's start and directly after it when the window
/// stops short of the line's end. The two counts plus the window's length
/// equal the line's length.
pub(super) fn match_text(line: &str, first: usize, window: &Window) -> String {
    if line.len() <= window.width {
        return line.to_owned();
    }
    let start = first.saturating_sub(window.lead);
    let shown = clip_to_boundary(line, start, start.saturating_add(window.width));
    // `clip_to_boundary` moves the start up to a character boundary, so the
    // bytes before the window are counted from that same boundary.
    let mut begin = start.min(line.len());
    while !line.is_char_boundary(begin) {
        begin += 1;
    }
    let after = line.len() - begin - shown.len();
    let mut text = String::new();
    if begin > 0 {
        text.push_str(&omitted(begin));
    }
    text.push_str(shown);
    if after > 0 {
        text.push_str(&omitted(after));
    }
    text
}

/// The text shown for a context `line`: the line when it is within the
/// width, and otherwise its first `width` bytes followed by a marker that
/// counts the rest.
pub(super) fn context_text(line: &str, window: &Window) -> String {
    if line.len() <= window.width {
        return line.to_owned();
    }
    let shown = clip_to_boundary(line, 0, window.width);
    format!("{shown}{}", omitted(line.len() - shown.len()))
}
