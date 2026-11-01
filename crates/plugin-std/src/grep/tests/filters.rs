//! The `glob` and `type` filters, and the one case where neither applies.

use serde_json::json;

use super::{grep, seeded, text_of};

/// The paths of the files with a match for `x` under `args`.
fn files(call: &runvil_plugin::testing::TestCall, mut args: serde_json::Value) -> String {
    args["pattern"] = json!("x");
    args["output_mode"] = json!("files_with_matches");
    text_of(grep(call, &args))
}

#[test]
fn a_glob_without_a_leading_slash_matches_at_any_depth() {
    let call = seeded(&[("/a.js", "x\n"), ("/d/e/b.js", "x\n"), ("/c.ts", "x\n")]);
    assert_eq!(files(&call, json!({ "glob": "*.js" })), "/a.js\n/d/e/b.js");
}

#[test]
fn a_glob_with_a_leading_slash_is_a_full_path() {
    let call = seeded(&[
        ("/src/a.rs", "x\n"),
        ("/src/deep/b.rs", "x\n"),
        ("/other/c.rs", "x\n"),
        ("/src/d.txt", "x\n"),
    ]);
    assert_eq!(
        files(&call, json!({ "glob": "/src/**/*.rs" })),
        "/src/a.rs\n/src/deep/b.rs"
    );
}

#[test]
fn a_type_keeps_the_files_with_its_extensions() {
    let call = seeded(&[
        ("/lib.rs", "x\n"),
        ("/main.RS", "x\n"),
        ("/notes.md", "x\n"),
        ("/sub/mod.rs", "x\n"),
    ]);
    assert_eq!(
        files(&call, json!({ "type": "rust" })),
        "/lib.rs\n/sub/mod.rs"
    );
}

#[test]
fn a_glob_and_a_type_together_keep_only_what_passes_both() {
    let call = seeded(&[("/a/x.rs", "x\n"), ("/b/x.rs", "x\n"), ("/a/y.txt", "x\n")]);
    assert_eq!(
        files(&call, json!({ "glob": "/a/**", "type": "rust" })),
        "/a/x.rs"
    );
}

#[test]
fn a_named_single_file_ignores_both_filters() {
    let call = seeded(&[("/c.ts", "x\n")]);
    let output = files(
        &call,
        json!({ "path": "/c.ts", "glob": "*.js", "type": "rust" }),
    );
    assert_eq!(output, "/c.ts");
}

#[test]
fn a_file_a_filter_drops_is_neither_searched_nor_a_skip() {
    let call = seeded(&[("/a.js", "x\n"), ("/b.bin.ts", "x\n")]);
    let output =
        grep(&call, &json!({ "pattern": "zzz", "glob": "*.js" })).expect("the search succeeds");
    assert!(
        output
            .text()
            .starts_with("No matches found. Searched 1 file."),
        "{}",
        output.text()
    );
    assert!(!output.text().contains("Skipped"), "{}", output.text());
}

#[test]
fn a_zero_match_search_with_a_brace_glob_adds_the_hint_line() {
    let call = seeded(&[("/a.ts", "x\n")]);
    let output =
        grep(&call, &json!({ "pattern": "x", "glob": "*.{ts,tsx}" })).expect("the search succeeds");
    assert_eq!(
        output.text(),
        "No matches found. Searched 0 files.\n\n\
         [No ignore rules ran: .gitignore is not applied, and only entries named .git are \
         skipped.]\n\
         {, [ and ? in glob match themselves; there is no alternation or character class. Call \
         Grep once per alternative, or filter with type."
    );
}

#[test]
fn a_bracket_or_a_question_mark_in_the_glob_also_adds_the_hint_and_a_plain_glob_does_not() {
    let call = seeded(&[("/a.ts", "x\n")]);
    for glob in ["*.[jt]s", "*.t?"] {
        let output =
            grep(&call, &json!({ "pattern": "x", "glob": glob })).expect("the search succeeds");
        assert!(
            output
                .text()
                .contains("{, [ and ? in glob match themselves"),
            "{glob}: {}",
            output.text()
        );
    }
    let plain =
        grep(&call, &json!({ "pattern": "zzz", "glob": "*.ts" })).expect("the search succeeds");
    assert!(
        !plain.text().contains("match themselves"),
        "{}",
        plain.text()
    );
}

#[test]
fn a_result_with_matches_carries_no_hint() {
    let call = seeded(&[("/a.ts", "x\n")]);
    let output = text_of(grep(&call, &json!({ "pattern": "x", "glob": "*.ts" })));
    assert_eq!(output, "/a.ts\n  1:x");
}
