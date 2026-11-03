//! Tests for `ErrorCode` numbering and `Location` ranges.

use std::collections::BTreeSet;
use std::num::NonZeroU32;

use super::{ErrorCode, Location};

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
