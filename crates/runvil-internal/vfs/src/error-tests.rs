use std::collections::BTreeSet;

use super::{PathReason, VfsError};

/// One of every variant, each with its tag and its display text.
#[expect(
    clippy::too_many_lines,
    reason = "one table holds every variant beside its tag and its text"
)]
fn every_variant() -> Vec<(VfsError, &'static str, &'static str)> {
    let path = || "/x".to_owned();
    vec![
        (
            VfsError::NotFound { path: path() },
            "not_found",
            "not found: /x",
        ),
        (
            VfsError::AlreadyExists { path: path() },
            "already_exists",
            "already exists: /x",
        ),
        (
            VfsError::NotADirectory { path: path() },
            "not_a_directory",
            "not a directory: /x",
        ),
        (
            VfsError::IsADirectory { path: path() },
            "is_a_directory",
            "is a directory: /x",
        ),
        (
            VfsError::DirectoryNotEmpty { path: path() },
            "directory_not_empty",
            "directory not empty: /x",
        ),
        (
            VfsError::NotUtf8 {
                path: path(),
                valid_up_to: 3,
            },
            "not_utf8",
            "not UTF-8: /x, valid up to byte 3",
        ),
        (
            VfsError::InvalidPath {
                path: String::new(),
                reason: PathReason::Empty,
            },
            "invalid_path",
            "invalid path \"\": path is empty",
        ),
        (
            VfsError::InvalidRange {
                path: path(),
                reason: "start is below 1",
            },
            "invalid_range",
            "invalid line range for /x: start is below 1",
        ),
        (
            VfsError::Anchor {
                path: path(),
                anchor: "TODO".to_owned(),
                count: 2,
            },
            "anchor",
            "anchor \"TODO\" occurs 2 times in /x, expected exactly one",
        ),
        (
            VfsError::PermissionDenied {
                path: path(),
                reason: "denied".to_owned(),
            },
            "permission_denied",
            "permission denied: denied",
        ),
        (
            VfsError::ApprovalRequired {
                path: path(),
                reason: "ask".to_owned(),
            },
            "approval_required",
            "approval required: ask",
        ),
        (
            VfsError::ScopeEnded { path: path() },
            "scope_ended",
            "access ended: the run or tool call that owned this access has ended, \
             so /x can no longer be touched through it",
        ),
        (
            VfsError::ReadOnly { path: path() },
            "read_only",
            "read-only: /x is served read-only and cannot change",
        ),
        (
            VfsError::OutsideRoot {
                path: path(),
                detail: "/x escapes the mounted root".to_owned(),
            },
            "outside_root",
            "outside the mounted root: /x escapes the mounted root",
        ),
        (
            VfsError::RootProtected {
                path: "/".to_owned(),
            },
            "root_protected",
            "root protected: the mounted root / cannot be removed, renamed, or replaced",
        ),
        (
            VfsError::NoStore,
            "no_store",
            "no store: the handle declares no store",
        ),
        (
            VfsError::CrossMount {
                path: path(),
                detail: "rename of /x to /y".to_owned(),
            },
            "cross_mount",
            "across mounts: rename of /x to /y",
        ),
        (
            VfsError::Io {
                path: path(),
                kind: std::io::ErrorKind::PermissionDenied,
                raw_os_error: Some(13),
                message: "access denied".to_owned(),
            },
            "io",
            "I/O failure on /x: access denied",
        ),
        (
            VfsError::Unsupported {
                path: path(),
                detail: "no".to_owned(),
            },
            "unsupported",
            "unsupported operation: no",
        ),
        (
            VfsError::Conflict {
                path: path(),
                detail: "d".to_owned(),
            },
            "conflict",
            "conflicting claim: d",
        ),
        (
            VfsError::Backend {
                message: "m".to_owned(),
            },
            "backend",
            "backend failure: m",
        ),
    ]
}

#[test]
fn the_derives_hold_as_before() {
    // Clone, PartialEq, Eq, and Debug stay available on the struct
    // variants: backends compare and clone errors.
    let error = VfsError::NotFound {
        path: "/x".to_owned(),
    };
    let clone = error.clone();
    assert_eq!(error, clone);
    assert!(format!("{error:?}").contains("NotFound"));
    assert_eq!(
        VfsError::InvalidPath {
            path: String::new(),
            reason: PathReason::Empty,
        },
        VfsError::InvalidPath {
            path: String::new(),
            reason: PathReason::Empty,
        }
    );
}

#[test]
fn every_variant_displays_with_its_own_prefix() {
    for (error, _, text) in every_variant() {
        assert_eq!(error.to_string(), text);
    }
}

#[test]
fn every_variant_has_its_own_stable_tag() {
    let variants = every_variant();
    for (error, tag, _) in &variants {
        assert_eq!(error.tag(), *tag, "{error:?}");
    }
    let distinct: BTreeSet<&str> = variants.iter().map(|(error, ..)| error.tag()).collect();
    assert_eq!(
        distinct.len(),
        variants.len(),
        "no two variants share a tag"
    );
}

#[test]
fn every_path_reason_has_its_own_stable_tag() {
    let reasons = [
        (PathReason::Empty, "empty"),
        (PathReason::Absolute, "absolute"),
        (PathReason::Traversal, "traversal"),
        (PathReason::Control, "control"),
        (PathReason::EmptySegment, "empty_segment"),
        (PathReason::Backslash, "backslash"),
        (PathReason::ReservedName, "reserved_name"),
        (PathReason::UnsafeSuffix, "unsafe_suffix"),
        (PathReason::TooLong, "too_long"),
        (PathReason::Wildcard, "wildcard"),
        (PathReason::IntoDescendant, "into_descendant"),
    ];
    for (reason, tag) in reasons {
        assert_eq!(reason.tag(), tag, "{reason:?}");
    }
    let distinct: BTreeSet<&str> = reasons.iter().map(|(reason, _)| reason.tag()).collect();
    assert_eq!(distinct.len(), reasons.len(), "no two reasons share a tag");
}
