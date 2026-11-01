//! The render: the numbered lines, the byte cap, the trailer, the cut of a
//! first line that alone passes the cap, and the notes.

use super::{read_capped, seeded, text_of};
use crate::read::BYTE_CAP;
use crate::read::render::{notes, render};
use crate::read::window::Window;

/// The three lines most render tests print. Each costs 11 bytes: a
/// seven-byte prefix, three bytes of text, and a newline.
const LINES: [&str; 3] = ["aaa", "bbb", "ccc"];

/// The window over every line of [`LINES`].
const ALL: Window = Window { first: 1, last: 3 };

#[test]
fn every_line_prints_as_a_six_wide_number_a_pipe_and_the_text() {
    let text = render(&LINES, &ALL, &[], BYTE_CAP);
    assert_eq!(text, "     1|aaa\n     2|bbb\n     3|ccc\n");
}

#[test]
fn a_window_in_the_middle_keeps_the_real_line_numbers() {
    let text = render(&LINES, &Window { first: 2, last: 3 }, &[], BYTE_CAP);
    assert_eq!(text, "     2|bbb\n     3|ccc\n");
}

#[test]
fn a_window_that_stops_early_ends_with_the_limit_trailer() {
    let text = render(&LINES, &Window { first: 2, last: 2 }, &[], BYTE_CAP);
    assert_eq!(
        text,
        "     2|bbb\n[Showing lines 2-2 of 3. Use offset=3 to continue.]"
    );
}

#[test]
fn lines_that_exactly_fill_the_cap_all_print_and_no_trailer_follows() {
    let text = render(&LINES, &ALL, &[], 33);
    assert_eq!(text, "     1|aaa\n     2|bbb\n     3|ccc\n");
}

#[test]
fn the_line_that_would_pass_the_cap_is_left_out_and_the_trailer_names_the_cap() {
    let text = render(&LINES, &ALL, &[], 32);
    assert_eq!(
        text,
        "     1|aaa\n     2|bbb\n\
         [Showing lines 1-2 of 3 (32-byte limit). Use offset=3 to continue.]"
    );
}

#[test]
fn the_cap_counts_each_prefix_the_text_and_the_newline() {
    for (cap, printed) in [(11, 1), (21, 1), (22, 2), (32, 2), (33, 3)] {
        let text = render(&LINES, &ALL, &[], cap);
        let shown = text.lines().filter(|line| line.contains('|')).count();
        assert_eq!(shown, printed, "cap {cap}: {text}");
    }
}

#[test]
fn the_cap_trailer_wins_over_the_limit_when_the_cap_stops_first() {
    let text = render(&LINES, &Window { first: 1, last: 2 }, &[], 11);
    assert_eq!(
        text,
        "     1|aaa\n[Showing lines 1-1 of 3 (11-byte limit). Use offset=2 to continue.]"
    );
}

#[test]
fn a_first_line_that_alone_passes_the_cap_is_cut_and_the_trailer_says_so() {
    let text = render(&LINES, &ALL, &[], 10);
    assert_eq!(
        text,
        "     1|aa\n[Line 1 is 3 bytes and is cut after 2 bytes. \
         No parameter reads the rest of a line. Use offset=2 to continue.]"
    );
}

#[test]
fn a_cut_first_line_on_the_last_line_of_the_file_offers_no_next_offset() {
    let text = render(&LINES, &Window { first: 3, last: 3 }, &[], 10);
    assert_eq!(
        text,
        "     3|cc\n[Line 3 is 3 bytes and is cut after 2 bytes. \
         No parameter reads the rest of a line.]"
    );
}

#[test]
fn a_cap_below_the_prefix_keeps_no_text_and_does_not_panic() {
    let text = render(&LINES, &ALL, &[], 7);
    assert_eq!(
        text,
        "     1|\n[Line 1 is 3 bytes and is cut after 0 bytes. \
         No parameter reads the rest of a line. Use offset=2 to continue.]"
    );
}

#[test]
fn notes_come_after_the_lines_and_before_the_trailer_each_on_its_own_line() {
    let note_lines = ["[Note: one]".to_owned(), "[Note: two]".to_owned()];
    let with_trailer = render(&LINES, &Window { first: 1, last: 1 }, &note_lines, BYTE_CAP);
    assert_eq!(
        with_trailer,
        "     1|aaa\n[Note: one]\n[Note: two]\n\
         [Showing lines 1-1 of 3. Use offset=2 to continue.]"
    );
    let without = render(&LINES, &ALL, &note_lines, BYTE_CAP);
    assert_eq!(
        without,
        "     1|aaa\n     2|bbb\n     3|ccc\n[Note: one]\n[Note: two]"
    );
}

#[test]
fn a_utf16_file_adds_a_note_naming_its_byte_order() {
    assert_eq!(
        notes(Some("UTF-16LE"), 0),
        ["[Note: the file is UTF-16LE and was decoded to text.]"]
    );
    assert_eq!(
        notes(Some("UTF-16BE"), 0),
        ["[Note: the file is UTF-16BE and was decoded to text.]"]
    );
}

#[test]
fn replaced_bytes_add_a_note_that_counts_the_sequences() {
    assert_eq!(
        notes(None, 1),
        ["[Note: 1 invalid UTF-8 sequence in this file were shown as U+FFFD.]"]
    );
    assert_eq!(
        notes(None, 3),
        ["[Note: 3 invalid UTF-8 sequences in this file were shown as U+FFFD.]"]
    );
}

#[test]
fn the_utf16_note_comes_before_the_replacement_note_and_clean_text_has_none() {
    assert_eq!(
        notes(Some("UTF-16LE"), 2),
        [
            "[Note: the file is UTF-16LE and was decoded to text.]",
            "[Note: 2 invalid UTF-8 sequences in this file were shown as U+FFFD.]"
        ]
    );
    assert!(notes(None, 0).is_empty());
}

#[test]
fn five_thousand_short_lines_stop_at_2083_lines_under_the_default_cap() {
    let file = format!("{}\n", "x".repeat(40)).repeat(5_000);
    let call = seeded("/many.txt", file.as_bytes());
    let text = text_of(read_capped(&call, "/many.txt", None, None, BYTE_CAP));
    let trailer =
        "[Showing lines 1-2083 of 5000 (100000-byte limit). Use offset=2084 to continue.]";
    assert!(
        text.ends_with(&format!("\n{trailer}")),
        "the trailer ends the read"
    );
    let numbered = text
        .lines()
        .filter(|line| line.as_bytes().get(6) == Some(&b'|'));
    assert_eq!(numbered.count(), 2_083);
    assert_eq!(
        text.len() - trailer.len(),
        2_083 * 48,
        "each numbered line is 7 + 40 + 1 bytes and the trailer is not counted"
    );
    assert!(text.len() <= 100_120, "{}", text.len());
}

/// What Read answers for a file whose first line is `line`, followed by
/// a second line when `later` is set, split into the line it printed
/// after its prefix and the trailer.
fn first_line_cut(line: &str, later: bool) -> (String, String) {
    let mut file = line.to_owned();
    file.push('\n');
    if later {
        file.push_str("next\n");
    }
    let call = seeded("/long.txt", file.as_bytes());
    let text = text_of(read_capped(&call, "/long.txt", None, None, BYTE_CAP));
    let (shown, trailer) = text.split_once('\n').expect("a cut read has a trailer");
    let kept = shown
        .strip_prefix("     1|")
        .expect("the first line is numbered");
    (kept.to_owned(), trailer.to_owned())
}

#[test]
fn a_150000_byte_ascii_line_keeps_99992_bytes() {
    let line = "x".repeat(150_000);
    let (kept, trailer) = first_line_cut(&line, false);
    assert_eq!(kept.len(), 99_992);
    assert!(line.starts_with(&kept));
    assert_eq!(
        trailer,
        "[Line 1 is 150000 bytes and is cut after 99992 bytes. \
         No parameter reads the rest of a line.]"
    );
    let (_, trailer) = first_line_cut(&line, true);
    assert_eq!(
        trailer,
        "[Line 1 is 150000 bytes and is cut after 99992 bytes. \
         No parameter reads the rest of a line. Use offset=2 to continue.]"
    );
}

#[test]
fn the_cut_numbered_line_fills_the_cap_exactly_for_ascii() {
    let call = seeded("/long.txt", "x".repeat(150_000).as_bytes());
    let text = text_of(read_capped(&call, "/long.txt", None, None, BYTE_CAP));
    let first = text.split_once('\n').expect("one numbered line").0;
    assert_eq!(first.len() + 1, 100_000);
}

#[test]
fn fifty_thousand_cjk_characters_keep_99990_bytes() {
    let line = "\u{4E2D}".repeat(50_000);
    let (kept, trailer) = first_line_cut(&line, false);
    assert_eq!(kept.len(), 99_990);
    assert_eq!(kept.chars().count(), 33_330);
    assert!(
        trailer.contains("is 150000 bytes and is cut after 99990 bytes."),
        "{trailer}"
    );
}

#[test]
fn a_line_of_two_byte_characters_keeps_99992_bytes() {
    let line = "\u{E9}".repeat(75_000);
    let (kept, trailer) = first_line_cut(&line, false);
    assert_eq!(kept.len(), 99_992);
    assert!(line.starts_with(&kept));
    assert!(trailer.contains("cut after 99992 bytes."), "{trailer}");
}

#[test]
fn a_line_of_four_byte_emoji_keeps_24998_of_them() {
    let line = "\u{1F600}".repeat(37_500);
    let (kept, trailer) = first_line_cut(&line, false);
    assert_eq!(kept.len(), 99_992);
    assert_eq!(kept.chars().count(), 24_998);
    assert!(trailer.contains("cut after 99992 bytes."), "{trailer}");
}

#[test]
fn a_cut_at_a_later_line_names_that_line() {
    let file = format!("short\n{}\ntail\n", "x".repeat(150_000));
    let call = seeded("/long.txt", file.as_bytes());
    let text = text_of(read_capped(&call, "/long.txt", Some(2), None, BYTE_CAP));
    let trailer = text.split_once('\n').expect("a cut read has a trailer").1;
    assert_eq!(
        trailer,
        "[Line 2 is 150000 bytes and is cut after 99992 bytes. \
         No parameter reads the rest of a line. Use offset=3 to continue.]"
    );
}

#[test]
fn a_line_that_only_just_fits_is_not_cut() {
    let line = "x".repeat(BYTE_CAP - 8);
    let call = seeded("/fit.txt", format!("{line}\n").as_bytes());
    let text = text_of(read_capped(&call, "/fit.txt", None, None, BYTE_CAP));
    assert_eq!(text, format!("     1|{line}\n"));
}

#[test]
fn a_cap_sweep_never_splits_a_character_and_the_notice_counts_the_kept_bytes() {
    let line = "a\u{E9}\u{20AC}\u{1F600}".repeat(30);
    let call = seeded("/mixed.txt", format!("{line}\n").as_bytes());
    for cap in 12..=60 {
        let text = text_of(read_capped(&call, "/mixed.txt", None, None, cap));
        let (shown, trailer) = text.split_once('\n').expect("a cut read has a trailer");
        let kept = shown
            .strip_prefix("     1|")
            .expect("the first line is numbered");
        let allowance = cap - 8;
        assert!(line.starts_with(kept), "cap {cap}: a prefix of the line");
        assert!(
            kept.len() <= allowance,
            "cap {cap}: {} bytes kept",
            kept.len()
        );
        assert!(
            allowance - kept.len() < 4,
            "cap {cap}: the cut stops within one character of the allowance"
        );
        assert_eq!(
            trailer,
            format!(
                "[Line 1 is {} bytes and is cut after {} bytes. \
                 No parameter reads the rest of a line.]",
                line.len(),
                kept.len()
            ),
            "cap {cap}"
        );
    }
}
