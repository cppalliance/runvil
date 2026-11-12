//! How an index counts: 1 is the first and -1 is the last.

/// Returns the position, from 0, of the `n`th of `len` matches, counting
/// from 1 at the front, or from -1 at the back when `n` is negative.
/// Zero, and an `n` past either end, names nothing.
#[must_use]
pub fn position(len: usize, n: i64) -> Option<usize> {
    let count = i64::try_from(len).ok()?;
    let at = if n > 0 { n - 1 } else { count.checked_add(n)? };
    usize::try_from(at).ok().filter(|at| *at < len)
}

/// Returns the `n`th of `list`, as [`position`] counts.
#[must_use]
pub fn nth<T>(list: &[T], n: i64) -> Option<&T> {
    list.get(position(list.len(), n)?)
}

/// Says where several matches are, for the message that asks for an
/// index: `, at lines 20, 24, and 30`. Each of `lines` is the line of one
/// match, or `None` for a match that records none. The text is empty when
/// any line is unknown, so a message never names a line it cannot.
pub(crate) fn at_lines(lines: &[Option<i64>]) -> String {
    let Some(lines) = lines
        .iter()
        .map(|line| line.map(|line| line.to_string()))
        .collect::<Option<Vec<String>>>()
    else {
        return String::new();
    };
    match lines.as_slice() {
        [] => String::new(),
        [one] => format!(", at line {one}"),
        [first, second] => format!(", at lines {first} and {second}"),
        [rest @ .., last] => format!(", at lines {}, and {last}", rest.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::{at_lines, nth, position};

    #[test]
    fn a_position_counts_from_zero_and_only_inside_the_matches() {
        assert_eq!(position(3, 1), Some(0));
        assert_eq!(position(3, -1), Some(2));
        assert_eq!(position(3, 4), None);
        assert_eq!(position(0, 1), None);
    }

    #[test]
    fn the_lines_of_several_matches_read_as_a_sentence_lists_them() {
        assert_eq!(at_lines(&[Some(3), Some(7)]), ", at lines 3 and 7");
        assert_eq!(
            at_lines(&[Some(20), Some(24), Some(30)]),
            ", at lines 20, 24, and 30"
        );
        assert_eq!(at_lines(&[Some(5)]), ", at line 5");
        // One unknown line leaves the lines out altogether.
        assert_eq!(at_lines(&[Some(3), None]), "");
        assert_eq!(at_lines(&[]), "");
    }

    #[test]
    fn a_positive_index_counts_from_the_front_and_a_negative_one_from_the_back() {
        let list = ["a", "b", "c"];
        assert_eq!(nth(&list, 1), Some(&"a"));
        assert_eq!(nth(&list, 3), Some(&"c"));
        assert_eq!(nth(&list, -1), Some(&"c"));
        assert_eq!(nth(&list, -3), Some(&"a"));
    }

    #[test]
    fn zero_and_an_index_past_either_end_name_nothing() {
        let list = ["a", "b", "c"];
        for n in [0, 4, -4, i64::MAX, i64::MIN] {
            assert_eq!(nth(&list, n), None, "{n}");
        }
        assert_eq!(nth::<&str>(&[], 1), None);
        assert_eq!(nth::<&str>(&[], -1), None);
    }
}
