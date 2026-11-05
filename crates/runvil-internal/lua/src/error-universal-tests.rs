//! Tests for the failures this crate shares with the Engine: each one's
//! text, its facts, and the table kind and fields it renders.

use std::borrow::Cow;
use std::num::NonZeroU32;

use runvil_types::error::{ErrorCode, FactKey, Location};
use runvil_types::ids::TaskId;
use serde_json::Value;

use super::*;
use crate::error_value::{ErrorField, ErrorKind, error_fields, error_kind};

/// The string field `name` with `value`.
fn string(name: &str, value: &str) -> (String, ErrorField) {
    (name.to_owned(), ErrorField::String(value.to_owned()))
}

/// The task id `path` names.
fn task(path: &str) -> TaskId {
    path.parse().expect("the test task id parses")
}

#[test]
fn a_lua_message_is_a_lua_runtime_failure_with_the_default_prose() {
    let error = lua_runtime("a refused edit");
    assert_eq!(error.code, ErrorCode::LuaRuntime);
    assert_eq!(error.message, "a refused edit");
    assert_eq!(error.prose, ErrorCode::LuaRuntime.prose());
    assert_eq!(error_kind(&error), ErrorKind::Lua);
}

#[test]
fn a_compile_failure_names_its_region_as_its_section() {
    let region = "section `S` prologue";
    let error = lua_compile(region, 7, "syntax error near '='");
    assert_eq!(error.code, ErrorCode::LuaCompile);
    assert_eq!(
        error.message,
        "lua compilation error at section `S` prologue (line 7): syntax error near '='"
    );
    assert_eq!(
        error.facts.get(&FactKey::Section),
        Some(&Value::from(region))
    );
    assert_eq!(
        (error_kind(&error), error_fields(&error)),
        (ErrorKind::Lua, Vec::new())
    );
}

#[test]
fn the_typed_failures_keep_their_text_and_name_their_facts() {
    for (resource, text, tag) in [
        (
            QuotaResource::LogEvent,
            "lua log event quota exceeded",
            "log_event",
        ),
        (
            QuotaResource::LogByte,
            "lua log byte quota exceeded",
            "log_byte",
        ),
    ] {
        let quota = lua_quota(resource);
        assert_eq!(quota.message, text);
        assert_eq!(quota.facts.get(&FactKey::Reason), Some(&Value::from(tag)));
        assert_eq!(error_kind(&quota), ErrorKind::Lua);
    }
    let exhausted = context_exhausted("manual");
    assert_eq!(exhausted.message, "context exhausted: manual");
    assert_eq!(error_fields(&exhausted), vec![string("reason", "manual")]);
    assert_eq!(
        tool_loop_exhausted().message,
        "tool-call loop did not converge"
    );
}

#[test]
fn a_task_failure_names_its_task() {
    for (error, kind) in [
        (task_not_owned(&task("0.1")), ErrorKind::TaskNotOwned),
        (task_consumed(&task("0.1")), ErrorKind::TaskConsumed),
        (task_canceled(&task("0.1")), ErrorKind::Canceled),
    ] {
        assert_eq!(error.facts.get(&FactKey::Task), Some(&Value::from("0.1")));
        assert_eq!(
            (error_kind(&error), error_fields(&error)),
            (kind, vec![string("task", "0.1")]),
            "{error:?}"
        );
    }
    assert_eq!(
        task_canceled(&task("0.1")).message,
        "task `0.1` was canceled"
    );
}

#[test]
fn an_internal_fault_keeps_its_message_whole_and_its_site() {
    let message = format!("tool call counts: id {:?} could not be read", "web/fetch");
    let site = std::panic::Location::caller();
    let error = internal_at(Cow::Owned(message.clone()), site);
    assert_eq!(error.code, ErrorCode::Internal);
    assert_eq!(
        error.message,
        format!("internal invariant violated: {message}")
    );
    assert_eq!(error.prose, ErrorCode::Internal.prose());
    assert_eq!(
        error.at,
        Some(Location::line(
            site.file(),
            NonZeroU32::new(site.line()).expect("a line is non-zero")
        ))
    );
    assert_eq!(error_kind(&error), ErrorKind::Internal);
}
