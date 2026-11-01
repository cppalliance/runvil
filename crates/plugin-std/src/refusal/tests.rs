//! Unit tests for the refusal texts: one sentence per `VfsError`, byte
//! for byte, with the path echoed as the model wrote it.

use std::io::ErrorKind;

use runvil_plugin::{PathReason, VfsError};

use super::{missing_root, neither_file_nor_directory, not_a_directory, refusal};

/// The path the model wrote, and the canonical path every error below
/// carries, which differs so a test can tell which one a sentence echoes.
const WRITTEN: &str = "notes/a.md";
const CANONICAL: &str = "/canonical/a.md";

/// What `refusal` says for `error` when `Read` was called on
/// `notes/a.md`.
fn say(error: &VfsError) -> String {
    refusal("Read", WRITTEN, error)
}

#[test]
fn not_found_says_the_file_was_not_found_with_the_path_as_written() {
    let error = VfsError::NotFound {
        path: CANONICAL.to_owned(),
    };
    assert_eq!(say(&error), "File not found: notes/a.md");
}

#[test]
fn a_directory_where_a_file_was_named_says_so() {
    let error = VfsError::IsADirectory {
        path: CANONICAL.to_owned(),
    };
    assert_eq!(say(&error), "Path is a directory, not a file: notes/a.md");
}

#[test]
fn a_file_where_a_directory_was_named_says_so() {
    let error = VfsError::NotADirectory {
        path: CANONICAL.to_owned(),
    };
    assert_eq!(say(&error), "Path is not a directory: notes/a.md");
}

#[test]
fn a_conflict_says_the_path_changed_this_round_and_names_the_tool_to_call_again() {
    let error = VfsError::Conflict {
        path: CANONICAL.to_owned(),
        detail: "write by another call".to_owned(),
    };
    assert_eq!(
        say(&error),
        "notes/a.md was changed, or is being changed, by another call in this round, so it \
         cannot be read in this round. Call Read again in your next message."
    );
    assert_eq!(
        refusal("Grep", "/src", &error),
        "/src was changed, or is being changed, by another call in this round, so it \
         cannot be read in this round. Call Grep again in your next message."
    );
}

#[test]
fn an_io_failure_names_the_path_the_kind_and_the_message() {
    let error = VfsError::Io {
        path: CANONICAL.to_owned(),
        kind: ErrorKind::TimedOut,
        raw_os_error: Some(110),
        message: "the disk did not answer".to_owned(),
    };
    assert_eq!(
        say(&error),
        "I/O failure on notes/a.md (timed out): the disk did not answer"
    );
}

#[test]
fn a_denied_or_unapproved_read_passes_the_policys_reason_through_unchanged() {
    let reason = "Mode::Plan allows only markdown paths, and /secret.rs is not one";
    let denied = VfsError::PermissionDenied {
        path: CANONICAL.to_owned(),
        reason: reason.to_owned(),
    };
    assert_eq!(say(&denied), reason);
    let approval = "Reading /vault needs approval (rule: vault)";
    let asked = VfsError::ApprovalRequired {
        path: CANONICAL.to_owned(),
        reason: approval.to_owned(),
    };
    assert_eq!(say(&asked), approval);
}

#[test]
fn every_other_variant_gives_its_display_text_without_the_leading_label() {
    let cases: Vec<(VfsError, &str)> = vec![
        (
            VfsError::AlreadyExists {
                path: CANONICAL.to_owned(),
            },
            "/canonical/a.md",
        ),
        (
            VfsError::DirectoryNotEmpty {
                path: CANONICAL.to_owned(),
            },
            "/canonical/a.md",
        ),
        (
            VfsError::NotUtf8 {
                path: CANONICAL.to_owned(),
                valid_up_to: 4,
            },
            "/canonical/a.md, valid up to byte 4",
        ),
        (
            VfsError::InvalidPath {
                path: "a\\b".to_owned(),
                reason: PathReason::Backslash,
            },
            "path contains a backslash",
        ),
        (
            VfsError::InvalidRange {
                path: CANONICAL.to_owned(),
                reason: "the range is empty",
            },
            "the range is empty",
        ),
        (
            VfsError::Anchor {
                path: CANONICAL.to_owned(),
                anchor: String::new(),
                count: 0,
            },
            "/canonical/a.md",
        ),
        (
            VfsError::Anchor {
                path: CANONICAL.to_owned(),
                anchor: "old".to_owned(),
                count: 0,
            },
            "anchor \"old\" was not found in /canonical/a.md, expected exactly one",
        ),
    ];
    for (error, expected) in cases {
        assert_eq!(say(&error), expected, "{error:?}");
    }
}

#[test]
fn the_state_and_backend_variants_also_drop_the_leading_label() {
    let cases: Vec<(VfsError, &str)> = vec![
        (
            VfsError::ScopeEnded {
                path: CANONICAL.to_owned(),
            },
            "the run or tool call that owned this access has ended, so /canonical/a.md can no \
             longer be touched through it",
        ),
        (
            VfsError::ReadOnly {
                path: CANONICAL.to_owned(),
            },
            "/canonical/a.md is served read-only and cannot change",
        ),
        (
            VfsError::OutsideRoot {
                path: CANONICAL.to_owned(),
                detail: "/canonical/a.md is a link to /etc/passwd, which is outside the root"
                    .to_owned(),
            },
            "/canonical/a.md is a link to /etc/passwd, which is outside the root",
        ),
        (
            VfsError::RootProtected {
                path: "/".to_owned(),
            },
            "the mounted root / cannot be removed, renamed, or replaced",
        ),
        (VfsError::NoStore, "the handle declares no store"),
        (
            VfsError::CrossMount {
                path: CANONICAL.to_owned(),
                detail: "copy /a to /b".to_owned(),
            },
            "copy /a to /b",
        ),
        (
            VfsError::Unsupported {
                path: CANONICAL.to_owned(),
                detail: "this backend cannot seek".to_owned(),
            },
            "this backend cannot seek",
        ),
        (
            VfsError::Backend {
                message: "the store is offline".to_owned(),
            },
            "the store is offline",
        ),
    ];
    for (error, expected) in cases {
        assert_eq!(say(&error), expected, "{error:?}");
    }
}

#[test]
fn an_outside_root_error_reads_as_its_detail() {
    let error = VfsError::OutsideRoot {
        path: CANONICAL.to_owned(),
        detail: "the link leaves the run's filesystem".to_owned(),
    };
    assert_eq!(say(&error), "the link leaves the run's filesystem");
}

#[test]
fn a_display_text_with_no_label_is_kept_whole() {
    let error = VfsError::Anchor {
        path: CANONICAL.to_owned(),
        anchor: "x".to_owned(),
        count: 3,
    };
    assert_eq!(
        say(&error),
        "anchor \"x\" occurs 3 times in /canonical/a.md, expected exactly one"
    );
}

#[test]
fn a_relative_path_is_echoed_as_written_and_never_as_the_canonical_one() {
    let error = VfsError::NotFound {
        path: "/notes/a.md".to_owned(),
    };
    let text = say(&error);
    assert!(text.ends_with("notes/a.md"), "{text}");
    assert!(!text.contains("/notes/a.md"), "{text}");
}

#[test]
fn no_message_names_a_tool_other_than_the_one_called() {
    let others = ["Write", "StrReplace", "Delete", "Glob", "Grep", "Shell"];
    let errors = [
        VfsError::NotFound {
            path: CANONICAL.to_owned(),
        },
        VfsError::IsADirectory {
            path: CANONICAL.to_owned(),
        },
        VfsError::NotADirectory {
            path: CANONICAL.to_owned(),
        },
        VfsError::Conflict {
            path: CANONICAL.to_owned(),
            detail: "x".to_owned(),
        },
        VfsError::Io {
            path: CANONICAL.to_owned(),
            kind: ErrorKind::Other,
            raw_os_error: None,
            message: "x".to_owned(),
        },
        VfsError::Anchor {
            path: CANONICAL.to_owned(),
            anchor: String::new(),
            count: 0,
        },
        VfsError::ReadOnly {
            path: CANONICAL.to_owned(),
        },
        VfsError::NoStore,
        VfsError::DirectoryNotEmpty {
            path: CANONICAL.to_owned(),
        },
    ];
    for error in errors {
        let text = say(&error);
        for other in others {
            assert!(!text.contains(other), "{error:?} says {text}");
        }
    }
    assert!(!neither_file_nor_directory("p").contains("Shell"));
}

#[test]
fn a_root_that_does_not_exist_says_the_path_does_not_exist() {
    assert_eq!(
        missing_root("/home/dev/gone"),
        "Path does not exist: /home/dev/gone"
    );
    assert_eq!(missing_root("gone"), "Path does not exist: gone");
}

#[test]
fn a_missing_root_is_worded_apart_from_a_missing_file() {
    // The `NotFound` sentence says file, which is wrong for a directory
    // the model named, so the listing and search tools word the root
    // themselves.
    let error = VfsError::NotFound {
        path: CANONICAL.to_owned(),
    };
    assert_eq!(say(&error), "File not found: notes/a.md");
    assert_eq!(missing_root(WRITTEN), "Path does not exist: notes/a.md");
}

#[test]
fn the_not_a_directory_sentence_is_the_one_the_refusal_gives() {
    let error = VfsError::NotADirectory {
        path: CANONICAL.to_owned(),
    };
    assert_eq!(say(&error), not_a_directory(WRITTEN));
    assert_eq!(not_a_directory("/a/b"), "Path is not a directory: /a/b");
}

#[test]
fn the_root_sentences_name_no_tool() {
    for text in [missing_root("/p"), not_a_directory("/p")] {
        for tool in [
            "Read",
            "Write",
            "StrReplace",
            "Delete",
            "Glob",
            "Grep",
            "Shell",
        ] {
            assert!(!text.contains(tool), "{text}");
        }
    }
}

#[test]
fn a_special_file_says_it_is_neither_a_file_nor_a_directory() {
    assert_eq!(
        neither_file_nor_directory("/dev/null"),
        "Path is neither a file nor a directory: /dev/null"
    );
}
