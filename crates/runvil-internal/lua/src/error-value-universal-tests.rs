//! Tests for the universal error's Lua error value: the kind each code
//! renders as, the text its table holds, and the fields its facts give.

use runvil_types::error::{Error, ErrorCode, FactKey};
use serde_json::Value;

use super::super::tests::every_store_failure;
use super::super::{ErrorField, ErrorKind, store_error, store_error_value_fields};
use super::{error_fields, error_kind, error_table_message};

/// An error with `code` and `facts`, whose message and prose say which
/// they are.
fn error(code: ErrorCode, facts: &[(FactKey, Value)]) -> Error {
    let mut error = Error::new(code, "the message", "the prose");
    error.facts.extend(facts.iter().cloned());
    error
}

/// What `error`'s table holds: its kind, its message, and its fields.
fn rendered(error: &Error) -> (ErrorKind, String, Vec<(String, ErrorField)>) {
    (
        error_kind(error),
        error_table_message(error),
        error_fields(error),
    )
}

/// The string field `name` with `value`.
fn string(name: &str, value: &str) -> (String, ErrorField) {
    (name.to_owned(), ErrorField::String(value.to_owned()))
}

#[test]
fn a_task_refusal_renders_its_kind_and_its_task_field() {
    for (code, kind) in [
        (ErrorCode::TaskNotOwned, ErrorKind::TaskNotOwned),
        (ErrorCode::TaskConsumed, ErrorKind::TaskConsumed),
        (ErrorCode::TaskCancelled, ErrorKind::Cancelled),
    ] {
        let refusal = error(code, &[(FactKey::Task, Value::from("1.2"))]);
        assert_eq!(
            rendered(&refusal),
            (kind, "the message".to_owned(), vec![string("task", "1.2")]),
            "{code:?}"
        );
    }
    let interrupted = error(ErrorCode::Interrupted, &[]);
    assert_eq!(
        rendered(&interrupted),
        (ErrorKind::Cancelled, "the message".to_owned(), Vec::new())
    );
}

#[test]
fn leaked_tasks_render_as_one_string_joined_by_commas() {
    let leaked = error(
        ErrorCode::TasksLive,
        &[(FactKey::Tasks, Value::from(vec!["1", "1.2"]))],
    );
    assert_eq!(error_kind(&leaked), ErrorKind::TasksLive);
    assert_eq!(error_fields(&leaked), vec![string("tasks", "1, 1.2")]);
}

#[test]
fn every_store_failure_renders_the_fields_its_variant_gives() {
    for (failure, code) in every_store_failure() {
        let converted = store_error(&failure);
        assert_eq!(
            rendered(&converted),
            (
                ErrorKind::Store,
                converted.message.clone(),
                store_error_value_fields(&failure)
            ),
            "{code:?}"
        );
    }
}

#[test]
fn an_empty_reply_holds_its_prose_and_its_finish_reason() {
    let empty = error(
        ErrorCode::EmptyReply,
        &[(FactKey::FinishReason, Value::from("length"))],
    );
    assert_eq!(
        rendered(&empty),
        (
            ErrorKind::EmptyModelReply,
            "the prose".to_owned(),
            vec![string("finish_reason", "length")]
        )
    );
    let bare = error(ErrorCode::EmptyReply, &[]);
    assert_eq!(error_fields(&bare), Vec::new());
}

#[test]
fn a_model_failure_holds_its_prose_and_no_fields() {
    let failure = error(ErrorCode::Transport, &[(FactKey::Count, Value::from(3))]);
    assert_eq!(
        rendered(&failure),
        (ErrorKind::ModelFailure, "the prose".to_owned(), Vec::new())
    );
}

#[test]
fn context_exhaustion_renders_its_reason() {
    let exhausted = error(
        ErrorCode::ContextExhausted,
        &[(FactKey::Reason, Value::from("precheck"))],
    );
    assert_eq!(
        rendered(&exhausted),
        (
            ErrorKind::ContextExhausted,
            "the message".to_owned(),
            vec![string("reason", "precheck")]
        )
    );
}

#[test]
fn a_refused_tool_call_renders_the_name_it_tried() {
    for (code, kind) in [
        (ErrorCode::OutOfScopeTool, ErrorKind::OutOfScopeTool),
        (ErrorCode::UnboundTool, ErrorKind::UnboundTool),
        (ErrorCode::ModelOnlyTool, ErrorKind::ModelOnlyTool),
    ] {
        let refusal = error(code, &[(FactKey::Name, Value::from("echo"))]);
        assert_eq!(error_kind(&refusal), kind, "{code:?}");
        assert_eq!(
            error_fields(&refusal),
            vec![string("name", "echo")],
            "{code:?}"
        );
    }
}

#[test]
fn a_code_without_table_fields_renders_none_whatever_facts_it_holds() {
    let facts = [
        (FactKey::Name, Value::from("echo")),
        (FactKey::Reason, Value::from("why")),
        (FactKey::Path, Value::from("a.md")),
    ];
    for (code, kind) in [
        (ErrorCode::ToolSchemaInvalid, ErrorKind::Internal),
        (ErrorCode::Substitution, ErrorKind::Lua),
        (ErrorCode::LuaQuota, ErrorKind::Lua),
        (ErrorCode::LuaCompile, ErrorKind::Lua),
        (ErrorCode::ToolOther, ErrorKind::Tool),
        (ErrorCode::ModelRequired, ErrorKind::ModelRequired),
        (ErrorCode::Determinism, ErrorKind::Internal),
        (ErrorCode::StructureInvalid, ErrorKind::Internal),
    ] {
        assert_eq!(
            rendered(&error(code, &facts)),
            (kind, "the message".to_owned(), Vec::new()),
            "{code:?}"
        );
    }
}
