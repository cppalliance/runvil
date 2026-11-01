//! The window: which lines of a file an offset and a limit pick.

/// A run of lines, as the numbers of its first and last line, counting
/// from 1. Both ends are inside the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Window {
    /// The first line, at least 1.
    pub(super) first: usize,
    /// The last line, at least `first` and at most the line count.
    pub(super) last: usize,
}

/// The window an `offset` and a `limit` pick over a file of `total` lines.
///
/// `total` is at least 1, and `limit`, when there is one, is at least 1.
/// An absent offset starts at line 1, and so does an offset of `0`. A
/// positive offset starts there, and picks nothing (`None`) when it is
/// past the last line. A negative offset counts back from the end, so
/// `-1` is the last line, and it never picks nothing: it stops at line 1
/// when it reaches past the start. The window ends at the last line, or
/// `limit` lines after its start when that is sooner.
pub(super) fn window(total: usize, offset: Option<i64>, limit: Option<usize>) -> Option<Window> {
    let first = match offset {
        None | Some(0) => 1,
        Some(offset) if offset > 0 => {
            let first = usize::try_from(offset).unwrap_or(usize::MAX);
            if first > total {
                return None;
            }
            first
        }
        Some(offset) => {
            let back = usize::try_from(offset.unsigned_abs()).unwrap_or(usize::MAX);
            total.saturating_sub(back) + 1
        }
    };
    let last = limit.map_or(total, |limit| {
        first.saturating_add(limit.saturating_sub(1)).min(total)
    });
    Some(Window { first, last })
}
