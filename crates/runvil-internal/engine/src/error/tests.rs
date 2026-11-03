//! Tests for the internal error: its source chains through the public
//! wrappers, its bridges from other crates' errors, and the kinds and
//! messages, locations included, it ends a run with.

use runvil_lua::Error as LuaError;

use super::*;
use crate::prompt::Prompt;

#[path = "tests-chain.rs"]
mod chain;

#[test]
fn context_exhaustion_maps_from_lua_and_classifies() {
    // The compactor's typed exhaustion crosses the crate seam
    // variant-for-variant and classifies as its own run-error kind, so a
    // caller can distinguish context exhaustion from a transport failure.
    for (reason, message) in [
        (
            "provider",
            "context exhausted: the provider rejected the request as exceeding the model's context window",
        ),
        ("manual", "context exhausted: manual"),
    ] {
        let lua_error = LuaError::ContextExhausted {
            reason: reason.to_owned(),
        };
        let error = Error::from(lua_error);
        assert!(
            matches!(&error, Error::ContextExhausted { reason: kept, .. } if kept == reason),
            "the mapping preserves the reason, got {error:?}"
        );
        assert!(
            error.originating_error().is_none(),
            "a variant with no originating error has no cause"
        );
        assert_eq!(error.to_string(), message);
        assert_eq!(error.kind(), crate::RunErrorKind::ContextExhausted);
        assert_eq!(error.message(), message);
    }
}

#[test]
fn a_schema_build_failure_renders_its_refusal_and_keeps_it_as_the_cause() {
    // The binding failure renders the schema refusal in its message once
    // and keeps the typed refusal as its cause.
    let schema_error =
        runvil_model_client::detail::tool_schema_new("echo", "", serde_json::json!([]))
            .expect_err("an array is not an object schema");
    let bind = Error::BindSchema {
        alias: "echo".to_owned(),
        cause: Box::new(schema_error),
    };
    assert_eq!(
        bind.to_string(),
        "model-facing schema build failure for tool alias \"echo\": tool \"echo\" parameters \
         schema must be a JSON object"
    );
    assert_eq!(bind.message(), bind.to_string());
    let cause = bind
        .originating_error()
        .and_then(|cause| cause.downcast_ref::<runvil_types::error::Error>())
        .expect("the schema refusal is the cause");
    assert_eq!(
        cause.code,
        runvil_types::error::ErrorCode::ToolSchemaInvalid
    );
}

#[test]
fn lua_compile_keeps_the_originating_compiler_error_as_its_cause() {
    // A compile failure keeps the concrete `mlua` error as its cause
    // instead of flattening it into `message` alone.
    let compile = Error::LuaCompile {
        location: "section `S` prologue".to_owned(),
        source_line: 7,
        lua_source: "x =".to_owned(),
        message: "syntax error near '='".to_owned(),
        cause: Box::new(mlua::Error::SyntaxError {
            message: "syntax error near '='".to_owned(),
            incomplete_input: false,
        }),
    };
    let cause = compile
        .originating_error()
        .and_then(|cause| cause.downcast_ref::<mlua::Error>())
        .expect("the compiler error is the cause");
    assert!(matches!(cause, mlua::Error::SyntaxError { .. }));
}

#[test]
fn typed_error_survives_the_lua_external_boundary() {
    // Passing the typed error (not its `to_string()`) to
    // `mlua::Error::external` keeps the original error as a downcastable
    // source across the Lua boundary, rather than flattening it to text.
    let original = Error::OutOfScopeToolCall {
        name: "echo".to_owned(),
        global_exists: false,
        has_dynamic_set: false,
        static_aliases: vec!["other".to_owned()],
    };
    let display = original.to_string();
    let external = mlua::Error::external(original);
    match &external {
        mlua::Error::ExternalError(cause) => {
            let recovered = cause
                .downcast_ref::<Error>()
                .expect("the original typed Error is preserved, not stringified");
            assert_eq!(recovered.to_string(), display);
        }
        other => panic!("expected an ExternalError holding the typed error, got {other:?}"),
    }
    // Re-wrapping through the crate's Lua boundary keeps the cause reachable.
    let wrapped = Error::lua(external);
    assert!(wrapped.originating_error().is_some());
}

#[test]
fn completion_errors_preserve_their_causes_across_the_error_type_bridge() {
    // A broker's failure arrives with its concrete cause in its message;
    // the mapping onto this crate's error type keeps the failure whole, so
    // the cause stays in the message and out of the prose.
    use runvil_types::error::ErrorCode;

    let cause = std::io::Error::other("connection refused");
    let completion = crate::model::completion_error(
        ErrorCode::Unavailable,
        "model access is turned off or not configured",
    )
    .with_cause(&cause);
    let bridged = Error::from(completion);
    let Error::Completion(error) = &bridged else {
        panic!("the error type holds the completion error whole, got {bridged:?}");
    };
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert!(
        error.message.contains("connection refused"),
        "the cause must survive the bridge: {error:?}"
    );
    assert_eq!(error.prose, "model access is turned off or not configured");
    assert!(bridged.to_string().contains("connection refused"));
}

#[test]
fn a_frontmatter_failure_message_ends_with_its_line_and_column() {
    // The parser's surfaced YAML position crosses the error-type
    // bridge and ends the run's failure message. A frontmatter failure
    // predates the prompt's name, so the message names no prompt.
    let source = concat!(
        "---\n",
        "name: x\n",
        "description: d\n",
        "uses:\n",
        "  - not a Pack id\n",
        "---\n",
        "\n# T\n\n## S\n\np\n",
    );
    let parse = Prompt::parse(source, "prompt", "test")
        .0
        .expect_err("a Pack id with spaces must be rejected");
    let error = Error::Parse(parse);
    assert_eq!(error.kind(), crate::RunErrorKind::Parse);
    assert_eq!(error.message(), format!("{error} (line 5, column 5)"));
}

#[test]
fn a_structured_failure_message_ends_with_the_prompt_name_and_position() {
    // A post-frontmatter parse failure ends its message with the
    // prompt's frontmatter name and the offending span's line and
    // column.
    let source = "---\nname: dup\ndescription: d\n---\n\n# T\n\n## S\n\np\n\n## S\n\nq\n";
    let parse = Prompt::parse(source, "prompt", "test")
        .0
        .expect_err("duplicate sibling sections must be rejected");
    let error = Error::Parse(parse);
    assert_eq!(
        error.message(),
        format!("{error} (prompt dup, line 12, column 1)")
    );
}

#[test]
fn a_parse_failure_run_raises_names_the_prompt_and_its_file() {
    // A parse failure `run` raises about a parsed prompt gets the prompt's
    // name as its `Name` fact and the prompt's file in its prose, and its
    // message ends with the name, then the start line and column when it
    // has a location.
    use std::num::NonZeroU32;

    use runvil_types::error::{ErrorCode, FactKey, Location};

    let code = ErrorCode::StructureInvalid;
    let message = "not a runvil prompt: no runvil version";
    let stamped = crate::prompt::detail::with_prompt_context(
        crate::prompt::detail::parse_error(code, message),
        "greeter",
        "agents/greeter.md",
    );
    assert_eq!(stamped.code, code);
    assert_eq!(stamped.message, message);
    assert_eq!(stamped.at, None);
    assert_eq!(
        stamped.facts.get(&FactKey::Name),
        Some(&serde_json::Value::from("greeter"))
    );
    assert_eq!(
        stamped.prose,
        format!("agents/greeter.md: {message}. {}", code.prose())
    );

    let error = Error::parse(code, message).with_prompt_context("greeter", "agents/greeter.md");
    assert_eq!(error.kind(), crate::RunErrorKind::Parse);
    assert!(error.originating_error().is_none());
    assert_eq!(error.message(), format!("{message} (prompt greeter)"));

    let mut located = stamped;
    located.at = Location::new(
        "agents/greeter.md",
        NonZeroU32::new(3).expect("3 is non-zero"),
        NonZeroU32::new(7).expect("7 is non-zero"),
        NonZeroU32::new(4).expect("4 is non-zero"),
        NonZeroU32::MIN,
    );
    assert_eq!(
        Error::Parse(located).message(),
        format!("{message} (prompt greeter, line 3, column 7)"),
        "the location names the start line and column, not the end"
    );
}

#[test]
fn an_internal_fault_message_ends_with_the_rust_file_and_line() {
    // An internal invariant failure locates itself in the Rust
    // source, captured at the construction site.
    let expected_line = line!() + 1;
    let error = Error::internal("a test invariant");
    assert_eq!(
        error.message(),
        format!(
            "internal invariant violated: a test invariant (at {}:{expected_line})",
            file!()
        )
    );
}

#[test]
fn a_failure_without_a_location_ends_its_message_with_its_own_text() {
    // Cancellation and the other non-positional kinds have no source
    // position to point at.
    let error = Error::Interrupted;
    assert_eq!(error.message(), error.to_string());
}

#[test]
fn interrupted_and_substitution_failures_classify_under_kinds_a_finished_run_reports() {
    // A finished run reports cancellation as `RunOutcome::Cancelled`, so an
    // interruption that reaches a failed outcome is the Engine's fault, and
    // a failed `{{ }}` substitution is the author's program failing, as any
    // Lua fault is.
    assert_eq!(Error::Interrupted.kind(), crate::RunErrorKind::Internal);
    let (var, sys) = (serde_json::json!({}), serde_json::json!({}));
    let unset = |_: &str| -> Result<Option<serde_json::Value>> { Ok(None) };
    let sources = crate::subst::Sources {
        args: "",
        argv: None,
        item: None,
        var: &var,
        sys: &sys,
        globals: &unset,
    };
    let error = crate::subst::substitute("{{ var.missing }}", &sources)
        .expect_err("a missing key fails the substitution");
    assert!(matches!(error, Error::Substitution(_)), "{error:?}");
    assert_eq!(error.kind(), crate::RunErrorKind::Lua);
}

#[test]
fn requirements_unmet_classifies_and_reports_the_notice_as_its_message() {
    // The refusal notice is the whole message, and the kind classifies
    // it for code.
    let error = Error::RequirementsUnmet {
        notice: "the environment cannot satisfy this prompt:\n- role 'strong': requires a context of at least 200000 tokens; the model filling it provides 32000".to_owned(),
        cause: None,
    };
    assert_eq!(error.kind(), crate::RunErrorKind::RequirementsUnmet);
    assert_eq!(error.message(), error.to_string());
    assert!(error.message().contains("strong"));
}

#[test]
fn the_call_depth_cap_is_a_quota_that_lua_still_reads_as_lua() {
    // The run classifies the cap as a quota, while an author's `pcall`
    // keeps reading the `lua` kind it always branched on.
    for (via, text) in [
        (DepthVia::Call, "call recursion exceeded cap of 8"),
        (DepthVia::Fanout, "tasks.fanout recursion exceeded cap of 8"),
    ] {
        let error = Error::CallDepthExceeded { cap: 8, via };
        assert_eq!(error.to_string(), text);
        assert_eq!(
            runvil_lua::ErrorValue::kind(&error),
            runvil_lua::ErrorKind::Lua
        );
        assert_eq!(error.kind(), crate::RunErrorKind::Quota);
        assert_eq!(error.message(), text);
        assert!(error.originating_error().is_none());
    }
}

#[test]
fn an_owned_lua_internal_message_reaches_the_engine_error_whole() {
    let message = format!("tool call counts: id {:?} could not be read", "web/fetch");
    let expected_line = line!() + 1;
    let site = std::panic::Location::caller();
    let error = Error::from(LuaError::Internal {
        message: std::borrow::Cow::Owned(message.clone()),
        location: site,
    });
    match &error {
        Error::Internal {
            message: carried, ..
        } => assert_eq!(carried, &message),
        other => panic!("expected an internal error, got {other:?}"),
    }
    assert_eq!(
        error.to_string(),
        format!("internal invariant violated: {message}")
    );
    assert_eq!(error.kind(), crate::RunErrorKind::Internal);
    assert_eq!(
        error.message(),
        format!(
            "internal invariant violated: {message} (at {}:{expected_line})",
            file!()
        ),
        "the message ends with the construction site's file and line"
    );
}

/// One failure of each store variant, with fields a table can carry.
fn one_failure_per_store_variant() -> Vec<runvil_vfs::VfsError> {
    use runvil_vfs::{PathReason, VfsError};
    let path = || "notes.md".to_owned();
    vec![
        VfsError::NotFound { path: path() },
        VfsError::AlreadyExists { path: path() },
        VfsError::NotADirectory { path: path() },
        VfsError::IsADirectory { path: path() },
        VfsError::DirectoryNotEmpty { path: path() },
        VfsError::NotUtf8 {
            path: path(),
            valid_up_to: 7,
        },
        VfsError::InvalidPath {
            path: path(),
            reason: PathReason::Traversal,
        },
        VfsError::InvalidRange {
            path: path(),
            reason: "the bounds are invalid",
        },
        VfsError::Anchor {
            path: path(),
            anchor: "TODO".to_owned(),
            count: 2,
        },
        VfsError::PermissionDenied {
            path: path(),
            reason: "denied".to_owned(),
        },
        VfsError::ApprovalRequired {
            path: path(),
            reason: "ask".to_owned(),
        },
        VfsError::ScopeEnded { path: path() },
        VfsError::ReadOnly { path: path() },
        VfsError::OutsideRoot {
            path: path(),
            detail: "escapes".to_owned(),
        },
        VfsError::RootProtected { path: path() },
        VfsError::NoStore,
        VfsError::CrossMount {
            path: path(),
            detail: "two mounts".to_owned(),
        },
        VfsError::Io {
            path: path(),
            kind: std::io::ErrorKind::PermissionDenied,
            raw_os_error: Some(13),
            message: "denied".to_owned(),
        },
        VfsError::Unsupported {
            path: path(),
            detail: "no".to_owned(),
        },
        VfsError::Conflict {
            path: path(),
            detail: "raced".to_owned(),
        },
        VfsError::Backend {
            message: "gone".to_owned(),
        },
    ]
}

#[test]
fn every_store_failure_rebuilds_from_its_raised_table_under_its_own_tag() {
    use runvil_vfs::VfsError;
    for failure in one_failure_per_store_variant() {
        let raised = runvil_lua::Raised {
            kind: runvil_lua::ErrorKind::Store,
            message: failure.to_string(),
            fields: runvil_lua::store_error_value_fields(&failure)
                .into_iter()
                .collect(),
        };
        let Error::Store { cause: source, .. } = Error::from(LuaError::Raised(raised)) else {
            panic!("a raised store table maps to a store error: {failure:?}");
        };
        assert_eq!(source.tag(), failure.tag(), "{failure:?}");
        match (&source, &failure) {
            (
                _,
                VfsError::NotFound { .. }
                | VfsError::AlreadyExists { .. }
                | VfsError::NotADirectory { .. }
                | VfsError::IsADirectory { .. }
                | VfsError::DirectoryNotEmpty { .. }
                | VfsError::ScopeEnded { .. }
                | VfsError::ReadOnly { .. }
                | VfsError::RootProtected { .. }
                | VfsError::NotUtf8 { .. }
                | VfsError::InvalidPath { .. }
                | VfsError::InvalidRange { .. }
                | VfsError::Anchor { .. }
                | VfsError::NoStore,
            ) => assert_eq!(source, failure, "{failure:?}"),
            (
                VfsError::Io {
                    kind, raw_os_error, ..
                },
                _,
            ) => assert_eq!(
                (*kind, *raw_os_error),
                (std::io::ErrorKind::Other, None),
                "a table holds no I/O kind or OS code"
            ),
            _ => {}
        }
    }
}
