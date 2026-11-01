//! The byte cap: the walk is breadth first, stops when the next line would
//! pass the cap, marks what it did not expand, and ends with its note.

use std::collections::HashSet;

use runvil_plugin::OutputTrust;
use runvil_plugin::testing::TestCall;

use super::{list_capped, seeded, text_of};
use crate::text::BODY_CAP;

/// The note the output ends with after the `    ...` line and a blank
/// line.
const TOO_LARGE: &str = "target_directory is too large (either too many children or their names \
     are too long). Immediate children were truncated. Try a more specific target_directory or \
     ignore_globs.";

/// Two files at the top and two small directories, each with one file.
///
/// The lines, with the bytes each counts with its newline, are the root
/// `/p/` (4), `  - a.txt` (10), `  - b/` (7), `  - c/` (7), `    - x.txt`
/// (12), and `    - y.txt` (12).
fn small() -> TestCall {
    seeded(&["/p/a.txt", "/p/b/x.txt", "/p/c/y.txt"])
}

#[test]
fn the_default_cap_is_fifteen_thousand_bytes() {
    assert_eq!(BODY_CAP, 15_000);
}

#[test]
fn a_tree_that_fits_has_no_note() {
    let call = small();
    let text = text_of(list_capped(&call, "/p", &[], 52));
    assert_eq!(
        text,
        "/p/\n  - a.txt\n  - b/\n    - x.txt\n  - c/\n    - y.txt"
    );
}

#[test]
fn a_tree_that_just_passes_the_cap_stops_at_the_last_line_that_fits() {
    let call = small();
    let text = text_of(list_capped(&call, "/p", &[], 51));
    let expected =
        format!("/p/\n  - a.txt\n  - b/\n    - x.txt\n  - c/...\n    ...\n\n{TOO_LARGE}");
    assert_eq!(text, expected);
}

#[test]
fn the_note_follows_a_dots_line_and_a_blank_line_byte_for_byte() {
    let call = small();
    let text = text_of(list_capped(&call, "/p", &[], 40));
    let expected = [
        "/p/",
        "  - a.txt",
        "  - b/",
        "    - x.txt",
        "  - c/...",
        "    ...",
        "",
        TOO_LARGE,
    ]
    .join("\n");
    assert_eq!(text, expected);
    assert!(text.ends_with("ignore_globs."));
}

#[test]
fn a_directory_whose_first_child_does_not_fit_is_marked_unexpanded() {
    let call = small();
    let text = text_of(list_capped(&call, "/p", &[], 39));
    let expected = format!("/p/\n  - a.txt\n  - b/...\n  - c/...\n    ...\n\n{TOO_LARGE}");
    assert_eq!(text, expected);
}

#[test]
fn the_directory_being_listed_keeps_the_children_already_added() {
    // Cap 21 holds the root, `a.txt`, and `b/`. The root's `c/` does not
    // fit, so the walk stops with `c` not in the tree at all, and `b` is
    // shown but never expanded.
    let call = small();
    let text = text_of(list_capped(&call, "/p", &[], 21));
    let expected = format!("/p/\n  - a.txt\n  - b/...\n    ...\n\n{TOO_LARGE}");
    assert_eq!(text, expected);
}

#[test]
fn the_walk_reaches_the_top_levels_before_going_deeper() {
    // Breadth first: with room for the root's children and one more line,
    // the one more line is the first child of the first directory, not a
    // grandchild reached by going deep first.
    let call = seeded(&["/r/a/deep/f.txt", "/r/a/g.txt", "/r/b/h.txt", "/r/z.txt"]);
    let text = text_of(list_capped(&call, "/r", &[], 40));
    assert!(text.contains("  - a/\n    - deep/...\n"), "{text}");
    assert!(text.contains("  - b/...\n"), "{text}");
    assert!(!text.contains("f.txt"), "{text}");
}

#[test]
fn nothing_fits_when_the_cap_is_smaller_than_the_root_line() {
    let call = small();
    let text = text_of(list_capped(&call, "/p", &[], 3));
    assert_eq!(text, format!("/p/\n    ...\n\n{TOO_LARGE}"));
}

#[test]
fn the_note_and_the_dots_do_not_count_against_the_cap() {
    // The lines that stay in the tree use at most the cap, though the
    // output is longer because of the note and the `...` suffixes.
    let call = small();
    let text = text_of(list_capped(&call, "/p", &[], 40));
    let kept: usize = text
        .lines()
        .take_while(|line| *line != "    ...")
        .map(|line| line.trim_end_matches("...").len() + 1)
        .sum();
    assert!(kept <= 40, "{kept} bytes of tree lines in {text}");
    assert!(text.len() > 40);
}

#[test]
fn a_truncated_listing_is_a_successful_untrusted_answer() {
    let call = small();
    let output = list_capped(&call, "/p", &[], 21).expect("a truncated listing succeeds");
    assert_eq!(output.trust(), OutputTrust::Untrusted);
}

#[test]
fn a_cap_sweep_over_multi_byte_names_never_cuts_inside_a_name_or_a_line() {
    let files = [
        "/w/é/日本語/ファイル.txt",
        "/w/é/ü.md",
        "/w/📦pkg/readme.md",
        "/w/📦pkg/📄.txt",
        "/w/日本/a.md",
        "/w/ñandú.rs",
        "/w/z.txt",
    ];
    let call = seeded(&files);
    let full = text_of(list_capped(&call, "/w", &[], BODY_CAP));
    assert!(!full.contains("target_directory is too large"), "{full}");
    let names: HashSet<&str> = full
        .lines()
        .skip(1)
        .map(|line| {
            line.trim_start()
                .trim_start_matches("- ")
                .trim_end_matches('/')
        })
        .collect();
    let all_bytes = full.len() + 1;
    for cap in 0..=all_bytes {
        let text = text_of(list_capped(&call, "/w", &[], cap));
        let mut lines = text.lines();
        assert_eq!(lines.next(), Some("/w/"), "cap {cap}");
        let mut kept = "/w/".len() + 1;
        let mut seen_note = false;
        for line in lines {
            if line == "    ..." {
                seen_note = true;
                break;
            }
            let stripped = line.trim_end_matches("...");
            assert!(line.starts_with(' '), "cap {cap}: {line:?}");
            let name = stripped
                .trim_start()
                .strip_prefix("- ")
                .unwrap_or_else(|| panic!("cap {cap}: {line:?} is not a bullet line"))
                .trim_end_matches('/');
            assert!(names.contains(name), "cap {cap}: {name:?} is cut in {text}");
            kept += stripped.len() + 1;
        }
        assert_eq!(seen_note, cap < all_bytes, "cap {cap}: {text}");
        if seen_note {
            assert!(text.ends_with(TOO_LARGE), "cap {cap}: {text}");
            assert!(kept <= cap.max(4), "cap {cap}: {kept} bytes kept in {text}");
        }
    }
}
