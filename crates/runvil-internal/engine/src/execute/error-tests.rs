//! The stable tag of each run-error kind: the variant's name, which parses
//! back to the kind. The kind each failure code classifies as, and the
//! location a failed run's message ends with.

use std::collections::BTreeSet;
use std::num::NonZeroU32;

use runvil_types::error::{Error, ErrorCode, FactKey, Location};

use super::{RunErrorKind, failure_message, outcome, run_error_kind};
use crate::execute::RunOutcome;
use crate::prompt::Prompt;

/// Every kind, in declaration order.
const KINDS: [RunErrorKind; 10] = [
    RunErrorKind::Parse,
    RunErrorKind::Binding,
    RunErrorKind::Completion,
    RunErrorKind::Tool,
    RunErrorKind::Vfs,
    RunErrorKind::Lua,
    RunErrorKind::Quota,
    RunErrorKind::ContextExhausted,
    RunErrorKind::Internal,
    RunErrorKind::RequirementsUnmet,
];

#[test]
fn each_kinds_tag_is_its_debug_name_and_parses_back_to_the_kind() {
    for kind in KINDS {
        assert_eq!(kind.as_str(), format!("{kind:?}"));
        assert_eq!(RunErrorKind::from_tag(kind.as_str()), Some(kind));
    }
}

#[test]
fn a_retired_tag_parses_to_the_kind_that_absorbed_it_and_no_kind_writes_it() {
    for (tag, kind) in [
        ("Version", RunErrorKind::Parse),
        ("Determinism", RunErrorKind::Vfs),
    ] {
        assert_eq!(RunErrorKind::from_tag(tag), Some(kind), "{tag:?}");
        assert!(
            KINDS.iter().all(|written| written.as_str() != tag),
            "{tag:?}"
        );
    }
}

#[test]
fn a_tag_no_kind_has_parses_to_none() {
    for tag in ["Input", "parse", "", "Internal "] {
        assert_eq!(RunErrorKind::from_tag(tag), None, "{tag:?}");
    }
}

/// Every code beside the kind a run it ends reports, which is the kind the
/// failure it stands for always classified as.
const CODE_KINDS: [(ErrorCode, RunErrorKind); 68] = [
    (ErrorCode::FrontmatterInvalid, RunErrorKind::Parse),
    (ErrorCode::StructureInvalid, RunErrorKind::Parse),
    (ErrorCode::FenceInvalid, RunErrorKind::Parse),
    (ErrorCode::ListEmptyItem, RunErrorKind::Parse),
    (ErrorCode::UnsupportedVersion, RunErrorKind::Parse),
    (ErrorCode::InvalidValue, RunErrorKind::Parse),
    (ErrorCode::LuaRuntime, RunErrorKind::Lua),
    (ErrorCode::LuaCompile, RunErrorKind::Lua),
    (ErrorCode::LuaQuota, RunErrorKind::Quota),
    (ErrorCode::CallDepthExceeded, RunErrorKind::Quota),
    (ErrorCode::Substitution, RunErrorKind::Lua),
    (ErrorCode::TasksLive, RunErrorKind::Lua),
    (ErrorCode::TaskNotOwned, RunErrorKind::Lua),
    (ErrorCode::TaskConsumed, RunErrorKind::Lua),
    (ErrorCode::TaskCanceled, RunErrorKind::Lua),
    (ErrorCode::ContextOverflow, RunErrorKind::Completion),
    (ErrorCode::RateLimited, RunErrorKind::Completion),
    (ErrorCode::QuotaExhausted, RunErrorKind::Completion),
    (ErrorCode::Overloaded, RunErrorKind::Completion),
    (ErrorCode::Refused, RunErrorKind::Completion),
    (ErrorCode::Timeout, RunErrorKind::Completion),
    (ErrorCode::Transport, RunErrorKind::Completion),
    (ErrorCode::ServerError, RunErrorKind::Completion),
    (ErrorCode::Rejected, RunErrorKind::Completion),
    (ErrorCode::MalformedResponse, RunErrorKind::Completion),
    (ErrorCode::EmptyReply, RunErrorKind::Completion),
    (ErrorCode::Unavailable, RunErrorKind::Completion),
    (ErrorCode::InvalidParts, RunErrorKind::Completion),
    (ErrorCode::ContextExhausted, RunErrorKind::ContextExhausted),
    (ErrorCode::ModelRequired, RunErrorKind::RequirementsUnmet),
    (ErrorCode::ToolInvalidArguments, RunErrorKind::Tool),
    (ErrorCode::ToolBackend, RunErrorKind::Tool),
    (ErrorCode::ToolTransport, RunErrorKind::Tool),
    (ErrorCode::ToolOther, RunErrorKind::Tool),
    (ErrorCode::OutOfScopeTool, RunErrorKind::Tool),
    (ErrorCode::UnboundTool, RunErrorKind::Tool),
    (ErrorCode::ModelOnlyTool, RunErrorKind::Tool),
    (ErrorCode::ToolLoopExhausted, RunErrorKind::Tool),
    (ErrorCode::ToolSchemaInvalid, RunErrorKind::Binding),
    (ErrorCode::ToolSetsInvalid, RunErrorKind::Binding),
    (ErrorCode::NotFound, RunErrorKind::Vfs),
    (ErrorCode::AlreadyExists, RunErrorKind::Vfs),
    (ErrorCode::NotADirectory, RunErrorKind::Vfs),
    (ErrorCode::IsADirectory, RunErrorKind::Vfs),
    (ErrorCode::DirectoryNotEmpty, RunErrorKind::Vfs),
    (ErrorCode::NotUtf8, RunErrorKind::Vfs),
    (ErrorCode::InvalidPath, RunErrorKind::Vfs),
    (ErrorCode::InvalidRange, RunErrorKind::Vfs),
    (ErrorCode::Anchor, RunErrorKind::Vfs),
    (ErrorCode::PermissionDenied, RunErrorKind::Vfs),
    (ErrorCode::ApprovalRequired, RunErrorKind::Vfs),
    (ErrorCode::ScopeEnded, RunErrorKind::Vfs),
    (ErrorCode::ReadOnly, RunErrorKind::Vfs),
    (ErrorCode::OutsideRoot, RunErrorKind::Vfs),
    (ErrorCode::RootProtected, RunErrorKind::Vfs),
    (ErrorCode::NoStore, RunErrorKind::Vfs),
    (ErrorCode::CrossMount, RunErrorKind::Vfs),
    (ErrorCode::Io, RunErrorKind::Vfs),
    (ErrorCode::Unsupported, RunErrorKind::Vfs),
    (ErrorCode::Conflict, RunErrorKind::Vfs),
    (ErrorCode::Backend, RunErrorKind::Vfs),
    (ErrorCode::Determinism, RunErrorKind::Vfs),
    (
        ErrorCode::RequirementsUnmet,
        RunErrorKind::RequirementsUnmet,
    ),
    (ErrorCode::Canceled, RunErrorKind::Internal),
    (ErrorCode::Internal, RunErrorKind::Internal),
    (ErrorCode::RecorderFailed, RunErrorKind::Internal),
    (ErrorCode::PluginLoadFailed, RunErrorKind::Internal),
    (ErrorCode::NoOutput, RunErrorKind::Internal),
];

#[test]
fn every_code_classifies_as_the_kind_its_failure_always_had() {
    let numbers: BTreeSet<u16> = CODE_KINDS.iter().map(|(code, _)| code.number()).collect();
    assert_eq!(numbers.len(), CODE_KINDS.len(), "each code is listed once");
    for (code, kind) in CODE_KINDS {
        assert_eq!(run_error_kind(code), kind, "{code:?}");
        assert_eq!(ErrorCode::from_tag(code.tag()), Some(code), "{code:?}");
    }
}

/// A failure with `code` and `message`.
fn failure(code: ErrorCode, message: &str) -> Error {
    Error::new(code, message, code.prose())
}

/// The line or column `n`.
fn at(n: u32) -> NonZeroU32 {
    NonZeroU32::new(n).expect("tests pass non-zero positions")
}

#[test]
fn a_parse_failure_message_ends_with_the_prompt_name_and_the_start_position() {
    let mut error = failure(ErrorCode::StructureInvalid, "duplicate section");
    assert_eq!(failure_message(&error), "duplicate section");
    error.facts.insert(FactKey::Name, "greeter".into());
    assert_eq!(
        failure_message(&error),
        "duplicate section (prompt greeter)"
    );
    error.at = Location::new("agents/greeter.md", at(3), at(7), at(4), at(1));
    assert_eq!(
        failure_message(&error),
        "duplicate section (prompt greeter, line 3, column 7)",
        "the location names the start line and column, not the end"
    );
    error.facts.clear();
    assert_eq!(
        failure_message(&error),
        "duplicate section (line 3, column 7)"
    );
    assert_eq!(
        outcome(&error),
        RunOutcome::Failed {
            kind: RunErrorKind::Parse,
            message: "duplicate section (line 3, column 7)".to_owned(),
        }
    );
}

#[test]
fn a_parsed_prompts_failure_message_ends_with_its_name_and_position() {
    let frontmatter = concat!(
        "---\n",
        "name: x\n",
        "description: d\n",
        "uses:\n",
        "  - not a Pack id\n",
        "---\n",
        "\n# T\n\n## S\n\np\n",
    );
    let refused = Prompt::parse(frontmatter, "prompt", "test")
        .0
        .expect_err("a Pack id with spaces must be rejected");
    assert_eq!(run_error_kind(refused.code), RunErrorKind::Parse);
    assert_eq!(
        failure_message(&refused),
        format!("{} (line 5, column 5)", refused.message),
        "a frontmatter failure predates the prompt's name"
    );
    let duplicate = "---\nname: dup\ndescription: d\n---\n\n# T\n\n## S\n\np\n\n## S\n\nq\n";
    let refused = Prompt::parse(duplicate, "prompt", "test")
        .0
        .expect_err("duplicate sibling sections must be rejected");
    assert_eq!(
        failure_message(&refused),
        format!("{} (prompt dup, line 12, column 1)", refused.message)
    );
}

#[test]
fn a_refused_uses_list_ends_with_its_line_and_never_names_the_pack_as_the_prompt() {
    let source = "---\nname: x\ndescription: d\nuses: [web, web]\n---\n\n# T\n";
    let refused = Prompt::parse(source, "prompt", "test")
        .0
        .expect_err("a Pack id declared twice must be refused");
    assert_eq!(
        refused.facts.get(&FactKey::Name),
        Some(&serde_json::Value::from("web")),
        "the refused Pack's id is the failure's name"
    );
    assert_eq!(
        outcome(&refused),
        RunOutcome::Failed {
            kind: RunErrorKind::Parse,
            message: "invalid frontmatter: Pack id web is declared more than once under uses \
                      (line 4, column 1)"
                .to_owned(),
        }
    );
}

#[test]
fn a_parse_failure_run_raises_names_the_prompt_and_its_file() {
    let code = ErrorCode::StructureInvalid;
    let message = "not a runvil prompt: no runvil version";
    let stamped = crate::prompt::detail::with_prompt_context(
        crate::prompt::detail::parse_error(code, message),
        "greeter",
        "agents/greeter.md",
    );
    assert_eq!((stamped.code, stamped.message.as_str()), (code, message));
    assert_eq!(stamped.at, None);
    assert_eq!(
        stamped.facts.get(&FactKey::Name),
        Some(&serde_json::Value::from("greeter"))
    );
    assert_eq!(
        stamped.prose,
        format!("agents/greeter.md: {message}. {}", code.prose())
    );
    assert_eq!(
        failure_message(&stamped),
        format!("{message} (prompt greeter)")
    );
}

#[test]
fn an_internal_failure_message_ends_with_its_rust_file_and_line() {
    let mut error = failure(ErrorCode::Internal, "internal invariant violated: x");
    assert_eq!(
        failure_message(&error),
        "internal invariant violated: x",
        "an internal failure with no location ends with its own text"
    );
    error.at = Some(Location::line("src/a.rs", at(12)));
    assert_eq!(
        failure_message(&error),
        "internal invariant violated: x (at src/a.rs:12)"
    );
    error.facts.insert(FactKey::Name, "ignored".into());
    assert_eq!(
        failure_message(&error),
        "internal invariant violated: x (at src/a.rs:12)",
        "an internal failure names no prompt"
    );
}

#[test]
fn any_other_located_failure_ends_with_its_file_and_line() {
    let mut error = failure(
        ErrorCode::LuaCompile,
        "lua compilation error at x (line 2): y",
    );
    error.at = Some(Location::line("agents/a.md", at(2)));
    error.facts.insert(FactKey::Name, "p".into());
    assert_eq!(
        failure_message(&error),
        "lua compilation error at x (line 2): y (at agents/a.md:2)",
        "the suffix adds the file to the line the message names"
    );
    let canceled = Error::canceled();
    assert_eq!(
        failure_message(&canceled),
        canceled.message,
        "a failure with no location ends with its own text"
    );
}

#[test]
fn a_run_time_invalid_value_takes_the_located_form_not_the_parse_form() {
    let mut error = failure(ErrorCode::InvalidValue, "unknown model role `bogus`");
    error.facts.insert(FactKey::Name, "bogus".into());
    assert_eq!(
        failure_message(&error),
        "unknown model role `bogus`",
        "the value its `Name` fact holds is no prompt name"
    );
    error.at = Some(Location::line("agents/a.md", at(9)));
    assert_eq!(
        outcome(&error),
        RunOutcome::Failed {
            kind: RunErrorKind::Parse,
            message: "unknown model role `bogus` (at agents/a.md:9)".to_owned(),
        }
    );
}
