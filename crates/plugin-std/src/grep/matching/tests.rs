//! Unit tests for the matching-line stream.

use regex::RegexBuilder;

use super::{each_match_line, line_of, line_starts};
use crate::text::split_lines;

/// The matching lines of `text` for `pattern`, each with its first-match
/// offset, using the flags Grep compiles with.
fn matches(pattern: &str, multiline: bool, text: &str) -> Vec<(usize, usize)> {
    let regex = RegexBuilder::new(pattern)
        .multi_line(true)
        .dot_matches_new_line(multiline)
        .crlf(multiline)
        .build()
        .expect("the pattern compiles");
    let lines = split_lines(text);
    let mut found = Vec::new();
    each_match_line(&regex, multiline, text, &lines, |line, offset| {
        found.push((line, offset));
        true
    });
    found
}

#[test]
fn line_by_line_reports_each_matching_line_once_with_its_first_match() {
    assert_eq!(
        matches("a", false, "xa\nb\nana\n"),
        [(0, 1), (2, 0)],
        "the second match on a line does not repeat the line"
    );
}

#[test]
fn a_multiline_match_touches_every_line_from_its_first_byte_to_its_last() {
    assert_eq!(
        matches("b\nc", true, "a\nb\nc\nd\n"),
        [(1, 0), (2, 0)],
        "lines 2 and 3"
    );
    assert_eq!(
        matches("b\n", true, "a\nb\nc\n"),
        [(1, 0)],
        "a match that ends with the newline touches only the line it ends"
    );
}

#[test]
fn a_line_two_matches_touch_is_reported_once_with_the_first_matchs_offset() {
    assert_eq!(
        matches("a\nb|c", true, "xa\nbc\n"),
        [(0, 1), (1, 0)],
        "line 2 holds the end of one match and the whole of another"
    );
}

#[test]
fn an_empty_match_touches_the_line_it_starts_on_and_none_after_a_final_newline() {
    assert_eq!(matches("$", true, "a\nb\n"), [(0, 1), (1, 1)]);
    assert_eq!(matches("$", true, "a"), [(0, 1)]);
    assert_eq!(matches("^", true, ""), []);
}

#[test]
fn the_stream_stops_when_the_callback_says_so() {
    let regex = RegexBuilder::new("a").build().expect("compiles");
    let text = "a\na\na\n";
    let lines = split_lines(text);
    let mut seen = 0;
    each_match_line(&regex, false, text, &lines, |_, _| {
        seen += 1;
        seen < 2
    });
    assert_eq!(seen, 2);
}

#[test]
fn line_starts_agree_with_split_lines_for_every_shape_of_ending() {
    for text in ["", "a", "a\n", "a\n\n", "\n", "a\r\nb\r\n", "a\n\nb"] {
        assert_eq!(line_starts(text).len(), split_lines(text).len(), "{text:?}");
    }
}

#[test]
fn line_of_places_offsets_on_the_line_that_holds_them() {
    let text = "a\n\nb";
    let starts = line_starts(text);
    assert_eq!(starts, [0, 2, 3]);
    assert_eq!(line_of(&starts, text, 0), Some(0));
    assert_eq!(line_of(&starts, text, 1), Some(0));
    assert_eq!(line_of(&starts, text, 2), Some(1));
    assert_eq!(line_of(&starts, text, 4), Some(2));
    assert_eq!(line_of(&line_starts("a\n"), "a\n", 2), None);
}
