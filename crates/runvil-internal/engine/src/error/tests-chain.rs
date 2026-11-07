//! The cause convention: a failure built with a cause holds the cause's
//! text once in its message, after its own, and keeps its prose free of
//! it; a model failure's table holds only its prose; the Lua crate's
//! failures end a run with their own code and text, a located one's
//! ending with where it failed; the parser's failures end theirs with the
//! prompt's name and position, a frontmatter failure's with its position
//! alone; and the store probe names a missing store.

use runvil_types::error::ErrorCode;
use runvil_vfs::VfsError;

use super::*;
use crate::prompt::Prompt;

#[test]
fn an_internal_failure_built_with_a_cause_shows_its_text_once() {
    // The call does not fit beside `let error`, so rustfmt puts the
    // function name on the following line, and that line is the one
    // `track_caller` records.
    let expected_line = line!() + 2;
    let error =
        make_internal_with_cause_error("a test invariant", &std::io::Error::other("the cause"));
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
    let error =
        make_requirements_unmet_error(notice, Some(&make_lua_runtime_error("assertion failed")));
    assert_eq!(error.message, format!("{notice}: assertion failed"));
    assert_eq!(error.prose, notice, "the notice alone is the model's text");
    assert_eq!(
        failed(&error),
        (
            ErrorCode::RequirementsUnmet,
            format!("{notice}: assertion failed")
        )
    );
}

#[test]
fn a_schema_build_failure_names_the_alias_and_ends_with_the_refusal() {
    let refusal = runvil_model_client::detail::tool_schema_new("echo", "", serde_json::json!([]))
        .expect_err("an array is not an object schema");
    let error = make_bind_schema_error("echo", &refusal);
    assert_eq!(error.code, ErrorCode::ToolSchemaInvalid);
    assert_eq!(
        error.message,
        "model-facing schema build failure for tool alias \"echo\": tool \"echo\" parameters \
         schema must be a JSON object"
    );
    assert_eq!(error.prose, ErrorCode::ToolSchemaInvalid.prose());
    assert_eq!(
        error.facts.get(&ErrorFact::Name),
        Some(&Value::from("echo"))
    );
    assert_eq!(
        error.facts.get(&ErrorFact::Reason),
        refusal.facts.get(&ErrorFact::Reason),
        "the refusal's reason carries over"
    );
    assert_eq!(
        failed(&error),
        (ErrorCode::ToolSchemaInvalid, error.message.clone())
    );
    assert_eq!(
        (error_kind(&error), error_fields(&error)),
        (ErrorKind::Internal, Vec::new())
    );
}

#[test]
fn a_model_failure_holds_its_cause_in_the_run_message_and_not_in_its_table() {
    let transport = ErrorCode::Transport;
    let error = crate::model::make_completion_error(transport, crate::model::phrase(transport))
        .with_cause(&std::io::Error::other("connection reset by peer"));
    assert_eq!(
        failed(&error),
        (
            ErrorCode::Transport,
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
fn the_lua_crates_failures_end_a_run_with_their_own_code_and_text() {
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
    let tool = runvil_types::tools::make_tool_error(ErrorCode::ToolBackend, "the backend refused");
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
    let mut not_found = runvil_lua::make_store_error(&VfsError::NotFound {
        path: "a.md".to_owned(),
    });
    "not found: a.md".clone_into(&mut not_found.message);
    for (failure, code, message) in [
        (
            compiled,
            ErrorCode::LuaCompile,
            "lua compilation error at section `S` prologue (line 7): syntax error: \
             [string \"line 7\"]:1: unexpected symbol near <eof> (at prompt:7)"
                .to_owned(),
        ),
        (
            runvil_lua::make_internal_at_error("a test invariant", site),
            ErrorCode::Internal,
            format!(
                "internal invariant violated: a test invariant (at {}:{})",
                site.file(),
                site.line()
            ),
        ),
        (
            dispatched,
            ErrorCode::ToolBackend,
            "tool call failure: the backend refused".to_owned(),
        ),
        (
            make_lua_quota_error(QuotaResource::LogEvent),
            ErrorCode::LuaQuota,
            "lua log event quota exceeded".to_owned(),
        ),
        (
            make_task_canceled_error(&"0.1".parse().expect("the test task id parses")),
            ErrorCode::TaskCanceled,
            "task `0.1` was canceled".to_owned(),
        ),
        (not_found, ErrorCode::NotFound, "not found: a.md".to_owned()),
    ] {
        assert_eq!(failed(&failure), (code, message));
    }
}

#[test]
fn a_parsed_prompts_located_message_ends_with_its_name_and_position() {
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
    assert_eq!(
        failed(&refused),
        (
            ErrorCode::FrontmatterInvalid,
            format!("{} (line 5, column 5)", refused.message)
        ),
        "a frontmatter failure predates the prompt's name"
    );
    let duplicate = "---\nname: dup\ndescription: d\n---\n\n# T\n\n## S\n\np\n\n## S\n\nq\n";
    let refused = Prompt::parse(duplicate, "prompt", "test")
        .0
        .expect_err("duplicate sibling sections must be rejected");
    assert_eq!(
        failed(&refused),
        (
            ErrorCode::StructureInvalid,
            format!("{} (prompt dup, line 12, column 1)", refused.message)
        )
    );
}

#[test]
fn a_refused_uses_list_ends_with_its_line_and_never_names_the_pack_as_the_prompt() {
    let source = "---\nname: x\ndescription: d\nuses: [web, web]\n---\n\n# T\n";
    let refused = Prompt::parse(source, "prompt", "test")
        .0
        .expect_err("a Pack id declared twice must be refused");
    assert_eq!(
        refused.facts.get(&ErrorFact::Name),
        Some(&Value::from("web")),
        "the refused Pack's id is the failure's name"
    );
    assert_eq!(
        failed(&refused),
        (
            ErrorCode::FrontmatterInvalid,
            "invalid frontmatter: Pack id web is declared more than once under uses \
             (line 4, column 1)"
                .to_owned()
        )
    );
}

#[test]
fn a_parse_failure_run_raises_names_the_prompt_and_its_file() {
    let code = ErrorCode::StructureInvalid;
    let message = "not a runvil prompt: no runvil version";
    let stamped = crate::prompt::detail::with_prompt_context(
        runvil_types::error::make_failure_error(code, message),
        "greeter",
        "agents/greeter.md",
    );
    assert_eq!((stamped.code, stamped.message.as_str()), (code, message));
    assert_eq!(stamped.at, None);
    assert_eq!(
        stamped.facts.get(&ErrorFact::Name),
        Some(&Value::from("greeter"))
    );
    assert_eq!(
        stamped.prose,
        format!("agents/greeter.md: {message}. {}", code.prose())
    );
    assert_eq!(
        stamped.located_message(),
        format!("{message} (prompt greeter)")
    );
}

#[test]
fn a_store_probe_failure_names_a_missing_store_and_keeps_the_fixed_text_otherwise() {
    let missing = make_store_error(&VfsError::NoStore);
    assert_eq!(missing.code, ErrorCode::NoStore);
    assert_eq!(
        missing.message,
        "this run declares no store, so no store operation can run"
    );
    let backend = VfsError::Backend {
        message: "the disk is gone".to_owned(),
    };
    let failure = make_store_error(&backend);
    assert_eq!(failure.code, ErrorCode::Backend);
    assert_eq!(
        failure.message,
        format!("store operation failed: {backend}")
    );
    assert_eq!(
        failed(&failure),
        (ErrorCode::Backend, failure.message.clone())
    );
    assert_eq!(
        error_fields(&failure),
        runvil_lua::store_error_value_fields(&backend)
    );
}
