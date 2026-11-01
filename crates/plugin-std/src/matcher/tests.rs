//! Unit tests for the glob matcher: each grammar rule, the refusals with
//! their reason texts, and the cases it shares with `Access::glob`, which
//! are pinned against that function's own tables.

use runvil_plugin::ToolId;
use runvil_plugin::testing::TestCall;

use super::{MAX_PATTERN_BYTES, Matcher, PatternError};

/// A matcher for `pattern`, which must be valid.
fn matcher(pattern: &str) -> Matcher {
    Matcher::new(pattern).expect("the pattern is valid")
}

/// Whether the valid `pattern` matches the absolute virtual `path`.
fn matches(pattern: &str, path: &str) -> bool {
    matcher(pattern).is_match(path)
}

/// The reason text of the refusal of `pattern`.
fn reason_of(pattern: &str) -> &'static str {
    match Matcher::new(pattern) {
        Ok(_) => panic!("{pattern:?} is refused"),
        Err(error) => error.reason(),
    }
}

#[test]
fn a_star_matches_zero_or_more_bytes_inside_one_segment() {
    assert!(matches("/a/*.rs", "/a/b.rs"));
    assert!(matches("/a/*.rs", "/a/.rs"));
    assert!(matches("/a/b*", "/a/b"));
    assert!(!matches("/a/*.rs", "/a/x/b.rs"));
    assert!(!matches("/a/*", "/a/x/y"));
}

#[test]
fn a_star_matches_dotfiles() {
    assert!(matches("/*", "/.env"));
    assert!(matches("/src/*", "/src/.hidden"));
    assert!(matches("*.log", "/.debug.log"));
    assert!(matches("/.*", "/.gitignore"));
}

#[test]
fn a_double_star_segment_matches_zero_or_more_whole_segments() {
    assert!(matches("/a/**/z", "/a/z"));
    assert!(matches("/a/**/z", "/a/b/z"));
    assert!(matches("/a/**/z", "/a/b/c/z"));
    assert!(!matches("/a/**/z", "/a/bz"));
    assert!(!matches("/a/**/z", "/b/z"));
}

#[test]
fn a_trailing_double_star_matches_everything_below() {
    assert!(matches("/a/**", "/a/b"));
    assert!(matches("/a/**", "/a/b/c/d"));
    assert!(!matches("/a/**", "/b/c"));
}

#[test]
fn every_byte_but_the_star_is_literal() {
    assert!(matches("/a?", "/a?"));
    assert!(!matches("/a?", "/ab"));
    assert!(matches("/[ab]", "/[ab]"));
    assert!(!matches("/[ab]", "/a"));
    assert!(matches("/{a,b}", "/{a,b}"));
    assert!(!matches("/{a,b}", "/a"));
    assert!(matches("/!x", "/!x"));
    assert!(matches("/a.rs", "/a.rs"));
    assert!(!matches("/a.rs", "/aXrs"));
}

#[test]
fn matching_is_case_sensitive() {
    assert!(matches("/README.md", "/README.md"));
    assert!(!matches("/README.md", "/readme.md"));
    assert!(!matches("*.TXT", "/a.txt"));
}

#[test]
fn a_pattern_without_a_leading_slash_matches_at_any_depth() {
    assert!(matches("node_modules", "/node_modules"));
    assert!(matches("node_modules", "/proj/node_modules"));
    assert!(matches("node_modules", "/a/b/c/node_modules"));
    assert!(matches("*.log", "/x.log"));
    assert!(matches("*.log", "/a/b/x.log"));
    assert!(matches("src/*.rs", "/proj/src/a.rs"));
}

#[test]
fn an_any_depth_pattern_still_matches_a_whole_segment_run() {
    assert!(!matches("node_modules", "/proj/my_node_modules"));
    assert!(!matches("node_modules", "/proj/node_modules_old"));
    assert!(!matches("src/*.rs", "/proj/mysrc/a.rs"));
    assert!(!matches("*.log", "/a.log/b"));
}

#[test]
fn a_pattern_with_a_leading_slash_is_a_full_path_from_the_root() {
    assert!(matches("/build", "/build"));
    assert!(!matches("/build", "/proj/build"));
    assert!(matches("/proj/build", "/proj/build"));
    assert!(!matches("/proj/build", "/other/proj/build"));
    assert!(matches("/", "/"));
    assert!(!matches("/", "/a"));
}

#[test]
fn a_leading_double_star_matches_with_or_without_the_slash_form() {
    assert!(matches("**/*.md", "/notes/today.md"));
    assert!(matches("**/*.md", "/today.md"));
    assert!(matches("/**/*.md", "/today.md"));
    assert!(matches("/**/*.md", "/a/b/today.md"));
}

#[test]
fn every_refusal_has_its_reason_text() {
    assert_eq!(reason_of(""), "pattern is empty");
    assert_eq!(
        reason_of(&"a".repeat(MAX_PATTERN_BYTES + 1)),
        "pattern is longer than 1024 bytes"
    );
    assert_eq!(reason_of("a\u{0}b"), "pattern contains a control character");
    assert_eq!(
        reason_of("a\u{1f}b"),
        "pattern contains a control character"
    );
    assert_eq!(
        reason_of("a\u{7f}b"),
        "pattern contains a control character"
    );
    assert_eq!(
        reason_of("a\\b"),
        "pattern does not support backslash escapes"
    );
    assert_eq!(
        reason_of("a***"),
        "more than two consecutive '*' are not supported"
    );
    assert_eq!(
        reason_of("a/***/b"),
        "more than two consecutive '*' are not supported"
    );
    assert_eq!(reason_of("a/**b"), "'**' must occupy a whole path segment");
    assert_eq!(reason_of("a**"), "'**' must occupy a whole path segment");
    assert_eq!(reason_of("**a/b"), "'**' must occupy a whole path segment");
}

#[test]
fn the_longest_allowed_pattern_is_1024_bytes() {
    assert_eq!(MAX_PATTERN_BYTES, 1024);
    assert!(Matcher::new(&"a".repeat(1024)).is_ok());
    assert_eq!(
        Matcher::new(&"a".repeat(1025)).err(),
        Some(PatternError::TooLong)
    );
}

#[test]
fn a_refusal_is_checked_in_a_fixed_order() {
    // Too long wins over a control byte, and a control byte over a
    // backslash, so the same bad pattern always hears the same reason.
    let long_and_control = format!("{}\u{0}", "a".repeat(MAX_PATTERN_BYTES));
    assert_eq!(
        reason_of(&long_and_control),
        "pattern is longer than 1024 bytes"
    );
    assert_eq!(reason_of("\u{0}\\"), "pattern contains a control character");
    assert_eq!(
        reason_of("\\***"),
        "pattern does not support backslash escapes"
    );
}

#[test]
fn the_validation_table_matches_the_one_access_glob_pins() {
    // The cases `glob.rs` pins in the vfs crate: the over-long, control,
    // backslash, and wildcard refusals, and one pattern that is fine.
    let over_long = "a".repeat(MAX_PATTERN_BYTES + 1);
    for (pattern, refused) in [
        (over_long.as_str(), true),
        ("a\u{0}b", true),
        ("a\\b", true),
        ("a/***/b", true),
        ("a/**b", true),
        ("ok/*.txt", false),
    ] {
        assert_eq!(
            Matcher::new(pattern).is_err(),
            refused,
            "{pattern:?} is refused: {refused}"
        );
    }
}

#[test]
fn the_match_table_is_the_one_access_glob_pins_when_anchored_with_a_leading_slash() {
    // The cases `glob.rs` pins, run anchored: each pattern and each path
    // gets a leading `/`, so a pattern is a full path from the root, as it
    // is for `Access::glob`.
    let paths = [
        "a.txt",
        "src/a.rs",
        "src/b.rs",
        "src/deep/c.rs",
        "src/deep/deeper/d.rs",
        "notes/today.md",
    ];
    for (pattern, expected) in [
        ("*.txt", [true, false, false, false, false, false]),
        ("src/*.rs", [false, true, true, false, false, false]),
        ("src/**/*.rs", [false, true, true, true, true, false]),
        ("**/*.md", [false, false, false, false, false, true]),
        ("src/**", [false, true, true, true, true, false]),
        ("no*match", [false, false, false, false, false, false]),
    ] {
        let matcher = matcher(&format!("/{pattern}"));
        for (path, expected) in paths.iter().zip(expected) {
            assert_eq!(
                matcher.is_match(&format!("/{path}")),
                expected,
                "pattern /{pattern} on /{path}"
            );
        }
    }
}

#[test]
fn an_unanchored_pattern_is_the_one_way_matching_differs_from_access_glob() {
    // `Access::glob("*.txt")` is anchored at the access root, so `*` never
    // crosses a `/` and only the top-level file comes back. The matcher
    // takes a pattern with no leading `/` at any depth.
    let call = TestCall::new(ToolId::parse("std/dir").expect("a two-segment tool id parses"));
    let access = call.context().access();
    access.write("/a.txt", b"").expect("the file is written");
    access
        .write("/x/y/a.txt", b"")
        .expect("the file is written");
    let globbed = access.glob("*.txt").expect("the pattern is valid");
    assert_eq!(globbed, ["a.txt"]);
    let any_depth = matcher("*.txt");
    assert!(any_depth.is_match("/a.txt"));
    assert!(any_depth.is_match("/x/y/a.txt"));
}

#[test]
fn a_long_text_does_not_blow_the_stack_or_the_clock() {
    // The match is an iterative sweep, so a deep path and a pattern full
    // of stars finish at once.
    let deep = format!("/{}", "d/".repeat(5_000));
    assert!(matches("**/**/**/x", &format!("{deep}x")));
    assert!(!matches("**/**/**/x", &format!("{deep}y")));
    assert!(matches("*a*a*a*a*b", &format!("/{}b", "a".repeat(2_000))));
    assert!(!matches("*a*a*a*a*b", &format!("/{}", "a".repeat(2_000))));
}

#[test]
fn the_reason_texts_are_distinct() {
    let reasons = [
        PatternError::Empty,
        PatternError::TooLong,
        PatternError::Control,
        PatternError::Backslash,
        PatternError::StarRun,
        PatternError::PartialDoubleStar,
    ]
    .map(PatternError::reason);
    for (index, reason) in reasons.iter().enumerate() {
        assert!(
            !reasons.iter().skip(index + 1).any(|other| other == reason),
            "{reason} appears twice"
        );
    }
}
