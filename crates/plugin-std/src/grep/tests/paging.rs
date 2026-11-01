//! Paging through a result with `head_limit` and `offset`, and the
//! limits that stop a search before the tree is done.

use serde_json::json;

use super::fake::FakeSource;
use super::{grep, grep_limited, grep_over, seeded, text_of};
use crate::grep::found::Limits;

/// Twenty-five lines, each a match.
fn twenty_five() -> String {
    let lines: Vec<String> = (1..=25).map(|number| format!("hit {number}\n")).collect();
    lines.concat()
}

/// The page of `/f.txt` that `head_limit` and `offset` select.
fn page(call: &runvil_plugin::testing::TestCall, offset: usize) -> String {
    text_of(grep(
        call,
        &json!({ "pattern": "hit", "head_limit": 10, "offset": offset }),
    ))
}

#[test]
fn three_pages_of_ten_say_where_to_continue_and_join_into_the_whole_result() {
    let call = seeded(&[("/f.txt", &twenty_five())]);
    let first = page(&call, 0);
    let second = page(&call, 10);
    let third = page(&call, 20);
    assert!(
        first.ends_with(
            "[Showing 1-10 of at least 11 matching lines. Use offset=10 to see more, or narrow \
             the search.]"
        ),
        "{first}"
    );
    assert!(
        second.ends_with(
            "[Showing 11-20 of at least 21 matching lines. Use offset=20 to see more, or narrow \
             the search.]"
        ),
        "{second}"
    );
    assert!(
        third.ends_with("[Showing 21-25 of 25 matching lines.]"),
        "{third}"
    );
    let rows = |text: &str| -> Vec<String> {
        text.lines()
            .filter(|line| line.starts_with("  "))
            .map(str::to_owned)
            .collect()
    };
    let unpaged = text_of(grep(&call, &json!({ "pattern": "hit", "head_limit": 0 })));
    assert!(!unpaged.contains("[Showing"), "no notice without a limit");
    let joined: Vec<String> = [&first, &second, &third]
        .into_iter()
        .flat_map(|text| rows(text))
        .collect();
    assert_eq!(joined, rows(&unpaged));
}

#[test]
fn an_offset_past_the_end_says_there_are_no_more_results() {
    let call = seeded(&[("/f.txt", &twenty_five())]);
    let past = grep(&call, &json!({ "pattern": "hit", "offset": 30 })).expect("it succeeds");
    assert_eq!(past.text(), "No more results at offset=30.");
}

#[test]
fn a_write_between_two_page_calls_shows_in_the_second_because_nothing_is_cached() {
    let call = seeded(&[("/f.txt", &twenty_five())]);
    let first = page(&call, 0);
    assert!(!first.contains("changed"), "{first}");
    let changed = twenty_five().replace("hit 12", "hit changed");
    call.context()
        .access()
        .write("/f.txt", changed.as_bytes())
        .expect("the file is rewritten");
    let second = page(&call, 10);
    assert!(second.contains("  12:hit changed"), "{second}");
}

#[test]
fn a_lowered_file_limit_stops_the_search_with_a_notice_that_offers_no_offset() {
    let call = seeded(&[
        ("/a", "m\n"),
        ("/b", "m\n"),
        ("/c", "m\n"),
        ("/d", "m\n"),
        ("/e", "m\n"),
    ]);
    let limits = Limits {
        max_files: 3,
        ..Limits::default()
    };
    let output = text_of(grep_limited(&call, &json!({ "pattern": "m" }), &limits));
    assert_eq!(
        output,
        "/a\n  1:m\n\n/b\n  1:m\n\n/c\n  1:m\n\n\
         [Search stopped after considering 3 files. Narrow path, glob or type to search the \
         rest.]"
    );
}

#[test]
fn a_file_limit_that_is_not_passed_prints_no_notice() {
    let call = seeded(&[("/a", "m\n"), ("/b", "m\n"), ("/c", "m\n")]);
    let limits = Limits {
        max_files: 3,
        ..Limits::default()
    };
    let output = text_of(grep_limited(&call, &json!({ "pattern": "m" }), &limits));
    assert!(!output.contains("[Search stopped"), "{output}");
}

#[test]
fn a_lowered_byte_limit_stops_the_search_before_the_next_read() {
    let source = FakeSource::default()
        .file("/a", "match one\n")
        .file("/b", "match two\n")
        .file("/c", "match three\n");
    let limits = Limits {
        max_bytes: 15,
        ..Limits::default()
    };
    let output = text_of(grep_over(&source, &json!({ "pattern": "match" }), &limits));
    assert_eq!(
        output,
        "/a\n  1:match one\n\n/b\n  1:match two\n\n\
         [Search stopped after reading 15 bytes. Narrow path, glob or type to search the rest.]"
    );
}

#[test]
fn a_limit_stop_with_no_match_still_says_so() {
    let source = FakeSource::default().file("/a", "x\n").file("/b", "x\n");
    let limits = Limits {
        max_files: 1,
        ..Limits::default()
    };
    let output = grep_over(&source, &json!({ "pattern": "zzz" }), &limits)
        .expect("the search succeeds")
        .text()
        .to_owned();
    assert!(
        output.starts_with("No matches found. Searched 1 file."),
        "{output}"
    );
    assert!(
        output.ends_with(
            "[Search stopped after considering 1 files. Narrow path, glob or type to search the \
             rest.]"
        ),
        "{output}"
    );
}
