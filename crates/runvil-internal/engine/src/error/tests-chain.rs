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
        error.prose, "the connection to the model backend failed",
        "the table's message, the prose, holds no cause"
    );
}

#[test]
fn the_lua_crates_failures_end_a_run_with_the_kind_and_text_they_always_had() {
    let site = std::panic::Location::caller();
    let emitter = crate::test_support::recording::null_emitter();
    let compiled = runvil_lua::LuaProgram::compile(
        "x =",
        "prompt",
        "section `S` prologue",
        NonZeroU32::new(7).expect("7 is non-zero"),
        &emitter,
        "S",
    )
    .expect_err("the block must not compile");
    let tool = runvil_types::tools::tool_error(ErrorCode::ToolBackend, "the backend refused");
    let info = runvil_types::tools::ToolInfo::new(
        runvil_types::tools::ToolId::parse("tools/echo").expect("a valid tool id"),
        "echo tool",
        serde_json::json!({ "type": "object" }),
    );
    let dispatched = runvil_lua::prepare_dispatch(
        &crate::lua::ToolBinding::from_info("echo", &info),
        None,
        Err(tool),
        &runvil_types::untrusted::GuardNonce::from_seed(&crate::test_support::scenario::TEST_SEED),
        &emitter,
        "S",
        crate::lua::ScriptReport { turn: 0 },
    )
    .expect_err("the tool's failure fails the dispatch");
    let mut not_found = runvil_lua::store_error(&VfsError::NotFound {
        path: "a.md".to_owned(),
    });
    "not found: a.md".clone_into(&mut not_found.message);
    for (failure, kind, message) in [
        (
            compiled,
            RunErrorKind::Lua,
            "lua compilation error at section `S` prologue (line 7): syntax error: \
             [string \"line 7\"]:1: unexpected symbol near <eof>"
                .to_owned(),
        ),
        (
            runvil_lua::internal_at("a test invariant", site),
            RunErrorKind::Internal,
            format!(
                "internal invariant violated: a test invariant (at {}:{})",
                site.file(),
                site.line()
            ),
        ),
        (
            dispatched,
            RunErrorKind::Tool,
            "tool call failure: the backend refused".to_owned(),
        ),
        (
            lua_quota(QuotaResource::LogEvent),
            RunErrorKind::Quota,
            "lua log event quota exceeded".to_owned(),
        ),
        (
            task_cancelled(&"0.1".parse().expect("the test task id parses")),
            RunErrorKind::Lua,
            "task `0.1` was cancelled".to_owned(),
        ),
        (not_found, RunErrorKind::Vfs, "not found: a.md".to_owned()),
    ] {
        assert_eq!(failed(&failure), (kind, message));
    }
    assert_eq!(
        crate::execute::outcome(&interrupted()),
        RunOutcome::Failed {
            kind: RunErrorKind::Internal,
            message: interrupted().message,
        },
        "an interruption, which the scheduler ends as cancelled before it reaches a failed \
         outcome"
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
