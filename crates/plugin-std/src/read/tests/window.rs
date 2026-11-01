//! The window: which lines an offset and a limit pick, and what Read
//! prints for them.

use super::{failure_of, numbered_file, read, read_capped, seeded, text_of};
use crate::read::BYTE_CAP;
use crate::read::window::{Window, window};
use serde_json::json;

/// The window of a 10-line file, as first and last line numbers.
fn ten(offset: Option<i64>, limit: Option<usize>) -> Option<(usize, usize)> {
    window(10, offset, limit).map(|picked| (picked.first, picked.last))
}

/// An offset, a limit, and the first and last line they pick.
type Case = (Option<i64>, Option<usize>, Option<(usize, usize)>);

#[test]
fn the_window_table_on_a_ten_line_file() {
    let table: [Case; 13] = [
        (None, None, Some((1, 10))),
        (Some(0), None, Some((1, 10))),
        (Some(1), None, Some((1, 10))),
        (Some(10), None, Some((10, 10))),
        (Some(-1), None, Some((10, 10))),
        (Some(-2), None, Some((9, 10))),
        (Some(-100), None, Some((1, 10))),
        (Some(-2), Some(5), Some((9, 10))),
        (None, Some(3), Some((1, 3))),
        (Some(4), Some(3), Some((4, 6))),
        (Some(9), Some(5), Some((9, 10))),
        (Some(11), None, None),
        (Some(11), Some(3), None),
    ];
    for (offset, limit, expected) in table {
        assert_eq!(
            ten(offset, limit),
            expected,
            "offset {offset:?}, limit {limit:?}"
        );
    }
}

#[test]
fn the_extremes_of_both_arguments_do_not_overflow() {
    assert_eq!(ten(Some(i64::MIN), None), Some((1, 10)));
    assert_eq!(ten(Some(i64::MAX), None), None);
    assert_eq!(ten(None, Some(usize::MAX)), Some((1, 10)));
    assert_eq!(ten(Some(5), Some(usize::MAX)), Some((5, 10)));
    assert_eq!(ten(Some(-3), Some(usize::MAX)), Some((8, 10)));
}

#[test]
fn a_one_line_file_has_one_window() {
    assert_eq!(window(1, None, None), Some(Window { first: 1, last: 1 }));
    assert_eq!(
        window(1, Some(-1), None),
        Some(Window { first: 1, last: 1 })
    );
    assert_eq!(window(1, Some(2), None), None);
}

#[test]
fn a_whole_file_read_numbers_every_line_and_prints_no_trailer() {
    let call = seeded("/notes.md", numbered_file(3, true).as_bytes());
    let text = text_of(read(&call, &json!({ "path": "/notes.md" })));
    assert_eq!(text, "     1|line 1\n     2|line 2\n     3|line 3\n");
}

#[test]
fn a_file_without_a_final_newline_has_the_same_lines() {
    let with = seeded("/a.md", numbered_file(3, true).as_bytes());
    let without = seeded("/a.md", numbered_file(3, false).as_bytes());
    let args = json!({ "path": "/a.md" });
    assert_eq!(text_of(read(&with, &args)), text_of(read(&without, &args)));
}

#[test]
fn a_blank_last_line_counts_as_a_line() {
    let call = seeded("/a.md", b"a\n\n");
    let text = text_of(read(&call, &json!({ "path": "/a.md" })));
    assert_eq!(text, "     1|a\n     2|\n");
}

#[test]
fn carriage_returns_are_not_part_of_a_line() {
    let call = seeded("/a.md", b"a\r\nb\r\n");
    let text = text_of(read(&call, &json!({ "path": "/a.md" })));
    assert_eq!(text, "     1|a\n     2|b\n");
}

#[test]
fn a_lone_carriage_return_inside_a_line_stays_in_the_line() {
    let call = seeded("/a.md", b"first\rNEEDLE\rthird\n");
    let text = text_of(read(&call, &json!({ "path": "/a.md" })));
    assert_eq!(text, "     1|first\rNEEDLE\rthird\n");
}

#[test]
fn a_limit_that_stops_before_the_end_names_the_next_offset() {
    let call = seeded("/ten.md", numbered_file(10, true).as_bytes());
    let text = text_of(read(
        &call,
        &json!({ "path": "/ten.md", "offset": 4, "limit": 3 }),
    ));
    assert_eq!(
        text,
        "     4|line 4\n     5|line 5\n     6|line 6\n\
         [Showing lines 4-6 of 10. Use offset=7 to continue.]"
    );
}

#[test]
fn a_read_that_reaches_the_last_line_prints_no_trailer() {
    let call = seeded("/ten.md", numbered_file(10, true).as_bytes());
    let tail = "     9|line 9\n    10|line 10\n";
    for args in [
        json!({ "path": "/ten.md", "offset": 9, "limit": 5 }),
        json!({ "path": "/ten.md", "offset": 9 }),
        json!({ "path": "/ten.md", "offset": -2 }),
        json!({ "path": "/ten.md", "offset": -2, "limit": 5 }),
        json!({ "path": "/ten.md", "offset": 9, "limit": 2 }),
    ] {
        assert_eq!(text_of(read(&call, &args)), tail, "{args}");
    }
}

#[test]
fn a_limit_that_ends_exactly_on_the_last_line_prints_no_trailer() {
    let call = seeded("/ten.md", numbered_file(10, true).as_bytes());
    let text = text_of(read(&call, &json!({ "path": "/ten.md", "limit": 10 })));
    assert!(!text.contains("[Showing"), "{text}");
}

#[test]
fn the_negative_offsets_read_the_tail_of_the_file() {
    let call = seeded("/ten.md", numbered_file(10, true).as_bytes());
    let last = text_of(read(&call, &json!({ "path": "/ten.md", "offset": -1 })));
    assert_eq!(last, "    10|line 10\n");
    let all = text_of(read(&call, &json!({ "path": "/ten.md", "offset": -100 })));
    assert_eq!(all, text_of(read(&call, &json!({ "path": "/ten.md" }))));
}

#[test]
fn an_offset_of_zero_reads_as_the_first_line() {
    let call = seeded("/ten.md", numbered_file(10, true).as_bytes());
    let zero = text_of(read(
        &call,
        &json!({ "path": "/ten.md", "offset": 0, "limit": 2 }),
    ));
    assert_eq!(
        zero,
        "     1|line 1\n     2|line 2\n[Showing lines 1-2 of 10. Use offset=3 to continue.]"
    );
}

#[test]
fn an_offset_past_the_end_fails_with_the_line_count() {
    let ten = seeded("/notes.md", numbered_file(10, true).as_bytes());
    assert_eq!(
        failure_of(read(&ten, &json!({ "path": "/notes.md", "offset": 11 }))),
        "Offset 11 is beyond the end of /notes.md, which has 10 lines."
    );
    let one = seeded("/one.md", b"only\n");
    assert_eq!(
        failure_of(read(&one, &json!({ "path": "/one.md", "offset": 2 }))),
        "Offset 2 is beyond the end of /one.md, which has 1 line."
    );
}

#[test]
fn the_line_number_widens_past_six_digits_and_the_prefix_stays_in_the_cap() {
    let call = seeded("/many.txt", "\n".repeat(1_000_000).as_bytes());
    let last = text_of(read_capped(&call, "/many.txt", Some(-1), None, BYTE_CAP));
    assert_eq!(last, "1000000|\n");
    let two = text_of(read_capped(&call, "/many.txt", Some(-2), None, BYTE_CAP));
    assert_eq!(two, "999999|\n1000000|\n");
}
