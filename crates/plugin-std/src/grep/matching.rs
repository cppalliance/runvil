//! Finding the matching lines of one decoded file.
//!
//! Outside `multiline` a line is a subject of its own. With it the whole
//! text is one subject, and every line a match touches is a matching line,
//! so a line number here is always the number Read prints for that line.

use regex::Regex;

/// Calls `emit` for each matching line of `text`, in line order and once
/// per line, until `emit` returns `false`.
///
/// `lines` is `text` split the way Read splits it. `emit` gets the line's
/// index, from 0, and the byte offset in that line of the first match
/// that covers it: where the first match starts when it starts on this
/// line, and 0 for a line a match enters from an earlier one.
pub(super) fn each_match_line(
    regex: &Regex,
    multiline: bool,
    text: &str,
    lines: &[&str],
    mut emit: impl FnMut(usize, usize) -> bool,
) {
    if !multiline {
        for (index, line) in lines.iter().enumerate() {
            if let Some(found) = regex.find(line)
                && !emit(index, found.start())
            {
                return;
            }
        }
        return;
    }
    let starts = line_starts(text);
    // The first line no match has been reported for yet.
    let mut next = 0;
    for found in regex.find_iter(text) {
        let Some(first) = line_of(&starts, text, found.start()) else {
            continue;
        };
        // An empty match touches only the line it starts on.
        let last_byte = found.end().saturating_sub(1).max(found.start());
        let last = line_of(&starts, text, last_byte).unwrap_or(first);
        let first_start = starts.get(first).copied().unwrap_or_default();
        for line in first.max(next)..=last {
            let offset = if line == first {
                found.start() - first_start
            } else {
                0
            };
            let width = lines.get(line).map_or(0, |line| line.len());
            if !emit(line, offset.min(width)) {
                return;
            }
        }
        next = next.max(last + 1);
    }
}

/// The byte offset where each line of `text` starts, split the way
/// `split_lines` splits: on `\n`, with no line after a final newline.
fn line_starts(text: &str) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut start = 0;
    for piece in text.split_terminator('\n') {
        starts.push(start);
        start += piece.len() + 1;
    }
    starts
}

/// The index of the line that holds byte `offset` of `text`, or `None`
/// when no line does: the text is empty, or `offset` is the end of a text
/// that ends in a newline.
fn line_of(starts: &[usize], text: &str, offset: usize) -> Option<usize> {
    let after = starts.partition_point(|start| *start <= offset);
    let line = after.checked_sub(1)?;
    if offset == text.len() && text.ends_with('\n') {
        return None;
    }
    Some(line)
}

#[cfg(test)]
mod tests;
