//! What Read says when it cannot read, and how it treats the files it can:
//! encodings, binary data, empty files, and the size ceiling.

use runvil_plugin::{FileType, OutputTrust};
use serde_json::json;

use super::{failure_of, new_call, read, read_capped, seed, seeded, text_of};
use crate::read::{BYTE_CAP, check_type};

#[test]
fn a_missing_path_fails_with_the_not_found_text() {
    let call = new_call();
    assert_eq!(
        failure_of(read(&call, &json!({ "path": "/nope.md" }))),
        "File not found: /nope.md"
    );
}

#[test]
fn a_relative_path_is_echoed_as_written() {
    let call = new_call();
    assert_eq!(
        failure_of(read(&call, &json!({ "path": "notes/a.md" }))),
        "File not found: notes/a.md"
    );
}

#[test]
fn a_directory_fails_with_the_directory_text() {
    let call = seeded("/dir/a.md", b"x\n");
    assert_eq!(
        failure_of(read(&call, &json!({ "path": "/dir" }))),
        "Path is a directory, not a file: /dir"
    );
}

#[test]
fn the_file_type_decision_covers_every_kind_of_node() {
    assert_eq!(check_type("/p", FileType::File), Ok(()));
    assert_eq!(check_type("/p", FileType::Symlink), Ok(()));
    assert_eq!(
        check_type("/p", FileType::Directory),
        Err("Path is a directory, not a file: /p".to_owned())
    );
    for special in [
        FileType::Fifo,
        FileType::Socket,
        FileType::CharDevice,
        FileType::BlockDevice,
    ] {
        assert_eq!(
            check_type("/p", special),
            Err("Path is neither a file nor a directory: /p".to_owned()),
            "{special:?}"
        );
    }
}

#[test]
fn a_binary_file_is_refused_by_its_content_not_its_name() {
    let call = seeded("/data.md", &[0x00, 0x01, 0x02, 0x03, 0xFF]);
    assert_eq!(
        failure_of(read(&call, &json!({ "path": "/data.md" }))),
        "File content is binary data, not text: /data.md"
    );
    let text = seeded("/data.bin", b"plain text\n");
    assert_eq!(
        text_of(read(&text, &json!({ "path": "/data.bin" }))),
        "     1|plain text\n"
    );
}

#[test]
fn a_file_one_byte_over_the_ceiling_is_refused_and_never_truncated() {
    let call = seeded("/big.txt", &vec![b'a'; 16_777_217]);
    assert_eq!(
        failure_of(read(&call, &json!({ "path": "/big.txt" }))),
        "File is too large to read: /big.txt is over the limit of 16777216 bytes."
    );
}

#[test]
fn a_file_exactly_at_the_ceiling_is_read() {
    let call = seeded("/edge.txt", &vec![b'a'; 16_777_216]);
    let text = text_of(read(&call, &json!({ "path": "/edge.txt" })));
    assert!(text.starts_with("     1|aaaa"), "the line is numbered");
    assert!(
        text.contains("[Line 1 is 16777216 bytes and is cut after 99992 bytes."),
        "{}",
        &text[text.len() - 150..]
    );
}

#[test]
fn the_ceiling_is_checked_before_the_binary_sniff() {
    let call = seeded("/big.bin", &vec![0u8; 16_777_217]);
    assert!(
        failure_of(read(&call, &json!({ "path": "/big.bin" }))).starts_with("File is too large")
    );
}

#[test]
fn an_empty_file_answers_file_is_empty_whatever_the_window() {
    let call = seeded("/empty.md", b"");
    for args in [
        json!({ "path": "/empty.md" }),
        json!({ "path": "/empty.md", "offset": 500 }),
        json!({ "path": "/empty.md", "offset": -3, "limit": 2 }),
        json!({ "path": "/empty.md", "limit": 1 }),
    ] {
        assert_eq!(text_of(read(&call, &args)), "File is empty.", "{args}");
    }
}

#[test]
fn a_file_of_only_a_byte_order_mark_is_empty() {
    let call = seeded("/mark.md", b"\xEF\xBB\xBF");
    assert_eq!(
        text_of(read(&call, &json!({ "path": "/mark.md", "offset": 9 }))),
        "File is empty."
    );
    let utf16 = seeded("/mark16.md", &[0xFF, 0xFE]);
    assert_eq!(
        text_of(read(&utf16, &json!({ "path": "/mark16.md" }))),
        "File is empty."
    );
}

#[test]
fn a_utf8_byte_order_mark_is_not_part_of_the_first_line() {
    let call = seeded("/mark.md", b"\xEF\xBB\xBFhello\n");
    assert_eq!(
        text_of(read(&call, &json!({ "path": "/mark.md" }))),
        "     1|hello\n"
    );
}

#[test]
fn a_utf16_file_is_decoded_and_the_note_names_its_byte_order() {
    let mut le = vec![0xFF, 0xFE];
    let mut be = vec![0xFE, 0xFF];
    for unit in "a\nb\n".encode_utf16() {
        le.extend_from_slice(&unit.to_le_bytes());
        be.extend_from_slice(&unit.to_be_bytes());
    }
    for (name, bytes, order) in [("/le.txt", le, "UTF-16LE"), ("/be.txt", be, "UTF-16BE")] {
        let call = seeded(name, &bytes);
        assert_eq!(
            text_of(read(&call, &json!({ "path": name }))),
            format!("     1|a\n     2|b\n[Note: the file is {order} and was decoded to text.]"),
            "{name}"
        );
    }
}

#[test]
fn a_utf16_file_without_a_mark_is_binary() {
    let call = seeded("/bare16.txt", &[b'h', 0, b'i', 0, b'\n', 0]);
    assert_eq!(
        failure_of(read(&call, &json!({ "path": "/bare16.txt" }))),
        "File content is binary data, not text: /bare16.txt"
    );
}

#[test]
fn an_invalid_byte_is_shown_as_the_replacement_character_with_a_note() {
    let call = seeded("/latin.txt", &[0x61, 0xFF, 0x62, 0x0A]);
    assert_eq!(
        text_of(read(&call, &json!({ "path": "/latin.txt" }))),
        "     1|a\u{FFFD}b\n\
         [Note: 1 invalid UTF-8 sequence in this file were shown as U+FFFD.]"
    );
}

#[test]
fn a_file_that_ends_mid_sequence_counts_one_replacement() {
    let call = seeded("/cut.txt", &[b'a', b'\n', b'b', 0xE2, 0x82]);
    assert_eq!(
        text_of(read(&call, &json!({ "path": "/cut.txt" }))),
        "     1|a\n     2|b\u{FFFD}\n\
         [Note: 1 invalid UTF-8 sequence in this file were shown as U+FFFD.]"
    );
}

#[test]
fn the_replacement_note_comes_before_the_trailer() {
    let call = seeded("/two.txt", &[0x61, 0xFF, 0x0A, 0x62, 0x0A]);
    assert_eq!(
        text_of(read(&call, &json!({ "path": "/two.txt", "limit": 1 }))),
        "     1|a\u{FFFD}\n\
         [Note: 1 invalid UTF-8 sequence in this file were shown as U+FFFD.]\n\
         [Showing lines 1-1 of 2. Use offset=2 to continue.]"
    );
}

#[test]
fn every_successful_read_is_untrusted_output() {
    let call = seeded("/a.md", b"one\ntwo\n");
    seed(&call, "/empty.md", b"");
    seed(&call, "/long.md", "x".repeat(150_000).as_bytes());
    for args in [
        json!({ "path": "/a.md" }),
        json!({ "path": "/a.md", "offset": 2 }),
        json!({ "path": "/a.md", "limit": 1 }),
        json!({ "path": "/empty.md" }),
        json!({ "path": "/long.md" }),
    ] {
        let output = read(&call, &args).expect("the read succeeds");
        assert_eq!(output.trust(), OutputTrust::Untrusted, "{args}");
    }
    let capped = read_capped(&call, "/a.md", None, None, 10).expect("a cut read succeeds");
    assert_eq!(capped.trust(), OutputTrust::Untrusted);
}

#[test]
fn the_byte_cap_is_one_hundred_thousand_bytes() {
    assert_eq!(BYTE_CAP, 100_000);
}
