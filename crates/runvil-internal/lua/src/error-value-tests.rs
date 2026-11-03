//! Tests for the error value's kind vocabulary: its tags parse back, and a
//! rejected tag's error names every accepted one. Also the universal error a
//! store failure becomes: its code, its facts, and its texts; and the
//! message a table rendered from a universal error holds.

use std::num::NonZeroU32;

use runvil_types::error::{Error, ErrorCode, FactKey, place};
use runvil_vfs::{PathReason, VfsError};
use serde_json::Value;

use super::{ErrorKind, error_table, store_error, store_error_value_fields};
use crate::error::{internal, lua_runtime};

#[test]
fn a_tables_message_puts_a_prompt_location_and_never_a_rust_one_before_the_prose() {
    let lua = mlua::Lua::new();
    let message = |error: &Error| -> String {
        error_table(&lua, error)
            .expect("the table builds")
            .raw_get("message")
            .expect("the message reads")
    };
    let unlocated = lua_runtime("boom");
    assert_eq!(message(&unlocated), unlocated.prose);
    let located = place(unlocated.clone(), "agents/a.md", NonZeroU32::new(7));
    assert_eq!(
        message(&located),
        format!("agents/a.md:7: {}", located.prose)
    );
    let fault = internal("a test invariant broke");
    assert!(fault.at.is_some(), "an internal failure is located in Rust");
    assert_eq!(
        message(&fault),
        fault.prose,
        "an internal failure's text shows no coordinates"
    );
}

#[test]
fn every_error_kind_tag_parses_back_to_its_kind() {
    for kind in ErrorKind::ALL {
        assert_eq!(kind.tag().parse::<ErrorKind>(), Ok(*kind));
    }
}

#[test]
fn an_unknown_error_kind_names_the_text_and_every_accepted_tag() {
    let error = "stoer"
        .parse::<ErrorKind>()
        .expect_err("a misspelled tag is not a kind");
    assert_eq!(error.code, ErrorCode::InvalidValue);
    assert_eq!(error.facts.get(&FactKey::Name), Some(&Value::from("stoer")));
    assert_eq!(error.prose, error.message);
    let message = error.to_string();
    assert!(message.contains("`stoer`"), "the rejected text: {message}");
    for kind in ErrorKind::ALL {
        assert!(
            message.contains(&format!("`{}`", kind.tag())),
            "required every accepted tag, actual {message:?} without `{}`",
            kind.tag()
        );
    }
}

/// Every store failure beside the code named after its variant.
#[expect(
    clippy::too_many_lines,
    reason = "one table holds every variant beside its code"
)]
pub(super) fn every_store_failure() -> Vec<(VfsError, ErrorCode)> {
    let path = || "a.md".to_owned();
    let text = || "why".to_owned();
    vec![
        (VfsError::NotFound { path: path() }, ErrorCode::NotFound),
        (
            VfsError::AlreadyExists { path: path() },
            ErrorCode::AlreadyExists,
        ),
        (
            VfsError::NotADirectory { path: path() },
            ErrorCode::NotADirectory,
        ),
        (
            VfsError::IsADirectory { path: path() },
            ErrorCode::IsADirectory,
        ),
        (
            VfsError::DirectoryNotEmpty { path: path() },
            ErrorCode::DirectoryNotEmpty,
        ),
        (
            VfsError::NotUtf8 {
                path: path(),
                valid_up_to: 3,
            },
            ErrorCode::NotUtf8,
        ),
        (
            VfsError::InvalidPath {
                path: path(),
                reason: PathReason::Traversal,
            },
            ErrorCode::InvalidPath,
        ),
        (
            VfsError::InvalidRange {
                path: path(),
                reason: "backwards",
            },
            ErrorCode::InvalidRange,
        ),
        (
            VfsError::Anchor {
                path: path(),
                anchor: text(),
                count: 2,
            },
            ErrorCode::Anchor,
        ),
        (
            VfsError::PermissionDenied {
                path: path(),
                reason: text(),
            },
            ErrorCode::PermissionDenied,
        ),
        (
            VfsError::ApprovalRequired {
                path: path(),
                reason: text(),
            },
            ErrorCode::ApprovalRequired,
        ),
        (VfsError::ScopeEnded { path: path() }, ErrorCode::ScopeEnded),
        (VfsError::ReadOnly { path: path() }, ErrorCode::ReadOnly),
        (
            VfsError::OutsideRoot {
                path: path(),
                detail: text(),
            },
            ErrorCode::OutsideRoot,
        ),
        (
            VfsError::RootProtected { path: path() },
            ErrorCode::RootProtected,
        ),
        (VfsError::NoStore, ErrorCode::NoStore),
        (
            VfsError::CrossMount {
                path: path(),
                detail: text(),
            },
            ErrorCode::CrossMount,
        ),
        (
            VfsError::Io {
                path: path(),
                kind: std::io::ErrorKind::Other,
                raw_os_error: None,
                message: text(),
            },
            ErrorCode::Io,
        ),
        (
            VfsError::Unsupported {
                path: path(),
                detail: text(),
            },
            ErrorCode::Unsupported,
        ),
        (
            VfsError::Conflict {
                path: path(),
                detail: text(),
            },
            ErrorCode::Conflict,
        ),
        (VfsError::Backend { message: text() }, ErrorCode::Backend),
    ]
}

#[test]
fn a_store_failure_takes_the_code_named_after_its_variant() {
    for (error, code) in every_store_failure() {
        let converted = store_error(&error);
        assert_eq!(converted.code, code, "{error:?}");
        assert_eq!(converted.message, error.to_string(), "{error:?}");
        assert_eq!(converted.prose, code.prose(), "{error:?}");
        assert_eq!(converted.at, None, "{error:?}");
    }
}

#[test]
fn a_store_failure_keeps_the_lua_table_fields_as_facts() {
    let anchor = store_error(&VfsError::Anchor {
        path: "a.md".to_owned(),
        anchor: "x".to_owned(),
        count: 2,
    });
    assert_eq!(anchor.facts.get(&FactKey::Path), Some(&Value::from("a.md")));
    assert_eq!(anchor.facts.get(&FactKey::Anchor), Some(&Value::from("x")));
    assert_eq!(anchor.facts.get(&FactKey::Count), Some(&Value::from(2)));
    let invalid = store_error(&VfsError::InvalidPath {
        path: "../a".to_owned(),
        reason: PathReason::Traversal,
    });
    assert_eq!(
        invalid.facts.get(&FactKey::Rule),
        Some(&Value::from("traversal"))
    );
    let not_utf8 = store_error(&VfsError::NotUtf8 {
        path: "a.bin".to_owned(),
        valid_up_to: 3,
    });
    assert_eq!(
        not_utf8.facts.get(&FactKey::ValidUpTo),
        Some(&Value::from(3))
    );
    for error in [
        VfsError::Anchor {
            path: "a.md".to_owned(),
            anchor: "x".to_owned(),
            count: 2,
        },
        VfsError::NoStore,
        VfsError::Backend {
            message: "down".to_owned(),
        },
    ] {
        // Every Lua table field but `reason`, which the code names.
        let fields = store_error_value_fields(&error).len() - 1;
        assert_eq!(store_error(&error).facts.len(), fields, "{error:?}");
    }
}
