//! `ignore_globs`: where a pattern matches, what a match leaves out, and
//! what a refused pattern says.

use runvil_plugin::{Entry, FileType, Stat, ToolErrorKind, VfsError};
use serde_json::json;

use super::{failure_of, list, seeded, text_of};
use crate::dir::walk::walk;
use crate::matcher::Matcher;

/// A tree with `node_modules`, `.log` files, and a `build` directory at
/// the root and one level down.
const FILES: [&str; 7] = [
    "/node_modules/x.js",
    "/src/node_modules/y.js",
    "/src/a.log",
    "/b.log",
    "/build/out.o",
    "/src/build/z.o",
    "/src/app.rs",
];

#[test]
fn a_pattern_with_no_leading_slash_leaves_out_entries_at_every_level() {
    let call = seeded(&FILES);
    let args = json!({ "target_directory": "/", "ignore_globs": ["node_modules", "*.log"] });
    let expected = "\
/
  - build/
    - out.o
  - src/
    - app.rs
    - build/
      - z.o";
    assert_eq!(text_of(list(&call, &args)), expected);
}

#[test]
fn a_pattern_with_a_leading_slash_leaves_out_only_the_entry_at_that_path() {
    let call = seeded(&FILES);
    let args = json!({ "target_directory": "/", "ignore_globs": ["/build"] });
    let expected = "\
/
  - b.log
  - node_modules/
    - x.js
  - src/
    - a.log
    - app.rs
    - build/
      - z.o
    - node_modules/
      - y.js";
    assert_eq!(text_of(list(&call, &args)), expected);
}

#[test]
fn a_pattern_is_matched_against_the_path_below_the_target_as_written() {
    let call = seeded(&FILES);
    let args = json!({ "target_directory": "/src", "ignore_globs": ["/src/build", "a.log"] });
    assert_eq!(
        text_of(list(&call, &args)),
        "/src/\n  - app.rs\n  - node_modules/\n    - y.js"
    );
}

#[test]
fn a_pattern_can_name_a_path_below_a_relative_target() {
    // The path a pattern is matched against has a leading `/` even when
    // the target was written without one.
    let call = seeded(&FILES);
    let args = json!({ "target_directory": "src", "ignore_globs": ["/src/build"] });
    assert_eq!(
        text_of(list(&call, &args)),
        "/src/\n  - a.log\n  - app.rs\n  - node_modules/\n    - y.js"
    );
}

#[test]
fn a_double_star_pattern_leaves_out_everything_below() {
    let call = seeded(&FILES);
    let args = json!({ "target_directory": "/", "ignore_globs": ["src/**"] });
    assert_eq!(
        text_of(list(&call, &args)),
        "/\n  - b.log\n  - build/\n    - out.o\n  - node_modules/\n    - x.js\n  - src/"
    );
}

#[test]
fn a_matching_directory_is_not_entered() {
    let lister = |path: &str| -> Result<Vec<Entry>, VfsError> {
        let directory = |name: &str| Entry::new(name, Stat::new(FileType::Directory, 0));
        match path {
            "/p" => Ok(vec![directory("node_modules"), directory("src")]),
            "/p/src" => Ok(vec![Entry::new("a.rs", Stat::new(FileType::File, 0))]),
            other => panic!("the walk listed {other}"),
        }
    };
    let ignore = [Matcher::new("node_modules").expect("the pattern is valid")];
    let tree = walk("/p", &ignore, 15_000, lister).expect("the walk succeeds");
    assert_eq!(
        crate::dir::render::render(&tree),
        "/p/\n  - src/\n    - a.rs"
    );
}

#[test]
fn a_pattern_that_matches_nothing_changes_nothing() {
    let call = seeded(&["/p/a.md", "/p/sub/b.md"]);
    let with = text_of(list(
        &call,
        &json!({ "target_directory": "/p", "ignore_globs": ["nothing-here"] }),
    ));
    let without = text_of(list(&call, &json!({ "target_directory": "/p" })));
    assert_eq!(with, without);
}

#[test]
fn an_empty_list_of_patterns_and_a_null_one_leave_nothing_out() {
    let call = seeded(&["/p/a.md"]);
    let plain = text_of(list(&call, &json!({ "target_directory": "/p" })));
    for ignore in [json!([]), json!(null)] {
        let args = json!({ "target_directory": "/p", "ignore_globs": ignore });
        assert_eq!(text_of(list(&call, &args)), plain, "{args}");
    }
}

#[test]
fn a_refused_pattern_is_an_invalid_arguments_error_with_the_reason() {
    let call = seeded(&["/p/a.md"]);
    for (pattern, reason) in [
        ("a***", "more than two consecutive '*' are not supported"),
        ("a/**b", "'**' must occupy a whole path segment"),
        ("", "pattern is empty"),
        ("a\\b", "pattern does not support backslash escapes"),
        ("a\u{1}b", "pattern contains a control character"),
    ] {
        let args = json!({ "target_directory": "/p", "ignore_globs": [pattern] });
        let error = list(&call, &args).expect_err("the pattern is refused");
        assert_eq!(error.kind(), ToolErrorKind::InvalidArguments, "{pattern:?}");
        assert_eq!(
            error.to_string(),
            format!(
                "std_ListDir: `ignore_globs` is invalid: \"{pattern}\" does not parse: {reason}."
            )
        );
    }
}

#[test]
fn an_over_long_pattern_is_echoed_cut_to_80_bytes_with_dots() {
    let call = seeded(&["/p/a.md"]);
    let pattern = format!("{}***", "a".repeat(100));
    let args = json!({ "target_directory": "/p", "ignore_globs": [pattern] });
    assert_eq!(
        failure_of(list(&call, &args)),
        format!(
            "std_ListDir: `ignore_globs` is invalid: \"{}...\" does not parse: \
             more than two consecutive '*' are not supported.",
            "a".repeat(80)
        )
    );
}

#[test]
fn the_first_refused_pattern_is_the_one_named() {
    let call = seeded(&["/p/a.md"]);
    let args = json!({ "target_directory": "/p", "ignore_globs": ["ok", "a***", "a/**b"] });
    assert_eq!(
        failure_of(list(&call, &args)),
        "std_ListDir: `ignore_globs` is invalid: \"a***\" does not parse: \
         more than two consecutive '*' are not supported."
    );
}

#[test]
fn a_pattern_is_refused_before_the_path_is_looked_at() {
    let call = seeded(&[]);
    let args = json!({ "target_directory": "/does/not/exist", "ignore_globs": ["a***"] });
    assert_eq!(
        failure_of(list(&call, &args)),
        "std_ListDir: `ignore_globs` is invalid: \"a***\" does not parse: \
         more than two consecutive '*' are not supported."
    );
}
