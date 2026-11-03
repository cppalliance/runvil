//! Tests for the Engine's failure constructors: each one's code, message,
//! prose, location, and facts, the kind and message each ends a run with,
//! and the Lua error table each renders as.

use std::num::NonZeroU32;

use runvil_lua::{
    ErrorField, ErrorKind, QuotaResource, error_fields, error_kind, error_table_message,
};
use runvil_types::error::{ErrorCode, FactKey, Location};
use runvil_types::ids::TaskId;
use serde_json::Value;

use super::*;
use crate::RunErrorKind;
use crate::execute::RunOutcome;

#[path = "tests-chain.rs"]
mod chain;

/// The kind and message `error` ends a run with.
fn failed(error: &Error) -> (RunErrorKind, String) {
    match crate::execute::outcome(error) {
        RunOutcome::Failed { kind, message } => (kind, message),
        other => panic!("a failure ends a run as failed, got {other:?}"),
    }
}

/// The task id `path` names.
fn task(path: &str) -> TaskId {
    path.parse().expect("the test task id parses")
}

/// The string field `name` with `value`.
fn field(name: &str, value: &str) -> (String, ErrorField) {
    (name.to_owned(), ErrorField::String(value.to_owned()))
}

#[test]
fn an_internal_fault_is_located_at_its_rust_call_site_and_ends_its_message_there() {
    let expected_line = line!() + 1;
    let error = internal("a test invariant");
    assert_eq!(error.code, ErrorCode::Internal);
    assert_eq!(
        error.message,
        "internal invariant violated: a test invariant"
    );
    assert_eq!(error.prose, ErrorCode::Internal.prose());
    assert_eq!(
        error.at,
        Some(Location::line(
            file!(),
            NonZeroU32::new(expected_line).expect("a line is non-zero")
        ))
    );
    assert!(error.facts.is_empty());
    assert_eq!(
        failed(&error),
        (
            RunErrorKind::Internal,
            format!(
                "internal invariant violated: a test invariant (at {}:{expected_line})",
                file!()
            )
        )
    );
    assert_eq!(error_kind(&error), ErrorKind::Internal);
    assert_eq!(error_table_message(&error), error.message);
}

#[test]
fn a_lua_failure_keeps_its_text_and_reads_as_lua() {
    let error = lua_runtime("messages must not be empty");
    assert_eq!(error.code, ErrorCode::LuaRuntime);
    assert_eq!(error.message, "messages must not be empty");
    assert_eq!(error.prose, ErrorCode::LuaRuntime.prose());
    assert_eq!((error.at.clone(), error.facts.len()), (None, 0));
    assert_eq!(
        failed(&error),
        (RunErrorKind::Lua, "messages must not be empty".to_owned())
    );
    assert_eq!(error_kind(&error), ErrorKind::Lua);
}

#[test]
fn an_interruption_has_no_location_and_reads_as_cancelled() {
    let error = interrupted();
    assert_eq!(error.code, ErrorCode::Interrupted);
    assert_eq!(
        error.message,
        "interrupted: the run was cancelled or this call was stopped"
    );
    assert_eq!(
        failed(&error),
        (RunErrorKind::Internal, error.message.clone())
    );
    assert_eq!(error_kind(&error), ErrorKind::Cancelled);
    assert_eq!(error_fields(&error), Vec::new());
}

#[test]
fn an_unsupported_version_names_the_major_only_in_its_message() {
    let error = unsupported_version(3);
    assert_eq!(error.code, ErrorCode::UnsupportedVersion);
    assert_eq!(
        error.message,
        "unsupported runvil version: 3 (this build supports major 0)"
    );
    assert!(error.facts.is_empty());
    assert_eq!(failed(&error), (RunErrorKind::Parse, error.message.clone()));
    assert_eq!(error_kind(&error), ErrorKind::Internal);
}

#[test]
fn a_refusal_notice_is_the_prose_and_the_whole_message_without_a_cause() {
    let notice = "the environment cannot satisfy this prompt:\n- role 'strong': requires a \
                  context of at least 200000 tokens; the model filling it provides 32000";
    let error = requirements_unmet(notice, None);
    assert_eq!(error.code, ErrorCode::RequirementsUnmet);
    assert_eq!(
        (error.message.as_str(), error.prose.as_str()),
        (notice, notice)
    );
    assert_eq!(
        failed(&error),
        (RunErrorKind::RequirementsUnmet, notice.to_owned())
    );
    assert_eq!(error_kind(&error), ErrorKind::Internal);
    assert_eq!(error_table_message(&error), notice);
}

#[test]
fn a_missing_model_names_its_section() {
    let error = model_required("Only");
    assert_eq!(error.code, ErrorCode::ModelRequired);
    assert_eq!(error.message, "model binding required for section Only");
    assert_eq!(error.prose, ErrorCode::ModelRequired.prose());
    assert_eq!(
        error.facts.get(&FactKey::Section),
        Some(&Value::from("Only"))
    );
    assert_eq!(
        failed(&error),
        (RunErrorKind::RequirementsUnmet, error.message.clone())
    );
    assert_eq!(error_kind(&error), ErrorKind::ModelRequired);
    assert_eq!(error_fields(&error), Vec::new());
}

#[test]
fn context_exhaustion_holds_its_reason_and_renders_it() {
    for (reason, message) in [
        (
            "provider",
            "context exhausted: the provider rejected the request as exceeding the model's \
             context window",
        ),
        ("manual", "context exhausted: manual"),
    ] {
        let error = context_exhausted(reason);
        assert_eq!(error.code, ErrorCode::ContextExhausted);
        assert_eq!(error.message, message);
        assert_eq!(
            error.facts.get(&FactKey::Reason),
            Some(&Value::from(reason))
        );
        assert_eq!(
            failed(&error),
            (RunErrorKind::ContextExhausted, message.to_owned())
        );
        assert_eq!(error_kind(&error), ErrorKind::ContextExhausted);
        assert_eq!(error_fields(&error), vec![field("reason", reason)]);
    }
}

#[test]
fn a_spent_quota_and_the_call_depth_cap_are_quotas_that_lua_reads_as_lua() {
    for (resource, tag, text) in [
        (
            QuotaResource::LogEvent,
            "log_event",
            "lua log event quota exceeded",
        ),
        (
            QuotaResource::LogByte,
            "log_byte",
            "lua log byte quota exceeded",
        ),
    ] {
        let error = lua_quota(resource);
        assert_eq!(error.code, ErrorCode::LuaQuota);
        assert_eq!(error.message, text);
        assert_eq!(error.facts.get(&FactKey::Reason), Some(&Value::from(tag)));
        assert_eq!(failed(&error), (RunErrorKind::Quota, text.to_owned()));
        assert_eq!(
            (error_kind(&error), error_fields(&error)),
            (ErrorKind::Lua, Vec::new())
        );
    }
    for (via, tag, text) in [
        (DepthVia::Call, "call", "call recursion exceeded cap of 8"),
        (
            DepthVia::Fanout,
            "fanout",
            "tasks.fanout recursion exceeded cap of 8",
        ),
    ] {
        let error = call_depth_exceeded(8, via);
        assert_eq!(error.code, ErrorCode::CallDepthExceeded);
        assert_eq!(error.message, text);
        assert_eq!(error.facts.get(&FactKey::Reason), Some(&Value::from(tag)));
        assert_eq!(error.facts.get(&FactKey::Limit), Some(&Value::from(8)));
        assert_eq!(failed(&error), (RunErrorKind::Quota, text.to_owned()));
        assert_eq!(
            (error_kind(&error), error_fields(&error)),
            (ErrorKind::Lua, Vec::new())
        );
    }
}

#[test]
fn leaked_tasks_are_named_in_spawn_order_and_joined_in_the_table() {
    let error = tasks_live(&[task("1.2"), task("1.10")]);
    assert_eq!(error.code, ErrorCode::TasksLive);
    assert_eq!(
        error.message,
        "chain ended with tasks still live: 1.2, 1.10; wait on or cancel every task a chain \
         spawns before it ends"
    );
    assert_eq!(
        error.facts.get(&FactKey::Tasks),
        Some(&Value::from(vec!["1.2", "1.10"]))
    );
    assert_eq!(failed(&error), (RunErrorKind::Lua, error.message.clone()));
    assert_eq!(error_kind(&error), ErrorKind::TasksLive);
    assert_eq!(error_fields(&error), vec![field("tasks", "1.2, 1.10")]);
}

#[test]
fn a_task_failure_names_its_task_in_its_fact_and_its_table() {
    let id = task("1.3");
    for (error, code, kind, message) in [
        (
            task_not_owned(&id),
            ErrorCode::TaskNotOwned,
            ErrorKind::TaskNotOwned,
            "task `1.3` is not a task this chain owns",
        ),
        (
            task_consumed(&id),
            ErrorCode::TaskConsumed,
            ErrorKind::TaskConsumed,
            "task `1.3` was already delivered: a task's result is taken by one wait",
        ),
        (
            task_cancelled(&id),
            ErrorCode::TaskCancelled,
            ErrorKind::Cancelled,
            "task `1.3` was cancelled",
        ),
    ] {
        assert_eq!(error.code, code);
        assert_eq!(error.message, message);
        assert_eq!(error.prose, code.prose());
        assert_eq!(error.facts.get(&FactKey::Task), Some(&Value::from("1.3")));
        assert_eq!(failed(&error), (RunErrorKind::Lua, message.to_owned()));
        assert_eq!(error_kind(&error), kind);
        assert_eq!(error_fields(&error), vec![field("task", "1.3")]);
    }
}

#[test]
fn a_store_failure_takes_the_stores_code_and_facts_under_its_own_message() {
    let cause = runvil_vfs::VfsError::Anchor {
        path: "notes.md".to_owned(),
        anchor: "TODO".to_owned(),
        count: 2,
    };
    let op = crate::lua::VfsOp::StrReplace {
        path: "notes.md".to_owned(),
        old: "TODO".to_owned(),
        new: "DONE".to_owned(),
    };
    let error = store_op(&op, &cause);
    let expected = runvil_lua::store_error(&cause);
    assert_eq!((error.code, &error.facts), (expected.code, &expected.facts));
    assert_eq!(error.message, crate::lua::store_error_message(&op, &cause));
    assert_eq!(error.prose, ErrorCode::Anchor.prose());
    assert_eq!(failed(&error), (RunErrorKind::Vfs, error.message.clone()));
    assert_eq!(error_kind(&error), ErrorKind::Store);
    assert_eq!(
        error_fields(&error),
        runvil_lua::store_error_value_fields(&cause)
    );
}

#[test]
fn a_determinism_violation_is_a_store_kind_that_lua_reads_as_internal() {
    let error = determinism("a.md: claimed by 1 and 2");
    assert_eq!(error.code, ErrorCode::Determinism);
    assert_eq!(
        error.message,
        "store determinism violation: a.md: claimed by 1 and 2"
    );
    assert_eq!(failed(&error), (RunErrorKind::Vfs, error.message.clone()));
    assert_eq!(
        (error_kind(&error), error_fields(&error)),
        (ErrorKind::Internal, Vec::new())
    );
}

#[test]
fn a_refused_tool_call_names_the_tool_and_its_text_is_its_prose() {
    let out_of_scope = out_of_scope_tool("echo", true, &["other".to_owned()], true);
    let unbound = unbound_tool("web/gone", &["web/fetch".to_owned()]);
    let model_only = model_only_tool("web/render", "web/render");
    for (error, code, kind, name) in [
        (
            &out_of_scope,
            ErrorCode::OutOfScopeTool,
            ErrorKind::OutOfScopeTool,
            "echo",
        ),
        (
            &unbound,
            ErrorCode::UnboundTool,
            ErrorKind::UnboundTool,
            "web/gone",
        ),
        (
            &model_only,
            ErrorCode::ModelOnlyTool,
            ErrorKind::ModelOnlyTool,
            "web/render",
        ),
    ] {
        assert_eq!(error.code, code);
        assert_eq!(error.prose, error.message, "{code:?}");
        assert_eq!(error.facts.get(&FactKey::Name), Some(&Value::from(name)));
        assert_eq!(failed(error), (RunErrorKind::Tool, error.message.clone()));
        assert_eq!(error_kind(error), kind);
        assert_eq!(error_fields(error), vec![field("name", name)]);
    }
    assert_eq!(
        out_of_scope.message,
        "tool \"echo\" is not in this section's scope; aliases this section offers: \
         [\"other\"] (a catalog tool that was not offered in this section); a Dynamic Set tool \
         is reached through GetDynamicTools and CallDynamicTool"
    );
    assert_eq!(
        unbound.message,
        "tool \"web/gone\" is not a tool in this run; catalog tools: [\"web/fetch\"]"
    );
    assert!(
        model_only
            .message
            .starts_with("tool \"web/render\" (web/render) is model-only")
    );
}

#[test]
fn an_unconverged_tool_loop_is_a_tool_kind() {
    let error = tool_loop_exhausted();
    assert_eq!(error.code, ErrorCode::ToolLoopExhausted);
    assert_eq!(error.message, "tool-call loop did not converge");
    assert_eq!(failed(&error), (RunErrorKind::Tool, error.message.clone()));
    assert_eq!(error_kind(&error), ErrorKind::ToolLoopExhausted);
}

#[test]
fn broken_tool_sets_name_the_rule_they_broke() {
    let packs = tool_sets(&SetEvidence::RenderedPacks {
        asked: vec!["web".parse().expect("a Pack id")],
        answered: Vec::new(),
    });
    assert_eq!(packs.code, ErrorCode::ToolSetsInvalid);
    assert_eq!(
        packs.message,
        "the render answered the Packs [] where the section offered the Packs [web]; a render \
         answers each offered Pack once, in offer order"
    );
    assert_eq!(
        packs.facts.get(&FactKey::Rule),
        Some(&Value::from("rendered_packs"))
    );
    assert_eq!(
        failed(&packs),
        (RunErrorKind::Binding, packs.message.clone())
    );
    let clash = tool_sets(&SetEvidence::Tools {
        rule: SetRule::UniqueWireName("fetch".to_owned()),
        tools: crate::lua::NonEmpty::new(
            SetTool {
                id: "web/fetch".parse().expect("a tool id"),
                set: crate::lua::OfferMode::Static,
            },
            Vec::new(),
        ),
    });
    assert_eq!(
        clash.facts.get(&FactKey::Rule),
        Some(&Value::from("unique_wire_name"))
    );
    assert_eq!(
        (error_kind(&clash), error_fields(&clash)),
        (ErrorKind::Internal, Vec::new())
    );
}
