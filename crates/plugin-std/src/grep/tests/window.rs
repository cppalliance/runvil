//! The two cuts inside a line: the window around a match and the head of
//! a context line. No cut may split a code point, and the omitted counts
//! plus what is shown must equal the line.

use serde_json::json;

use super::{grep_limited, seeded, text_of};
use crate::grep::found::Limits;
use crate::grep::window::{Window, context_text, match_text};

/// Splits shown text into the bytes omitted before it, the window, and
/// the bytes omitted after it.
fn parts(shown: &str) -> (usize, &str, usize) {
    let (before, rest) = match shown.strip_prefix("[... ") {
        Some(after_open) => {
            let (count, rest) = after_open
                .split_once(" bytes omitted]")
                .expect("an opening marker closes");
            (count.parse().expect("the count is a number"), rest)
        }
        None => (0, shown),
    };
    match rest.rfind("[... ") {
        Some(open) if rest.ends_with(" bytes omitted]") => {
            let count = rest[open + "[... ".len()..]
                .strip_suffix(" bytes omitted]")
                .expect("the marker closes the text")
                .parse()
                .expect("the count is a number");
            (before, &rest[..open], count)
        }
        _ => (before, rest, 0),
    }
}

/// Asserts that `shown` is a well-formed cut of `line`: the window is a
/// whole-character slice of it at the offset the first count gives, and
/// the counts plus the window add up to the line.
fn assert_cut_of(line: &str, shown: &str) {
    let (before, window, after) = parts(shown);
    assert_eq!(
        before + window.len() + after,
        line.len(),
        "the counts and the window cover the line: {shown:?}"
    );
    assert_eq!(line.get(before..before + window.len()), Some(window));
}

#[test]
fn a_line_within_the_width_is_shown_whole() {
    let window = Window::default();
    let line = "y".repeat(1000);
    assert_eq!(match_text(&line, 500, &window), line);
    assert_eq!(context_text(&line, &window), line);
}

#[test]
fn a_match_past_the_first_thousand_bytes_is_still_shown_with_both_markers() {
    let line = format!("{}NEEDLE{}", "a".repeat(1001), "a".repeat(993));
    assert_eq!(line.len(), 2000);
    let shown = match_text(&line, 1001, &Window::default());
    assert!(shown.contains("NEEDLE"), "{shown}");
    assert!(shown.starts_with("[... 801 bytes omitted]"), "{shown}");
    assert!(shown.ends_with("[... 199 bytes omitted]"), "{shown}");
    let (before, window, after) = parts(&shown);
    assert_eq!((before, window.len(), after), (801, 1000, 199));
    assert_eq!(before + window.len() + after, line.len());
}

#[test]
fn a_match_near_the_start_has_only_a_trailing_marker_and_one_near_the_end_only_a_leading() {
    let line = format!("NEEDLE{}", "a".repeat(2000));
    let head = match_text(&line, 0, &Window::default());
    assert!(!head.starts_with("[... "), "{head}");
    assert_cut_of(&line, &head);
    let end = "a".repeat(2000) + "NEEDLE";
    let tail = match_text(&end, 2000, &Window::default());
    assert!(tail.starts_with("[... "), "{tail}");
    assert!(!tail.ends_with(" bytes omitted]"), "{tail}");
    assert!(tail.contains("NEEDLE"), "{tail}");
    assert_cut_of(&end, &tail);
}

#[test]
fn a_line_of_cjk_characters_is_cut_on_character_boundaries() {
    let line = "日本語".repeat(200);
    assert_eq!(line.chars().count(), 600);
    for first in [0, 700, 899, 1000, 1801] {
        let shown = match_text(&line, first, &Window::default());
        assert_cut_of(&line, &shown);
    }
}

#[test]
fn a_line_of_four_byte_emoji_with_the_match_in_the_middle_is_cut_on_character_boundaries() {
    let line = "😀".repeat(1000);
    assert_eq!(line.len(), 4000);
    for first in 2000..2004 {
        let shown = match_text(&line, first, &Window::default());
        assert_cut_of(&line, &shown);
        let (_, window, _) = parts(&shown);
        assert!(window.chars().all(|c| c == '😀'), "whole characters only");
    }
}

#[test]
fn a_context_line_over_the_width_keeps_its_head_and_counts_the_rest() {
    let line = "日".repeat(600);
    let shown = context_text(&line, &Window::default());
    let (before, window, after) = parts(&shown);
    assert_eq!(before, 0);
    assert!(window.len() <= 1000, "{} bytes", window.len());
    assert_eq!(window.len(), 999, "333 whole characters");
    assert_eq!(after, 1800 - 999);
    assert_cut_of(&line, &shown);
}

#[test]
fn sweeping_the_window_start_over_every_byte_of_a_three_byte_character_never_splits_it() {
    let line = format!("{}日{}", "x".repeat(1500), "y".repeat(1500));
    for first in 1690..1710 {
        let shown = match_text(&line, first, &Window::default());
        assert_cut_of(&line, &shown);
    }
}

#[test]
fn sweeping_the_width_and_lead_over_multibyte_text_keeps_every_cut_whole() {
    let lines = [
        "日本語のテキスト".repeat(5),
        "😀🎉".repeat(12),
        format!("é{}ß{}", "a".repeat(9), "日".repeat(9)),
    ];
    for line in &lines {
        for width in 1..30 {
            for lead in [0, 1, 2, 3, 5, 8] {
                let window = Window { width, lead };
                for first in 0..line.len() {
                    assert_cut_of(line, &match_text(line, first, &window));
                }
                assert_cut_of(line, &context_text(line, &window));
            }
        }
    }
}

#[test]
fn sweeping_the_body_cap_and_window_over_emoji_and_cjk_text_keeps_the_body_whole_lines() {
    let text = format!(
        "{}\n{}\n{}\n",
        "日本語".repeat(12),
        "😀 needle 🎉".repeat(4),
        "needle é"
    );
    let call = seeded(&[("/a.txt", &text)]);
    for width in [1, 2, 3, 4, 7, 10, 25] {
        for lead in [0, 1, 4] {
            for cap in (10..260).step_by(13) {
                let limits = Limits {
                    body_cap: cap,
                    window: Window { width, lead },
                    ..Limits::default()
                };
                let output = text_of(grep_limited(
                    &call,
                    &json!({ "pattern": "needle", "-C": 1 }),
                    &limits,
                ));
                let body = output.split("\n\n").next().expect("a body");
                assert!(
                    body.len() <= cap || body.starts_with('['),
                    "cap {cap} width {width}: {output}"
                );
                for row in body.lines() {
                    let numbered = row
                        .strip_prefix("  ")
                        .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()));
                    assert!(
                        row.starts_with('/') || row == "  --" || numbered || row.starts_with('['),
                        "cap {cap} width {width}: {row:?} is not a whole row in {output}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_long_matching_line_in_a_search_is_shown_as_a_window_with_its_markers() {
    let line = format!("{}NEEDLE{}", "a".repeat(1500), "b".repeat(1500));
    let call = seeded(&[("/long.txt", &format!("{line}\n"))]);
    let output = text_of(super::grep(&call, &json!({ "pattern": "NEEDLE" })));
    let row = output.lines().nth(1).expect("a match row");
    let shown = row.strip_prefix("  1:").expect("the row is numbered");
    assert!(shown.contains("NEEDLE"), "{shown}");
    assert_cut_of(&line, shown);
    let (before, _, _) = parts(shown);
    assert_eq!(before, 1300, "the window starts 200 bytes before the match");
}
