//! The cause convention: a failure built with a cause holds the cause's
//! text once in its message, after its own, and keeps its prose free of
//! it; a model failure's table holds only its prose; the Lua crate's
//! failures end a run as they always did; and the store probe names a
//! missing store.

use runvil_types::error::ErrorCode;
use runvil_vfs::VfsError;

use super::*;

#[test]
fn an_internal_failure_built_with_a_cause_shows_its_text_once() {
    let expected_line = line!() + 1;
    let error = internal_with_cause("a test invariant", &std::io::Error::other("the cause"));
    assert_eq!(error.code, ErrorCode::Internal);
    assert_eq!(
        error.message,
        "internal invariant violated: a test invariant: the cause"
    );
    assert_eq!(error.prose, ErrorCode::Internal.prose());
    assert_eq!(
        failed(&error).1,
        format!(
            "internal invariant violated: a test invariant: the cause (at {}:{expected_line})",
            file!()
        ),
        "the message ends with the construction site's file and line"
    );
}

#[test]
fn a_requirements_refusal_appends_its_cause_once_after_the_notice() {
    let notice = "the environment cannot satisfy this prompt:\n- the H1 gate failed";
    let error = requirements_unmet(notice, Some(&lua_runtime("assertion failed")));
    assert_eq!(error.message, format!("{notice}: assertion failed"));
    assert_eq!(error.prose, notice, "the notice alone is the model's text");
    assert_eq!(
        failed(&error),
        (
            RunErrorKind::RequirementsUnmet,
            format!("{notice}: assertion failed")
        )
    );
    assert_eq!(error_table_message(&error), error.message);
}

#[test]
fn a_schema_build_failure_names_the_alias_and_ends_with_the_refusal() {
    let refusal = runvil_model_client::detail::tool_schema_new("echo", "", serde_json::json!([]))
        .expect_err("an array is not an object schema");
    let error = bind_schema("echo", &refusal);
    assert_eq!(error.code, ErrorCode::ToolSchemaInvalid);
    assert_eq!(
        error.message,
        "model-facing schema build failure for tool alias \"echo\": tool \"echo\" parameters \
         schema must be a JSON object"
    );
    assert_eq!(error.prose, ErrorCode::ToolSchemaInvalid.prose());
    assert_eq!(error.facts.get(&FactKey::Name), Some(&Value::from("echo")));
    assert_eq!(
        error.facts.get(&FactKey::Reason),
        refusal.facts.get(&FactKey::Reason),
        "the refusal's reason carries over"
    );
    assert_eq!(
        failed(&error),
        (RunErrorKind::Binding, error.message.clone())
    );
    assert_eq!(
        (error_kind(&error), error_fields(&error)),
        (ErrorKind::Internal, Vec::new())
    );
}

#[test]
fn a_model_failure_holds_its_cause_in_the_run_message_and_not_in_its_table() {
    let transport = ErrorCode::Transport;
    let error = crate::model::completion_error(transport, crate::model::phrase(transport))
        .with_cause(&std::io::Error::other("connection reset by peer"));
    assert_eq!(
        failed(&error),
        (
            RunErrorKind::Completion,
            "the connection to the model backend failed: connection reset by peer".to_owned()
        ),
        "the run's message holds the model failure's cause once"
    );
    assert_eq!(error_kind(&error), ErrorKind::ModelFailure);
    assert_eq!(
        error_table_message(&error),
        "the connection to the model backend failed"
    );
}

#[test]
fn the_lua_crates_failures_end_a_run_with_the_kind_and_text_they_always_had() {
    use runvil_lua::{Error as LuaError, ErrorField, Raised};

    let site = std::panic::Location::caller();
    let tool = runvil_types::tools::tool_error(ErrorCode::ToolBackend, "the backend refused");
    let table = |kind, message: &str, fields: &[(&str, &str)]| {
        LuaError::Raised(Raised {
            kind,
            message: message.to_owned(),
            fields: fields
                .iter()
                .map(|(name, value)| ((*name).to_owned(), ErrorField::String((*value).to_owned())))
                .collect(),
        })
    };
    for (failure, kind, message) in [
        (
            LuaError::LuaCompile {
                location: "section `S` prologue".to_owned(),
                source_line: 7,
                lua_source: "x =".to_owned(),
                message: "syntax error near '='".to_owned(),
                cause: Box::new(mlua::Error::SyntaxError {
                    message: "syntax error near '='".to_owned(),
                    incomplete_input: false,
                }),
            },
            RunErrorKind::Lua,
            "lua compilation error at section `S` prologue (line 7): syntax error near '='"
                .to_owned(),
        ),
        (
            LuaError::Internal {
                message: "a test invariant".into(),
                location: site,
            },
            RunErrorKind::Internal,
            format!(
                "internal invariant violated: a test invariant (at {}:{})",
                site.file(),
                site.line()
            ),
        ),
        (
            LuaError::Tool {
                message: tool.prose.clone(),
                cause: Box::new(tool),
            },
            RunErrorKind::Tool,
            "tool call failure: the backend refused".to_owned(),
        ),
        (
            LuaError::LuaQuota {
                resource: QuotaResource::LogEvent,
            },
            RunErrorKind::Quota,
            "lua log event quota exceeded".to_owned(),
        ),
        (
            table(
                ErrorKind::Cancelled,
                "task `0.1` was cancelled",
                &[("task", "0.1")],
            ),
            RunErrorKind::Lua,
            "task `0.1` was cancelled".to_owned(),
        ),
        (
            table(
                ErrorKind::Store,
                "not found: a.md",
                &[("reason", "not_found"), ("path", "a.md")],
            ),
            RunErrorKind::Vfs,
            "not found: a.md".to_owned(),
        ),
    ] {
        assert_eq!(failed(&into_universal(failure)), (kind, message));
    }
    assert_eq!(
        crate::execute::outcome(&into_universal(table(ErrorKind::Cancelled, "x", &[]))),
        RunOutcome::Failed {
            kind: RunErrorKind::Internal,
            message: interrupted().message,
        },
        "a cancelled table with no task is an interruption, which the scheduler ends as \
         cancelled before it reaches a failed outcome"
    );
}

#[test]
fn a_store_probe_failure_names_a_missing_store_and_keeps_the_fixed_text_otherwise() {
    let missing = store(&VfsError::NoStore);
    assert_eq!(missing.code, ErrorCode::NoStore);
    assert_eq!(
        missing.message,
        "this run declares no store, so no store operation can run"
    );
    let backend = VfsError::Backend {
        message: "the disk is gone".to_owned(),
    };
    let failure = store(&backend);
    assert_eq!(failure.code, ErrorCode::Backend);
    assert_eq!(
        failure.message,
        format!("store operation failed: {backend}")
    );
    assert_eq!(
        failed(&failure),
        (RunErrorKind::Vfs, failure.message.clone())
    );
    assert_eq!(
        error_fields(&failure),
        runvil_lua::store_error_value_fields(&backend)
    );
}
