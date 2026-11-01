//! Unit tests for the text module: the decode ladder, the binary sniff,
//! the line splitter, and the boundary clip.

use super::{BODY_CAP, MAX_FILE_BYTES, clip_to_boundary, decode, sniff_binary, split_lines};

/// The UTF-16 little-endian bytes of `text`, behind the `FF FE` mark.
fn utf16_le(text: &str) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

/// The UTF-16 big-endian bytes of `text`, behind the `FE FF` mark.
fn utf16_be(text: &str) -> Vec<u8> {
    let mut bytes = vec![0xFE, 0xFF];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_be_bytes());
    }
    bytes
}

#[test]
fn the_file_ceiling_is_sixteen_mebibytes() {
    assert_eq!(MAX_FILE_BYTES, 16 * 1024 * 1024);
    assert_eq!(MAX_FILE_BYTES, 16_777_216);
}

#[test]
fn the_body_cap_the_listing_and_search_tools_share_is_fifteen_thousand_bytes() {
    assert_eq!(BODY_CAP, 15_000);
}

#[test]
fn plain_utf8_decodes_unchanged_with_no_note() {
    let decoded = decode("hello\nwörld €\n".as_bytes());
    assert_eq!(decoded.text, "hello\nwörld €\n");
    assert_eq!(decoded.utf16, None);
    assert_eq!(decoded.invalid, 0);
}

#[test]
fn a_leading_utf8_mark_is_dropped_and_not_reported() {
    let decoded = decode(b"\xEF\xBB\xBFhi\n");
    assert_eq!(decoded.text, "hi\n");
    assert_eq!(decoded.utf16, None);
    assert_eq!(decoded.invalid, 0);
}

#[test]
fn a_mark_that_is_not_leading_stays_in_the_text() {
    let decoded = decode(b"a\xEF\xBB\xBFb");
    assert_eq!(decoded.text, "a\u{FEFF}b");
}

#[test]
fn utf16_little_endian_with_a_mark_is_decoded_and_named() {
    let decoded = decode(&utf16_le("hi\nthere\n"));
    assert_eq!(decoded.text, "hi\nthere\n");
    assert_eq!(decoded.utf16, Some("UTF-16LE"));
    assert_eq!(decoded.invalid, 0);
}

#[test]
fn utf16_big_endian_with_a_mark_is_decoded_and_named() {
    let decoded = decode(&utf16_be("hi\nthere\n"));
    assert_eq!(decoded.text, "hi\nthere\n");
    assert_eq!(decoded.utf16, Some("UTF-16BE"));
    assert_eq!(decoded.invalid, 0);
}

#[test]
fn utf16_decodes_a_surrogate_pair_and_replaces_a_lone_surrogate() {
    let decoded = decode(&utf16_le("a\u{1F600}b"));
    assert_eq!(decoded.text, "a\u{1F600}b");
    let lone = [0xFF, 0xFE, 0x61, 0x00, 0x00, 0xD8, 0x62, 0x00];
    let decoded = decode(&lone);
    assert_eq!(decoded.text, "a\u{FFFD}b");
    assert_eq!(decoded.utf16, Some("UTF-16LE"));
    assert_eq!(decoded.invalid, 0, "the count is for UTF-8 sequences only");
}

#[test]
fn utf16_drops_one_trailing_byte_that_does_not_make_a_unit() {
    let decoded = decode(&[0xFF, 0xFE, 0x61, 0x00, 0x62]);
    assert_eq!(decoded.text, "a");
    assert_eq!(decoded.utf16, Some("UTF-16LE"));
}

#[test]
fn a_file_that_is_only_a_mark_decodes_to_nothing() {
    assert_eq!(decode(b"\xEF\xBB\xBF").text, "");
    assert_eq!(decode(&[0xFF, 0xFE]).text, "");
    assert_eq!(decode(&[0xFE, 0xFF]).text, "");
    assert_eq!(decode(b"").text, "");
}

#[test]
fn an_invalid_byte_becomes_one_replacement_character_and_one_count() {
    let decoded = decode(&[0x61, 0xFF, 0x62, 0x0A]);
    assert_eq!(decoded.text, "a\u{FFFD}b\n");
    assert_eq!(decoded.utf16, None);
    assert_eq!(decoded.invalid, 1);
}

#[test]
fn a_file_that_ends_mid_sequence_counts_one() {
    let decoded = decode(&[0x61, 0xE2, 0x82]);
    assert_eq!(decoded.text, "a\u{FFFD}");
    assert_eq!(decoded.invalid, 1);
}

#[test]
fn separate_invalid_chunks_each_count_once() {
    let decoded = decode(&[0xFF, 0x61, 0xC0, 0xC1, 0x62, 0xE2, 0x82]);
    assert_eq!(decoded.invalid, 4, "FF, C0, C1, and the cut E2 82");
    assert_eq!(decoded.text.matches('\u{FFFD}').count(), 4);
}

#[test]
fn bytes_after_a_utf8_mark_are_counted_too() {
    let decoded = decode(&[0xEF, 0xBB, 0xBF, 0x61, 0xFF]);
    assert_eq!(decoded.text, "a\u{FFFD}");
    assert_eq!(decoded.invalid, 1);
}

#[test]
fn ordinary_text_is_not_binary() {
    assert!(!sniff_binary(b"fn main() {\n\tprintln!(\"hi\");\r\n}\n"));
    assert!(!sniff_binary("héllo wörld €\n".as_bytes()));
    assert!(!sniff_binary(b""));
}

#[test]
fn a_nul_in_the_first_8192_bytes_is_binary() {
    assert!(sniff_binary(b"\0abc"));
    let mut bytes = vec![b'a'; 9_000];
    bytes[8_191] = 0;
    assert!(sniff_binary(&bytes), "index 8191 is inside the sample");
    let mut bytes = vec![b'a'; 9_000];
    bytes[8_192] = 0;
    assert!(!sniff_binary(&bytes), "index 8192 is outside the sample");
}

#[test]
fn the_sniff_ignores_everything_past_the_sample() {
    let mut bytes = vec![b'a'; 8_192];
    bytes.extend(vec![0x01; 8_192]);
    assert!(!sniff_binary(&bytes));
}

#[test]
fn five_percent_control_bytes_is_binary_and_less_is_not() {
    let sample = |controls: usize| {
        let mut bytes = vec![b'a'; 1_000 - controls];
        bytes.extend(vec![0x01; controls]);
        bytes
    };
    assert!(sniff_binary(&sample(50)), "50 of 1000 is 5%");
    assert!(!sniff_binary(&sample(49)), "49 of 1000 is 4.9%");
    assert!(sniff_binary(&sample(1_000)));
}

#[test]
fn a_delete_byte_counts_as_a_control_byte() {
    let mut bytes = vec![b'a'; 950];
    bytes.extend(vec![0x7F; 50]);
    assert!(sniff_binary(&bytes));
}

#[test]
fn tab_line_feed_and_carriage_return_never_count() {
    let bytes: Vec<u8> = (0..1_000).map(|n| b"\t\n\r"[n % 3]).collect();
    assert!(!sniff_binary(&bytes));
}

#[test]
fn a_file_with_a_utf16_mark_skips_the_sniff_and_one_without_is_binary() {
    assert!(!sniff_binary(&utf16_le("hello\n")));
    assert!(!sniff_binary(&utf16_be("hello\n")));
    assert!(
        sniff_binary(&[b'h', 0, b'i', 0]),
        "UTF-16 without a mark holds NUL bytes"
    );
}

#[test]
fn a_final_newline_does_not_make_an_extra_line() {
    assert_eq!(split_lines("a\nb"), ["a", "b"]);
    assert_eq!(split_lines("a\nb\n"), ["a", "b"]);
}

#[test]
fn an_empty_text_has_no_lines_and_a_lone_newline_has_one() {
    assert!(split_lines("").is_empty());
    assert_eq!(split_lines("\n"), [""]);
    assert_eq!(split_lines("a\n\nb\n"), ["a", "", "b"]);
}

#[test]
fn one_trailing_carriage_return_is_stripped_from_each_line() {
    assert_eq!(split_lines("a\r\nb\r\n"), ["a", "b"]);
    assert_eq!(split_lines("a\r\nb\r"), ["a", "b"]);
    assert_eq!(split_lines("\r\n"), [""]);
    assert_eq!(split_lines("a\r\r\nb"), ["a\r", "b"]);
}

#[test]
fn a_lone_carriage_return_inside_a_line_is_kept() {
    assert_eq!(
        split_lines("first\rNEEDLE\rthird"),
        ["first\rNEEDLE\rthird"]
    );
}

/// The text of one 1-byte, one 2-byte, one 3-byte, and one 4-byte
/// character, twice.
const MIXED: &str = "aé€😀aé€😀";

/// The smallest character boundary at or after `at`, within `text`.
fn boundary_up(text: &str, at: usize) -> usize {
    (at.min(text.len())..=text.len())
        .find(|&n| text.is_char_boundary(n))
        .expect("the end of a string is a boundary")
}

/// The largest character boundary at or before `at`, within `text`.
fn boundary_down(text: &str, at: usize) -> usize {
    (0..=at.min(text.len()))
        .rev()
        .find(|&n| text.is_char_boundary(n))
        .expect("the start of a string is a boundary")
}

#[test]
fn clip_gives_the_exact_range_when_both_ends_are_boundaries() {
    assert_eq!(clip_to_boundary("hello", 1, 4), "ell");
    assert_eq!(clip_to_boundary("hello", 0, 5), "hello");
    assert_eq!(clip_to_boundary("hello", 2, 2), "");
}

#[test]
fn clip_rounds_the_start_up_and_the_end_down_over_every_range() {
    for text in ["a", "é", "€", "😀", "aé€😀", MIXED] {
        let len = text.len();
        for start in 0..=len + 2 {
            for end in 0..=len + 2 {
                let clipped = clip_to_boundary(text, start, end);
                let low = boundary_up(text, start);
                let high = boundary_down(text, end);
                let expected = if low >= high { "" } else { &text[low..high] };
                assert_eq!(clipped, expected, "{text:?} from {start} to {end}");
                if !clipped.is_empty() {
                    let at = clipped.as_ptr().addr() - text.as_ptr().addr();
                    assert!(
                        at >= start && at + clipped.len() <= end.min(len),
                        "{text:?} from {start} to {end} gave bytes {at}..{}",
                        at + clipped.len()
                    );
                }
            }
        }
    }
}

#[test]
fn clip_clamps_both_ends_to_the_length_and_a_reversed_range_is_empty() {
    assert_eq!(clip_to_boundary("abc", 1, 100), "bc");
    assert_eq!(clip_to_boundary("abc", 100, 200), "");
    assert_eq!(clip_to_boundary("abc", 3, 1), "");
    assert_eq!(clip_to_boundary("", 0, 5), "");
}

#[test]
fn a_prefix_cut_never_splits_a_character() {
    for width in 0..=MIXED.len() {
        let kept = clip_to_boundary(MIXED, 0, width);
        assert!(MIXED.starts_with(kept), "{width}");
        assert!(kept.len() <= width, "{width}");
        assert!(width - kept.len() < 4, "the cut stops within one character");
    }
    assert_eq!(clip_to_boundary("€€", 0, 5), "€");
    assert_eq!(clip_to_boundary("€€", 0, 2), "");
}
