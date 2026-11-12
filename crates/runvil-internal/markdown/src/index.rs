//! How an index counts: 1 is the first and -1 is the last.

/// Returns the `n`th of `list`, counting from 1 at the front, or from -1
/// at the back when `n` is negative. Zero, and an `n` past either end,
/// names nothing.
#[must_use]
pub fn nth<T>(list: &[T], n: i64) -> Option<&T> {
    let len = i64::try_from(list.len()).ok()?;
    let at = if n > 0 { n - 1 } else { len.checked_add(n)? };
    list.get(usize::try_from(at).ok()?)
}

#[cfg(test)]
mod tests {
    use super::nth;

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
