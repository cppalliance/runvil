//! How Grep reads a file's bytes: through the same `text` module Read
//! uses, so a Grep line number is the number Read prints for that line.

use runvil_plugin::OutputTrust;
use serde_json::json;

use super::fake::{FakeSource, Node};
use super::{grep_over, text_of};
use crate::grep::found::Limits;

/// The tree with `bytes` as the file `/a.txt`.
fn one(bytes: &[u8]) -> FakeSource {
    FakeSource::default().with("/a.txt", Node::File(bytes.to_vec()))
}

/// The text of searching `source` for `pattern`.
fn search(source: &FakeSource, pattern: &str) -> String {
    text_of(grep_over(
        source,
        &json!({ "pattern": pattern }),
        &Limits::default(),
    ))
}

/// `text` as UTF-16 little-endian code units, with a byte order mark when
/// `mark` is set.
fn utf16le(text: &str, mark: bool) -> Vec<u8> {
    let mut bytes = if mark { vec![0xFF, 0xFE] } else { Vec::new() };
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

#[test]
fn invalid_utf8_matches_with_the_replacement_character_and_a_note() {
    let output = search(&one(b"caf\xE9 NEEDLE\n"), "NEEDLE");
    assert_eq!(
        output,
        "/a.txt\n  1:caf\u{FFFD} NEEDLE\n\n\
         [Note: invalid UTF-8 in 1 file was shown as U+FFFD: /a.txt]"
    );
}

#[test]
fn a_byte_order_mark_is_dropped_so_a_caret_matches_the_first_line() {
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(b"NEEDLE first\nsecond\n");
    let output = search(&one(&bytes), "^NEEDLE");
    assert_eq!(output, "/a.txt\n  1:NEEDLE first");
}

#[test]
fn a_carriage_return_before_a_line_feed_is_not_part_of_the_line() {
    let output = search(&one(b"a\r\nNEEDLE\r\n"), "NEEDLE");
    assert_eq!(output, "/a.txt\n  2:NEEDLE");
    let anchored = search(&one(b"a\r\nNEEDLE\r\n"), "NEEDLE$");
    assert_eq!(anchored, "/a.txt\n  2:NEEDLE");
}

#[test]
fn a_lone_carriage_return_does_not_start_a_line() {
    let output = search(&one(b"first\rNEEDLE\rthird"), "NEEDLE");
    assert_eq!(output, "/a.txt\n  1:first\rNEEDLE\rthird");
}

#[test]
fn utf16_with_a_mark_is_decoded_and_numbered_by_its_lines() {
    let output = search(&one(&utf16le("a\nNEEDLE\n", true)), "NEEDLE");
    assert_eq!(output, "/a.txt\n  2:NEEDLE");
}

#[test]
fn utf16_without_a_mark_is_one_binary_skip() {
    let output = grep_over(
        &one(&utf16le("NEEDLE\n", false)),
        &json!({ "pattern": "NEEDLE" }),
        &Limits::default(),
    )
    .expect("the search succeeds");
    assert!(
        output
            .text()
            .contains("[Skipped 1 entry: 1 binary file. First: /a.txt]"),
        "{}",
        output.text()
    );
}

#[test]
fn five_control_bytes_in_a_hundred_make_a_file_binary_and_four_do_not() {
    let body = |controls: usize| {
        let mut bytes = vec![0x01; controls];
        bytes.extend_from_slice(b"NEEDLE");
        bytes.resize(100, b'a');
        bytes
    };
    let binary = grep_over(
        &one(&body(5)),
        &json!({ "pattern": "NEEDLE" }),
        &Limits::default(),
    )
    .expect("the search succeeds");
    assert!(binary.text().contains("1 binary file"), "{}", binary.text());
    let text = grep_over(
        &one(&body(4)),
        &json!({ "pattern": "NEEDLE", "output_mode": "files_with_matches" }),
        &Limits::default(),
    )
    .expect("the search succeeds");
    assert_eq!(text.text(), "/a.txt");
}

#[test]
fn a_precomposed_character_and_a_combining_pair_each_match_only_their_own_file() {
    let source = FakeSource::default()
        .file("/one.txt", "caf\u{e9}\n")
        .file("/two.txt", "cafe\u{301}\n");
    let precomposed = text_of(grep_over(
        &source,
        &json!({ "pattern": "caf\u{e9}", "output_mode": "files_with_matches" }),
        &Limits::default(),
    ));
    assert_eq!(precomposed, "/one.txt");
    let combining = text_of(grep_over(
        &source,
        &json!({ "pattern": "cafe\u{301}", "output_mode": "files_with_matches" }),
        &Limits::default(),
    ));
    assert_eq!(combining, "/two.txt");
}

#[test]
fn two_replaced_files_are_named_in_one_note() {
    let source = FakeSource::default()
        .with("/a.txt", Node::File(b"x \xFF\n".to_vec()))
        .with("/b.txt", Node::File(b"x \xFE\n".to_vec()));
    let output = text_of(grep_over(
        &source,
        &json!({ "pattern": "x", "output_mode": "files_with_matches" }),
        &Limits::default(),
    ));
    assert_eq!(
        output,
        "/a.txt\n/b.txt\n\n\
         [Note: invalid UTF-8 in 2 files was shown as U+FFFD: /a.txt, /b.txt]"
    );
}

#[test]
fn a_note_names_the_first_five_files_and_counts_the_rest() {
    let mut source = FakeSource::default();
    for index in 0..7 {
        source = source.with(&format!("/f{index}.txt"), Node::File(b"x \xFF\n".to_vec()));
    }
    let output = text_of(grep_over(
        &source,
        &json!({ "pattern": "x", "output_mode": "count" }),
        &Limits::default(),
    ));
    assert!(
        output.ends_with(
            "[Note: invalid UTF-8 in 7 files was shown as U+FFFD: /f0.txt, /f1.txt, /f2.txt, \
             /f3.txt, /f4.txt, and 2 more]"
        ),
        "{output}"
    );
}

#[test]
fn a_replacement_note_on_a_zero_match_result_makes_it_untrusted() {
    let output = grep_over(
        &one(b"caf\xE9\n"),
        &json!({ "pattern": "absent" }),
        &Limits::default(),
    )
    .expect("the search succeeds");
    assert_eq!(output.trust(), OutputTrust::Untrusted);
    assert!(
        output.text().contains("[Note: invalid UTF-8 in 1 file"),
        "{}",
        output.text()
    );
}

#[test]
fn an_empty_file_has_no_lines_to_match() {
    let output = grep_over(&one(b""), &json!({ "pattern": "^" }), &Limits::default())
        .expect("the search succeeds");
    assert!(
        output
            .text()
            .starts_with("No matches found. Searched 1 file."),
        "{}",
        output.text()
    );
}
