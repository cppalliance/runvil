//! The text of a result: the worked example, context, gaps, the count
//! mode, and the byte cap.

use serde_json::json;

use super::{grep, grep_limited, seeded, text_of};
use crate::grep::found::Limits;

/// A file of `lines` lines where line `n` of `at` holds `hit n` and every
/// other line holds `.`.
fn file_with_hits(lines: usize, at: &[usize]) -> String {
    (1..=lines)
        .map(|number| {
            if at.contains(&number) {
                format!("hit {number}\n")
            } else {
                ".\n".to_owned()
            }
        })
        .collect()
}

#[test]
fn the_worked_example_prints_paths_numbered_lines_a_gap_and_no_trailing_newline() {
    let call = seeded(&[
        (
            "/src/a.ts",
            "// one\n// two\nconst alpha = 1\nlet beta = 2\n// five\n// six\n// seven\n// eight\n\
             const gamma = 3\nexport default gamma\n",
        ),
        ("/src/b.ts", "const first = 0\n"),
    ]);
    let output = text_of(grep(&call, &json!({ "pattern": "const", "-A": 1 })));
    assert_eq!(
        output,
        "/src/a.ts\n  3:const alpha = 1\n  4-let beta = 2\n  --\n  9:const gamma = 3\n  \
         10-export default gamma\n\n/src/b.ts\n  1:const first = 0"
    );
    let tail = &output[output.len() - 30..];
    assert!(tail.ends_with("const first = 0"), "{tail:?}");
    assert!(!output.ends_with('\n'));
}

#[test]
fn context_c_with_a_override_prints_each_groups_lines_with_a_gap_between() {
    let call = seeded(&[("/a.txt", &file_with_hits(30, &[5, 20]))]);
    let output = text_of(grep(&call, &json!({ "pattern": "hit", "-C": 3, "-A": 1 })));
    let numbers: Vec<String> = output
        .lines()
        .skip(1)
        .map(|line| line.trim_start().to_owned())
        .collect();
    assert_eq!(
        numbers,
        [
            "2-.",
            "3-.",
            "4-.",
            "5:hit 5",
            "6-.",
            "--",
            "17-.",
            "18-.",
            "19-.",
            "20:hit 20",
            "21-."
        ]
    );
}

#[test]
fn overlapping_context_merges_and_no_line_prints_twice() {
    let call = seeded(&[("/a.txt", &file_with_hits(12, &[5, 7]))]);
    let output = text_of(grep(&call, &json!({ "pattern": "hit", "-C": 2 })));
    let numbers: Vec<String> = output
        .lines()
        .skip(1)
        .map(|line| line.trim_start().to_owned())
        .collect();
    assert_eq!(
        numbers,
        ["3-.", "4-.", "5:hit 5", "6-.", "7:hit 7", "8-.", "9-."],
        "lines 3 to 9 once, with no gap line"
    );
}

#[test]
fn touching_context_merges_without_a_gap() {
    let call = seeded(&[("/a.txt", &file_with_hits(12, &[3, 8]))]);
    let output = text_of(grep(&call, &json!({ "pattern": "hit", "-A": 2, "-B": 2 })));
    assert!(!output.contains("--"), "{output}");
}

#[test]
fn without_context_two_far_apart_matches_have_no_gap_line_between_them() {
    let call = seeded(&[("/a.txt", &file_with_hits(30, &[5, 20]))]);
    let output = text_of(grep(&call, &json!({ "pattern": "hit" })));
    assert_eq!(output, "/a.txt\n  5:hit 5\n  20:hit 20");
}

#[test]
fn a_blank_line_separates_files_and_none_ends_the_output() {
    let call = seeded(&[("/a.txt", "x\n"), ("/b.txt", "x\n")]);
    let output = text_of(grep(&call, &json!({ "pattern": "x" })));
    assert_eq!(output, "/a.txt\n  1:x\n\n/b.txt\n  1:x");
}

#[test]
fn count_mode_prints_the_path_and_its_matching_line_count() {
    let call = seeded(&[("/a.txt", "x\nx\ny\nx\n"), ("/b.txt", "y\n")]);
    let output = text_of(grep(
        &call,
        &json!({ "pattern": "x", "output_mode": "count" }),
    ));
    assert_eq!(output, "/a.txt:3");
}

#[test]
fn count_mode_counts_after_the_last_colon_when_the_name_has_one() {
    let call = seeded(&[("/a:b.txt", "x\nx\n")]);
    let output = text_of(grep(
        &call,
        &json!({ "pattern": "x", "output_mode": "count" }),
    ));
    assert_eq!(output, "/a:b.txt:2");
}

#[test]
fn the_byte_cap_cuts_the_body_on_a_line_and_says_where_to_continue() {
    let line = "x".repeat(100);
    let text = format!("{line}\n").repeat(400);
    let call = seeded(&[("/f.txt", &text)]);
    let output = text_of(grep(&call, &json!({ "pattern": "x" })));
    let (body, notice) = output
        .split_once("\n\n")
        .expect("a body and a notice are separated by a blank line");
    assert!(body.len() <= 15_000, "{} bytes", body.len());
    assert!(body.ends_with(&line), "the body ends on a whole line");
    assert_eq!(
        notice,
        "[Showing 1-141 of at least 142 matching lines. Output stopped at 15000 bytes. Use \
         offset=141 to see more, or narrow the search.]"
    );
}

#[test]
fn the_notice_after_a_cap_cut_names_the_offset_that_resumes_exactly_there() {
    let line = "x".repeat(100);
    let text = format!("{line}\n").repeat(400);
    let call = seeded(&[("/f.txt", &text)]);
    let next = text_of(grep(&call, &json!({ "pattern": "x", "offset": 141 })));
    assert!(next.starts_with("/f.txt\n  142:"), "{}", &next[..40]);
}

#[test]
fn the_cap_applies_to_the_other_modes_and_counts_each_line_with_its_newline() {
    let call = seeded(&[("/a", "x\n"), ("/b", "x\n"), ("/c", "x\n")]);
    let limits = Limits {
        body_cap: 6,
        ..Limits::default()
    };
    let output = text_of(grep_limited(
        &call,
        &json!({ "pattern": "x", "output_mode": "files_with_matches" }),
        &limits,
    ));
    assert_eq!(
        output,
        "/a\n/b\n\n[Showing 1-2 of at least 3 files. Output stopped at 6 bytes. Use offset=2 to \
         see more, or narrow the search.]"
    );
}

#[test]
fn a_cut_inside_a_files_lines_never_leaves_context_or_a_path_dangling() {
    let call = seeded(&[("/a.txt", &file_with_hits(40, &[10, 30]))]);
    for cap in 1..120 {
        let limits = Limits {
            body_cap: cap,
            ..Limits::default()
        };
        let output = text_of(grep_limited(
            &call,
            &json!({ "pattern": "hit", "-C": 2 }),
            &limits,
        ));
        let body = output.split("\n\n").next().expect("a body");
        assert!(
            body.len() < cap.max(1) || body.starts_with('['),
            "cap {cap}: {output}"
        );
        let last = body.lines().last().unwrap_or_default();
        assert!(
            !last.starts_with('/') && last != "  --",
            "cap {cap} left {last:?} dangling: {output}"
        );
        if let Some(first_row) = body.lines().nth(1) {
            let leading_context = first_row.contains('-');
            let has_match = body.lines().any(|row| row.contains(":hit"));
            assert!(
                !leading_context || has_match,
                "cap {cap} shows context with no match: {output}"
            );
        }
    }
}

#[test]
fn a_result_whose_first_entry_cannot_fit_says_so_instead_of_printing_nothing() {
    let call = seeded(&[("/a.txt", "x\n")]);
    let limits = Limits {
        body_cap: 3,
        ..Limits::default()
    };
    let output = text_of(grep_limited(&call, &json!({ "pattern": "x" }), &limits));
    assert_eq!(
        output,
        "[Output stopped at 3 bytes before the first result fit. Narrow the search with path, \
         glob or type.]"
    );
}
