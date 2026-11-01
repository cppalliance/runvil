//! Paging: following each `Use offset=N` trailer until the read ends must
//! rebuild the file exactly, whatever the cap and the limit.

use super::{numbered_file, read_capped, seeded, text_of};
use crate::read::BYTE_CAP;

/// Reads `/file.txt` page by page under `cap` and `limit`, following each
/// trailer's offset, and returns the text of every line it printed.
fn rebuild(file: &str, total: usize, cap: usize, limit: Option<usize>) -> Vec<String> {
    let call = seeded("/file.txt", file.as_bytes());
    let mut rebuilt: Vec<String> = Vec::new();
    let mut offset: Option<i64> = None;
    let mut pages = 0usize;
    loop {
        pages += 1;
        assert!(pages <= total + 1, "paging does not advance");
        let text = text_of(read_capped(&call, "/file.txt", offset, limit, cap));
        let mut shown: Vec<&str> = text.split('\n').collect();
        if shown.last() == Some(&"") {
            shown.pop();
        }
        let trailer = shown
            .last()
            .copied()
            .filter(|line| line.starts_with("[Showing lines "));
        if trailer.is_some() {
            shown.pop();
        }
        assert!(!shown.is_empty(), "a page is never empty: {text}");
        for line in shown {
            let number = rebuilt.len() + 1;
            let (prefix, content) = line.split_at(7);
            assert_eq!(prefix, format!("{number:>6}|"), "page {pages}");
            rebuilt.push(content.to_owned());
        }
        let Some(trailer) = trailer else {
            break;
        };
        let next: i64 = trailer
            .split("offset=")
            .nth(1)
            .and_then(|rest| rest.split(' ').next())
            .and_then(|digits| digits.parse().ok())
            .unwrap_or_else(|| panic!("the trailer names an offset: {trailer}"));
        assert_eq!(
            usize::try_from(next).expect("the offset is positive"),
            rebuilt.len() + 1,
            "the trailer continues where the page stopped"
        );
        offset = Some(next);
    }
    assert_eq!(rebuilt.len(), total, "the pages cover the file");
    rebuilt
}

#[test]
fn following_the_trailers_rebuilds_the_file_under_every_cap_and_limit() {
    for total in [1, 2, 5, 37, 2_000, 2_001] {
        for final_newline in [true, false] {
            let file = numbered_file(total, final_newline);
            let expected: Vec<String> = (1..=total).map(|n| format!("line {n}")).collect();
            for cap in [BYTE_CAP, 300, 90] {
                for limit in [None, Some(50), Some(7)] {
                    let rebuilt = rebuild(&file, total, cap, limit);
                    assert_eq!(
                        rebuilt, expected,
                        "{total} lines, final newline {final_newline}, cap {cap}, limit {limit:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_small_cap_makes_more_pages_than_a_large_one() {
    let file = numbered_file(37, true);
    let pages = |cap: usize| {
        let call = seeded("/file.txt", file.as_bytes());
        let mut count = 0;
        let mut offset: Option<i64> = None;
        loop {
            count += 1;
            let text = text_of(read_capped(&call, "/file.txt", offset, None, cap));
            let Some(rest) = text.rsplit_once("offset=") else {
                return count;
            };
            let digits = rest.1.trim_end_matches(" to continue.]");
            offset = Some(digits.parse().expect("the offset is a number"));
        }
    };
    assert_eq!(pages(BYTE_CAP), 1);
    assert!(pages(90) > pages(300));
    assert!(pages(300) > 1);
}
