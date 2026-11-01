//! Which files Grep visits and in what order, how a head limit and an
//! offset count across files, and what is trusted.

use runvil_plugin::OutputTrust;
use serde_json::json;

use super::{grep, new_call, seeded, text_of};

/// The files that sort differently by name and by full path.
const TRICKY: [(&str, &str); 5] = [
    ("/B", "x\n"),
    ("/a-b", "x\n"),
    ("/a.txt", "x\n"),
    ("/a/x", "x\n"),
    ("/b", "x\n"),
];

/// A file of `lines` lines where each line in `at` holds `hit` and every
/// other line holds `.`.
fn file_with_hits(lines: usize, at: &[usize]) -> String {
    (1..=lines)
        .map(|number| if at.contains(&number) { "hit\n" } else { ".\n" })
        .collect()
}

#[test]
fn files_come_in_full_path_order_in_every_mode() {
    let call = seeded(&TRICKY);
    let content = text_of(grep(&call, &json!({ "pattern": "x" })));
    assert_eq!(
        content,
        "/B\n  1:x\n\n/a-b\n  1:x\n\n/a.txt\n  1:x\n\n/a/x\n  1:x\n\n/b\n  1:x"
    );
    let files = text_of(grep(
        &call,
        &json!({ "pattern": "x", "output_mode": "files_with_matches" }),
    ));
    assert_eq!(files, "/B\n/a-b\n/a.txt\n/a/x\n/b");
    let count = text_of(grep(
        &call,
        &json!({ "pattern": "x", "output_mode": "count" }),
    ));
    assert_eq!(count, "/B:1\n/a-b:1\n/a.txt:1\n/a/x:1\n/b:1");
}

#[test]
fn the_order_is_the_order_glob_gives() {
    let call = seeded(&TRICKY);
    let globbed = call
        .context()
        .access()
        .glob("/**")
        .expect("the glob matches");
    let files: Vec<String> = globbed
        .into_iter()
        .filter(|path| TRICKY.iter().any(|(file, _)| file == path))
        .collect();
    let searched = text_of(grep(
        &call,
        &json!({ "pattern": "x", "output_mode": "files_with_matches" }),
    ));
    assert_eq!(searched, files.join("\n"));
}

#[test]
fn a_search_of_one_directory_prints_paths_as_absolute_virtual_paths() {
    let call = seeded(&[("/proj/src/a.txt", "x\n"), ("/other.txt", "x\n")]);
    for path in ["/proj", "/proj/", "proj"] {
        let output = text_of(grep(
            &call,
            &json!({ "pattern": "x", "path": path, "output_mode": "files_with_matches" }),
        ));
        assert_eq!(output, "/proj/src/a.txt", "{path}");
    }
}

#[test]
fn a_search_of_one_file_names_that_file() {
    let call = seeded(&[("/proj/a.txt", "one\ntwo\n"), ("/proj/b.txt", "two\n")]);
    let output = text_of(grep(
        &call,
        &json!({ "pattern": "two", "path": "/proj/a.txt" }),
    ));
    assert_eq!(output, "/proj/a.txt\n  2:two");
}

#[test]
fn entries_named_git_are_skipped_at_any_depth_and_other_dotfiles_are_searched() {
    let call = seeded(&[
        ("/.git/config", "x\n"),
        ("/sub/.git/HEAD", "x\n"),
        ("/.gitignore", "x\n"),
        ("/.env", "x\n"),
        ("/target/out.txt", "x\n"),
    ]);
    let output = text_of(grep(
        &call,
        &json!({ "pattern": "x", "output_mode": "files_with_matches" }),
    ));
    assert_eq!(output, "/.env\n/.gitignore\n/target/out.txt");
}

#[test]
fn head_limit_counts_across_files_and_stops_inside_the_file_it_reaches() {
    let call = seeded(&[
        ("/a.ts", &file_with_hits(45, &[12, 40])),
        ("/b.ts", &file_with_hits(9, &[1, 2, 3])),
    ]);
    let output = text_of(grep(&call, &json!({ "pattern": "hit", "head_limit": 2 })));
    assert_eq!(
        output,
        "/a.ts\n  12:hit\n  40:hit\n\n\
         [Showing 1-2 of at least 3 matching lines. Use offset=2 to see more, or narrow the \
         search.]"
    );
}

#[test]
fn an_offset_skips_entries_across_files_before_the_head_limit_applies() {
    let call = seeded(&[
        ("/f1", "m\nm\nm\n"),
        ("/f2", "m\nm\nm\n"),
        ("/f3", "m\nm\nm\n"),
    ]);
    let output = text_of(grep(
        &call,
        &json!({ "pattern": "m", "offset": 4, "head_limit": 2 }),
    ));
    assert_eq!(
        output,
        "/f2\n  2:m\n  3:m\n\n\
         [Showing 5-6 of at least 7 matching lines. Use offset=6 to see more, or narrow the \
         search.]"
    );
}

#[test]
fn head_limit_zero_prints_every_match() {
    let text = "m\n".repeat(40);
    let call = seeded(&[("/a.txt", &text)]);
    let output = text_of(grep(&call, &json!({ "pattern": "m", "head_limit": 0 })));
    assert_eq!(
        output.lines().count(),
        41,
        "the path and 40 matches, no notice"
    );
    assert!(output.ends_with("\n  40:m"), "{output}");
}

#[test]
fn a_head_limit_that_exactly_fits_prints_no_notice() {
    let call = seeded(&[("/a.txt", "m\nm\n")]);
    let output = text_of(grep(&call, &json!({ "pattern": "m", "head_limit": 2 })));
    assert_eq!(output, "/a.txt\n  1:m\n  2:m");
}

#[test]
fn head_limit_with_context_counts_entries_and_keeps_their_context_lines() {
    let call = seeded(&[("/a.txt", &file_with_hits(30, &[1, 20, 28]))]);
    let output = text_of(grep(
        &call,
        &json!({ "pattern": "hit", "-A": 5, "head_limit": 2 }),
    ));
    assert_eq!(
        output,
        "/a.txt\n  1:hit\n  2-.\n  3-.\n  4-.\n  5-.\n  6-.\n  --\n  20:hit\n  21-.\n  22-.\n  \
         23-.\n  24-.\n  25-.\n\n\
         [Showing 1-2 of at least 3 matching lines. Use offset=2 to see more, or narrow the \
         search.]"
    );
}

#[test]
fn files_with_matches_and_count_page_by_file() {
    let call = seeded(&[("/a", "m\nm\n"), ("/b", "m\n"), ("/c", "m\n")]);
    let files = text_of(grep(
        &call,
        &json!({ "pattern": "m", "output_mode": "files_with_matches", "head_limit": 2 }),
    ));
    assert_eq!(
        files,
        "/a\n/b\n\n[Showing 1-2 of at least 3 files. Use offset=2 to see more, or narrow the \
         search.]"
    );
    let count = text_of(grep(
        &call,
        &json!({ "pattern": "m", "output_mode": "count", "offset": 1 }),
    ));
    assert_eq!(count, "/b:1\n/c:1\n\n[Showing 2-3 of 3 files.]");
}

#[test]
fn a_result_with_matches_is_untrusted_output() {
    let call = seeded(&[("/a.txt", "needle\n")]);
    let output = grep(&call, &json!({ "pattern": "needle" })).expect("the search succeeds");
    assert_eq!(output.trust(), OutputTrust::Untrusted);
}

#[test]
fn a_bare_zero_match_result_is_trusted_and_a_past_the_end_one_is_not() {
    let call = seeded(&[("/a.txt", "needle\n")]);
    let none = grep(&call, &json!({ "pattern": "absent" })).expect("the search succeeds");
    assert_eq!(none.trust(), OutputTrust::Trusted);
    assert_eq!(
        none.text(),
        "No matches found. Searched 1 file.\n\n\
         [No ignore rules ran: .gitignore is not applied, and only entries named .git are \
         skipped.]"
    );
    let past =
        grep(&call, &json!({ "pattern": "needle", "offset": 5 })).expect("the search succeeds");
    assert_eq!(past.text(), "No more results at offset=5.");
    assert_eq!(past.trust(), OutputTrust::Untrusted);
}

#[test]
fn searching_nothing_says_no_files_were_searched() {
    let output = grep(&new_call(), &json!({ "pattern": "x" })).expect("the search succeeds");
    assert!(
        output
            .text()
            .starts_with("No matches found. Searched 0 files."),
        "{}",
        output.text()
    );
}

#[test]
fn the_description_names_the_file_limit_the_code_uses() {
    let text = &super::definition().description;
    let limit = crate::grep::found::MAX_FILES.to_string();
    assert!(text.contains(&limit), "{limit} is named: {text}");
}
