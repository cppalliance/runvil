//! How Grep compiles and applies the pattern: the dialect, the flags, and
//! what a bad pattern says.

use runvil_plugin::ToolErrorKind;
use serde_json::json;

use super::{failure_of, grep, new_call, seeded, text_of};

/// The sentence every refused pattern ends with.
const ADVICE: &str = "Grep matches with Rust regex syntax, which has no look-around and no \
                      back-references. To match the text literally, set literal to true.";

#[test]
fn a_pattern_the_dialect_cannot_compile_is_an_error_naming_the_pattern_the_reason_and_the_dialect()
{
    let cases = [
        (
            "(?=x)y",
            "look-around, including look-ahead and look-behind, is not supported",
        ),
        (
            "(?<=a)b",
            "look-around, including look-ahead and look-behind, is not supported",
        ),
        ("(a)\\1", "backreferences are not supported"),
        ("foo(", "unclosed group"),
    ];
    for (pattern, reason) in cases {
        let error = grep(&new_call(), &json!({ "pattern": pattern }))
            .expect_err("the pattern does not compile");
        assert_eq!(
            error.to_string(),
            format!("std_Grep: Invalid regular expression \"{pattern}\": {reason}. {ADVICE}")
        );
        assert_eq!(error.kind(), ToolErrorKind::InvalidArguments, "{pattern}");
    }
}

#[test]
fn a_bad_pattern_fails_before_any_path_is_looked_at() {
    let message = failure_of(grep(
        &new_call(),
        &json!({ "pattern": "foo(", "path": "/missing" }),
    ));
    assert!(message.contains("Invalid regular expression"), "{message}");
}

#[test]
fn a_pattern_that_holds_the_word_error_still_gets_its_own_reason() {
    let message = failure_of(grep(&new_call(), &json!({ "pattern": "error: (" })));
    assert_eq!(
        message,
        format!("std_Grep: Invalid regular expression \"error: (\": unclosed group. {ADVICE}")
    );
}

#[test]
fn literal_matches_the_pattern_as_plain_text() {
    let call = seeded(&[("/one.txt", "axb\n"), ("/two.txt", "a.b\n")]);
    let regex = text_of(grep(
        &call,
        &json!({ "pattern": "a.b", "output_mode": "count" }),
    ));
    assert_eq!(
        regex, "/one.txt:1\n/two.txt:1",
        "as a regex the dot matches x"
    );
    let literal = text_of(grep(
        &call,
        &json!({ "pattern": "a.b", "literal": true, "output_mode": "count" }),
    ));
    assert_eq!(literal, "/two.txt:1");
}

#[test]
fn literal_makes_a_pattern_that_would_not_compile_match_as_text() {
    let call = seeded(&[("/a.txt", "call foo( now\n")]);
    let output = text_of(grep(&call, &json!({ "pattern": "foo(", "literal": true })));
    assert_eq!(output, "/a.txt\n  1:call foo( now");
}

#[test]
fn matching_is_case_sensitive_unless_i_is_set() {
    let call = seeded(&[
        ("/a.txt", "Alpha\n"),
        ("/b.txt", "alpha\n"),
        ("/c.txt", "ALPHA\n"),
    ]);
    let exact = text_of(grep(
        &call,
        &json!({ "pattern": "alpha", "output_mode": "count" }),
    ));
    assert_eq!(exact, "/b.txt:1");
    let folded = text_of(grep(
        &call,
        &json!({ "pattern": "alpha", "-i": true, "output_mode": "count" }),
    ));
    assert_eq!(folded, "/a.txt:1\n/b.txt:1\n/c.txt:1");
}

#[test]
fn a_pattern_that_could_backtrack_forever_in_other_engines_completes_on_a_long_line() {
    let long = "a".repeat(1_600_000);
    let call = seeded(&[("/long.txt", &long), ("/almost.txt", &format!("{long}b"))]);
    let output = text_of(grep(
        &call,
        &json!({ "pattern": "^(a|aa)+$", "output_mode": "count" }),
    ));
    assert_eq!(output, "/long.txt:1");
}

#[test]
fn multiline_lets_dot_cross_a_line_break_and_a_pattern_span_lines() {
    let call = seeded(&[("/a.txt", "a\nb\n")]);
    let spanning = text_of(grep(&call, &json!({ "pattern": "a.b", "multiline": true })));
    assert_eq!(spanning, "/a.txt\n  1:a\n  2:b");
    let plain = grep(&call, &json!({ "pattern": "a.b" })).expect("the search succeeds");
    assert_eq!(
        plain.text(),
        "No matches found. Searched 1 file.\n\n\
         [No ignore rules ran: .gitignore is not applied, and only entries named .git are \
         skipped.]"
    );
}

#[test]
fn multiline_counts_every_line_a_match_touches_once() {
    let call = seeded(&[("/a.txt", "x\na\nb\nc\nx\n")]);
    let output = text_of(grep(
        &call,
        &json!({ "pattern": "a\nb\nc", "multiline": true, "output_mode": "count" }),
    ));
    assert_eq!(output, "/a.txt:3");
}

#[test]
fn carets_and_dollars_match_at_each_line() {
    let call = seeded(&[("/a.txt", "one\ntwo\nthree\n")]);
    let output = text_of(grep(
        &call,
        &json!({ "pattern": "^t", "output_mode": "count" }),
    ));
    assert_eq!(output, "/a.txt:2");
    let end = text_of(grep(
        &call,
        &json!({ "pattern": "e$", "output_mode": "count" }),
    ));
    assert_eq!(end, "/a.txt:2");
}
