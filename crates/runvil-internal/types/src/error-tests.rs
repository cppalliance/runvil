//! Tests for `ErrorCode` numbering, `Location` ranges, and the cause chain
//! `with_cause` appends.

use std::collections::BTreeSet;
use std::fmt;
use std::num::NonZeroU32;

use super::{Error, ErrorCode, Location};

/// A cause with an optional cause of its own.
#[derive(Debug)]
struct Layer(&'static str, Option<Box<Layer>>);

impl fmt::Display for Layer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for Layer {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.1
            .as_deref()
            .map(|layer| layer as &(dyn std::error::Error + 'static))
    }
}

#[test]
fn with_cause_appends_the_chain_to_the_message_and_keeps_the_prose() {
    let cause = Layer("disk gone", Some(Box::new(Layer("sector 9", None))));
    let error = Error::new(ErrorCode::Io, "read failed", "Try again.").with_cause(&cause);
    assert_eq!(error.message, "read failed: disk gone: sector 9");
    assert_eq!(error.prose, "Try again.");
}

#[test]
fn with_cause_skips_text_the_message_already_holds() {
    let cause = Layer("disk gone", Some(Box::new(Layer("", None))));
    let error =
        Error::new(ErrorCode::Io, "read failed: disk gone", "Try again.").with_cause(&cause);
    assert_eq!(error.message, "read failed: disk gone");
}

#[test]
fn with_cause_adds_only_what_follows_text_the_message_ends_with() {
    let inner = Error::new(ErrorCode::ToolTransport, "request failed", "request failed")
        .with_cause(&Layer("refused", None));
    let error = Error::new(
        ErrorCode::ToolTransport,
        "search: request failed",
        "Try again.",
    )
    .with_cause(&inner);
    assert_eq!(error.message, "search: request failed: refused");

    let unrelated = Error::new(ErrorCode::Io, "read failed", "Try again.")
        .with_cause(&Layer("disk gone: sector 9", None));
    assert_eq!(unrelated.message, "read failed: disk gone: sector 9");
}

/// Every code, in declaration order.
const ALL: [ErrorCode; 68] = [
    ErrorCode::FrontmatterInvalid,
    ErrorCode::StructureInvalid,
    ErrorCode::FenceInvalid,
    ErrorCode::ListEmptyItem,
    ErrorCode::UnsupportedVersion,
    ErrorCode::InvalidValue,
    ErrorCode::LuaRuntime,
    ErrorCode::LuaCompile,
    ErrorCode::LuaQuota,
    ErrorCode::CallDepthExceeded,
    ErrorCode::Substitution,
    ErrorCode::TasksLive,
    ErrorCode::TaskNotOwned,
    ErrorCode::TaskConsumed,
    ErrorCode::TaskCancelled,
    ErrorCode::ContextOverflow,
    ErrorCode::RateLimited,
    ErrorCode::QuotaExhausted,
    ErrorCode::Overloaded,
    ErrorCode::Refused,
    ErrorCode::Timeout,
    ErrorCode::Transport,
    ErrorCode::ServerError,
    ErrorCode::Rejected,
    ErrorCode::MalformedResponse,
    ErrorCode::EmptyReply,
    ErrorCode::Unavailable,
    ErrorCode::InvalidParts,
    ErrorCode::ContextExhausted,
    ErrorCode::ModelRequired,
    ErrorCode::ToolInvalidArguments,
    ErrorCode::ToolBackend,
    ErrorCode::ToolTransport,
    ErrorCode::ToolOther,
    ErrorCode::OutOfScopeTool,
    ErrorCode::UnboundTool,
    ErrorCode::ModelOnlyTool,
    ErrorCode::ToolLoopExhausted,
    ErrorCode::ToolSchemaInvalid,
    ErrorCode::ToolSetsInvalid,
    ErrorCode::NotFound,
    ErrorCode::AlreadyExists,
    ErrorCode::NotADirectory,
    ErrorCode::IsADirectory,
    ErrorCode::DirectoryNotEmpty,
    ErrorCode::NotUtf8,
    ErrorCode::InvalidPath,
    ErrorCode::InvalidRange,
    ErrorCode::Anchor,
    ErrorCode::PermissionDenied,
    ErrorCode::ApprovalRequired,
    ErrorCode::ScopeEnded,
    ErrorCode::ReadOnly,
    ErrorCode::OutsideRoot,
    ErrorCode::RootProtected,
    ErrorCode::NoStore,
    ErrorCode::CrossMount,
    ErrorCode::Io,
    ErrorCode::Unsupported,
    ErrorCode::Conflict,
    ErrorCode::Backend,
    ErrorCode::Determinism,
    ErrorCode::RequirementsUnmet,
    ErrorCode::Interrupted,
    ErrorCode::Internal,
    ErrorCode::RecorderFailed,
    ErrorCode::PluginLoadFailed,
    ErrorCode::NoOutput,
];

/// The family hundred of each code. The match is exhaustive, so a new code
/// fails to build here until it is placed in a family; add it to `ALL` too.
fn family(code: ErrorCode) -> u16 {
    match code {
        ErrorCode::FrontmatterInvalid
        | ErrorCode::StructureInvalid
        | ErrorCode::FenceInvalid
        | ErrorCode::ListEmptyItem
        | ErrorCode::UnsupportedVersion
        | ErrorCode::InvalidValue => 1,
        ErrorCode::LuaRuntime
        | ErrorCode::LuaCompile
        | ErrorCode::LuaQuota
        | ErrorCode::CallDepthExceeded
        | ErrorCode::Substitution => 2,
        ErrorCode::TasksLive
        | ErrorCode::TaskNotOwned
        | ErrorCode::TaskConsumed
        | ErrorCode::TaskCancelled => 3,
        ErrorCode::ContextOverflow
        | ErrorCode::RateLimited
        | ErrorCode::QuotaExhausted
        | ErrorCode::Overloaded
        | ErrorCode::Refused
        | ErrorCode::Timeout
        | ErrorCode::Transport
        | ErrorCode::ServerError
        | ErrorCode::Rejected
        | ErrorCode::MalformedResponse
        | ErrorCode::EmptyReply
        | ErrorCode::Unavailable
        | ErrorCode::InvalidParts
        | ErrorCode::ContextExhausted
        | ErrorCode::ModelRequired => 4,
        ErrorCode::ToolInvalidArguments
        | ErrorCode::ToolBackend
        | ErrorCode::ToolTransport
        | ErrorCode::ToolOther
        | ErrorCode::OutOfScopeTool
        | ErrorCode::UnboundTool
        | ErrorCode::ModelOnlyTool
        | ErrorCode::ToolLoopExhausted
        | ErrorCode::ToolSchemaInvalid
        | ErrorCode::ToolSetsInvalid => 5,
        ErrorCode::NotFound
        | ErrorCode::AlreadyExists
        | ErrorCode::NotADirectory
        | ErrorCode::IsADirectory
        | ErrorCode::DirectoryNotEmpty
        | ErrorCode::NotUtf8
        | ErrorCode::InvalidPath
        | ErrorCode::InvalidRange
        | ErrorCode::Anchor
        | ErrorCode::PermissionDenied
        | ErrorCode::ApprovalRequired
        | ErrorCode::ScopeEnded
        | ErrorCode::ReadOnly
        | ErrorCode::OutsideRoot
        | ErrorCode::RootProtected
        | ErrorCode::NoStore
        | ErrorCode::CrossMount
        | ErrorCode::Io
        | ErrorCode::Unsupported
        | ErrorCode::Conflict
        | ErrorCode::Backend
        | ErrorCode::Determinism => 6,
        ErrorCode::RequirementsUnmet | ErrorCode::Interrupted | ErrorCode::Internal => 7,
        ErrorCode::RecorderFailed | ErrorCode::PluginLoadFailed | ErrorCode::NoOutput => 8,
    }
}

fn nonzero(n: u32) -> NonZeroU32 {
    NonZeroU32::new(n).expect("tests pass non-zero numbers")
}

#[test]
fn every_code_is_listed_once() {
    let numbers: BTreeSet<u16> = ALL.into_iter().map(ErrorCode::number).collect();
    assert_eq!(numbers.len(), ALL.len());
}

#[test]
fn every_code_number_falls_in_its_family_hundred() {
    for code in ALL {
        assert_eq!(code.number() / 100, family(code), "{code:?}");
    }
}

#[test]
fn every_code_has_its_own_snake_case_tag() {
    let tags: BTreeSet<&str> = ALL.into_iter().map(ErrorCode::tag).collect();
    assert_eq!(tags.len(), ALL.len(), "no two codes share a tag");
    for tag in tags {
        assert!(
            tag.bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'),
            "{tag}"
        );
    }
}

#[test]
fn store_and_model_codes_keep_the_tags_stored_records_hold() {
    // The store and model tags a run log already holds.
    for (code, tag) in [
        (ErrorCode::NotFound, "not_found"),
        (ErrorCode::AlreadyExists, "already_exists"),
        (ErrorCode::NotADirectory, "not_a_directory"),
        (ErrorCode::IsADirectory, "is_a_directory"),
        (ErrorCode::DirectoryNotEmpty, "directory_not_empty"),
        (ErrorCode::NotUtf8, "not_utf8"),
        (ErrorCode::InvalidPath, "invalid_path"),
        (ErrorCode::InvalidRange, "invalid_range"),
        (ErrorCode::Anchor, "anchor"),
        (ErrorCode::PermissionDenied, "permission_denied"),
        (ErrorCode::ApprovalRequired, "approval_required"),
        (ErrorCode::ScopeEnded, "scope_ended"),
        (ErrorCode::ReadOnly, "read_only"),
        (ErrorCode::OutsideRoot, "outside_root"),
        (ErrorCode::RootProtected, "root_protected"),
        (ErrorCode::NoStore, "no_store"),
        (ErrorCode::CrossMount, "cross_mount"),
        (ErrorCode::Io, "io"),
        (ErrorCode::Unsupported, "unsupported"),
        (ErrorCode::Conflict, "conflict"),
        (ErrorCode::Backend, "backend"),
        (ErrorCode::ContextOverflow, "context_overflow"),
        (ErrorCode::RateLimited, "rate_limited"),
        (ErrorCode::QuotaExhausted, "quota_exhausted"),
        (ErrorCode::Overloaded, "overloaded"),
        (ErrorCode::Refused, "refused"),
        (ErrorCode::Timeout, "timeout"),
        (ErrorCode::Transport, "transport"),
        (ErrorCode::ServerError, "server_error"),
        (ErrorCode::Rejected, "rejected"),
        (ErrorCode::MalformedResponse, "malformed_response"),
        (ErrorCode::EmptyReply, "empty_reply"),
        (ErrorCode::Unavailable, "unavailable"),
        (ErrorCode::InvalidParts, "invalid_parts"),
    ] {
        assert_eq!(code.tag(), tag);
    }
}

#[test]
fn every_code_has_default_prose_in_whole_sentences() {
    for code in ALL {
        let prose = code.prose();
        assert!(!prose.is_empty(), "{code:?}");
        assert!(prose.ends_with('.'), "{code:?}: {prose}");
    }
}

#[test]
fn a_location_cannot_end_before_it_starts() {
    let at =
        |sl, sc, el, ec| Location::new("a.md", nonzero(sl), nonzero(sc), nonzero(el), nonzero(ec));
    assert!(at(2, 5, 2, 4).is_none());
    assert!(at(2, 5, 1, 9).is_none());
    assert!(at(2, 5, 2, 5).is_some());
    assert!(at(1, 9, 2, 1).is_some());
}

#[test]
fn a_line_location_covers_the_whole_line() {
    let line = Location::line("a.md", nonzero(7));
    assert_eq!(
        (line.start_line, line.start_column),
        (nonzero(7), nonzero(1))
    );
    assert_eq!((line.end_line, line.end_column), (nonzero(8), nonzero(1)));
    let last = Location::line("a.md", NonZeroU32::MAX);
    assert_eq!(
        (last.end_line, last.end_column),
        (NonZeroU32::MAX, nonzero(1))
    );
}
