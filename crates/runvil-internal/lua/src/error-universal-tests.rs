//! Tests for the crate boundary: each of this crate's errors, and each
//! raised error table, becomes the universal failure it stands in for,
//! with the same text, and renders the table the old kind rendered.

use std::collections::BTreeMap;
use std::num::NonZeroU32;

use runvil_types::error::{Error as Universal, ErrorCode, FactKey, Location};
use runvil_vfs::{PathReason, VfsError};
use serde_json::Value;

use super::*;
use crate::error_value::{
    ErrorField, ErrorKind, Raised, error_fields, error_kind, error_table_message,
    store_error_value_fields,
};

/// A raised table of `kind` with `message` and `fields`, across the
/// boundary.
fn raised(kind: ErrorKind, message: &str, fields: &[(&str, ErrorField)]) -> Universal {
    into_universal(Error::Raised(Raised {
        kind,
        message: message.to_owned(),
        fields: fields
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone()))
            .collect(),
    }))
}

/// The string field `value`.
fn string(value: &str) -> ErrorField {
    ErrorField::String(value.to_owned())
}

/// The task id `path` names.
fn task(path: &str) -> TaskId {
    path.parse().expect("the test task id parses")
}

#[test]
fn a_lua_message_and_a_runtime_failure_are_lua_runtime_failures() {
    let plain = into_universal(Error::Lua("a refused edit".to_owned()));
    assert_eq!(plain, lua_runtime("a refused edit"));
    assert_eq!(plain.prose, ErrorCode::LuaRuntime.prose());
    let runtime = into_universal(Error::lua(mlua::Error::RuntimeError("boom".to_owned())));
    assert_eq!(runtime.code, ErrorCode::LuaRuntime);
    assert_eq!(runtime.message, "runtime error: boom");
    assert_eq!(error_kind(&runtime), ErrorKind::Lua);
}

#[test]
fn a_compile_failure_is_located_at_its_region_line_and_names_the_region() {
    let region = "section `S` prologue";
    let error = into_universal(Error::LuaCompile {
        location: region.to_owned(),
        source_line: 7,
        lua_source: "x =".to_owned(),
        message: "syntax error near '='".to_owned(),
        cause: Box::new(mlua::Error::SyntaxError {
            message: "syntax error near '='".to_owned(),
            incomplete_input: false,
        }),
    });
    assert_eq!(error.code, ErrorCode::LuaCompile);
    assert_eq!(
        error.message,
        "lua compilation error at section `S` prologue (line 7): syntax error near '='"
    );
    assert_eq!(error.prose, ErrorCode::LuaCompile.prose());
    assert_eq!(
        error.at,
        Some(Location::line(
            region,
            NonZeroU32::new(7).expect("7 is non-zero")
        ))
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
fn the_typed_failures_keep_their_display_text_and_name_their_facts() {
    let quota = into_universal(Error::LuaQuota {
        resource: QuotaResource::LogByte,
    });
    assert_eq!(quota, lua_quota(QuotaResource::LogByte));
    assert_eq!(quota.message, "lua log byte quota exceeded");
    assert_eq!(
        quota.facts.get(&FactKey::Reason),
        Some(&Value::from("log_byte"))
    );
    let exhausted = into_universal(Error::ContextExhausted {
        reason: "manual".to_owned(),
    });
    assert_eq!(exhausted, context_exhausted("manual"));
    assert_eq!(exhausted.message, "context exhausted: manual");
    assert_eq!(
        error_fields(&exhausted),
        vec![("reason".to_owned(), string("manual"))]
    );
    let interruption = into_universal(Error::Interrupted);
    assert_eq!(interruption, interrupted());
    assert_eq!(
        interruption.message,
        "interrupted: the run was cancelled or this call was stopped"
    );
    assert_eq!(error_kind(&interruption), ErrorKind::Cancelled);
}

#[test]
fn an_internal_fault_keeps_its_message_whole_and_its_site() {
    let message = format!("tool call counts: id {:?} could not be read", "web/fetch");
    let site = std::panic::Location::caller();
    let error = into_universal(Error::Internal {
        message: Cow::Owned(message.clone()),
        location: site,
    });
    assert_eq!(error.code, ErrorCode::Internal);
    assert_eq!(
        error.message,
        format!("internal invariant violated: {message}")
    );
    assert_eq!(
        error.at,
        Some(Location::line(
            site.file(),
            NonZeroU32::new(site.line()).expect("a line is non-zero")
        ))
    );
    assert_eq!(error_kind(&error), ErrorKind::Internal);
}

#[test]
fn a_tool_failure_keeps_the_tools_error_under_a_message_that_names_the_call() {
    let mut tool = runvil_types::tools::tool_error(ErrorCode::ToolBackend, "the backend refused");
    tool.facts.insert(FactKey::Path, Value::from("a.md"));
    let error = into_universal(Error::tool(tool.clone()));
    assert_eq!(error.code, ErrorCode::ToolBackend);
    assert_eq!(error.message, "tool call failure: the backend refused");
    assert_eq!(error.prose, tool.prose);
    assert_eq!(error.facts, tool.facts);
    assert_eq!(error_kind(&error), ErrorKind::Tool);
    assert_eq!(
        error_table_message(&error),
        "tool call failure: the backend refused"
    );
    assert_eq!(error_fields(&error), Vec::new());
}

#[test]
fn a_tool_failure_outside_the_tool_codes_or_without_a_tool_error_is_tool_other() {
    let foreign = crate::store_error(&VfsError::NotFound {
        path: "a.md".to_owned(),
    });
    let coerced = into_universal(Error::tool(foreign));
    assert_eq!(coerced.code, ErrorCode::ToolOther);
    assert_eq!(
        (error_kind(&coerced), error_fields(&coerced)),
        (ErrorKind::Tool, Vec::new())
    );
    let json = serde_json::from_str::<Value>("{").expect_err("an unclosed object");
    let text = format!("structured tool \"t\" returned invalid JSON: {json}");
    let structured = into_universal(Error::Tool {
        message: text.clone(),
        cause: Box::new(json),
    });
    assert_eq!(structured.code, ErrorCode::ToolOther);
    assert_eq!(structured.message, format!("tool call failure: {text}"));
    assert_eq!(structured.prose, text);
    let table = raised(ErrorKind::Tool, "the backend refused", &[]);
    assert_eq!(table.code, ErrorCode::ToolOther);
    assert_eq!(table.message, "tool call failure: the backend refused");
}

#[test]
fn a_store_failure_keeps_its_message_beside_the_stores_code_and_facts() {
    let cause = VfsError::NotFound {
        path: "a.md".to_owned(),
    };
    let error = into_universal(Error::Store {
        message: "store.read failed: not found: a.md".to_owned(),
        cause: cause.clone(),
    });
    let expected = crate::store_error(&cause);
    assert_eq!((error.code, &error.facts), (expected.code, &expected.facts));
    assert_eq!(error.message, "store.read failed: not found: a.md");
    assert_eq!(error_fields(&error), store_error_value_fields(&cause));
}

#[test]
fn a_raised_cancelled_table_is_its_tasks_cancellation_or_an_interruption() {
    let cancelled = raised(
        ErrorKind::Cancelled,
        "task `0.1` was cancelled",
        &[("task", string("0.1"))],
    );
    assert_eq!(cancelled, task_cancelled(&task("0.1")));
    assert_eq!(
        error_fields(&cancelled),
        vec![("task".to_owned(), string("0.1"))]
    );
    let bare = raised(ErrorKind::Cancelled, "interrupted", &[]);
    assert_eq!(bare, interrupted());
    let unparsed = raised(ErrorKind::Cancelled, "x", &[("task", string("not a task"))]);
    assert_eq!(unparsed, interrupted());
}

#[test]
fn a_raised_table_maps_onto_the_failure_its_kind_names() {
    for reason in ["provider", "manual"] {
        let exhausted = raised(
            ErrorKind::ContextExhausted,
            &format!("context exhausted: {reason}"),
            &[("reason", string(reason))],
        );
        assert_eq!(exhausted, context_exhausted(reason));
    }
    assert_eq!(
        raised(ErrorKind::ContextExhausted, "context exhausted", &[]),
        lua_runtime("context exhausted")
    );
    assert_eq!(
        raised(ErrorKind::ToolLoopExhausted, "x", &[]),
        tool_loop_exhausted()
    );
    let empty = raised(
        ErrorKind::EmptyModelReply,
        "the model returned an empty turn",
        &[("finish_reason", string("length"))],
    );
    assert_eq!(empty.code, ErrorCode::EmptyReply);
    assert_eq!(empty.message, "the model returned an empty turn");
    assert_eq!(empty.prose, "the model returned an empty turn");
    assert_eq!(
        error_fields(&empty),
        vec![("finish_reason".to_owned(), string("length"))]
    );
    let owned = raised(ErrorKind::TaskNotOwned, "x", &[("task", string("0.4"))]);
    assert_eq!(owned, task_not_owned(&task("0.4")));
    let consumed = raised(ErrorKind::TaskConsumed, "x", &[("task", string("0.4"))]);
    assert_eq!(consumed, task_consumed(&task("0.4")));
    assert_eq!(
        raised(ErrorKind::TaskConsumed, "no task", &[]),
        lua_runtime("no task")
    );
    // A model failure, a missing model, or a leak holds none of the
    // structure its failure needs, so the table keeps its message as a Lua
    // failure rather than rebuilding it.
    for kind in [
        ErrorKind::ModelFailure,
        ErrorKind::ModelRequired,
        ErrorKind::TasksLive,
        ErrorKind::OutOfScopeTool,
        ErrorKind::UnboundTool,
        ErrorKind::ModelOnlyTool,
        ErrorKind::Lua,
        ErrorKind::Internal,
    ] {
        assert_eq!(raised(kind, "kept", &[]), lua_runtime("kept"), "{kind:?}");
    }
}

/// One failure of each store variant, with fields a table can carry.
fn one_failure_per_store_variant() -> Vec<VfsError> {
    let path = || "notes.md".to_owned();
    let text = || "why".to_owned();
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
            reason: text(),
        },
        VfsError::ApprovalRequired {
            path: path(),
            reason: text(),
        },
        VfsError::ScopeEnded { path: path() },
        VfsError::ReadOnly { path: path() },
        VfsError::OutsideRoot {
            path: path(),
            detail: text(),
        },
        VfsError::RootProtected { path: path() },
        VfsError::NoStore,
        VfsError::CrossMount {
            path: path(),
            detail: text(),
        },
        VfsError::Io {
            path: path(),
            kind: std::io::ErrorKind::PermissionDenied,
            raw_os_error: Some(13),
            message: text(),
        },
        VfsError::Unsupported {
            path: path(),
            detail: text(),
        },
        VfsError::Conflict {
            path: path(),
            detail: text(),
        },
        VfsError::Backend { message: text() },
    ]
}

#[test]
fn every_store_failure_rebuilds_from_its_raised_table_under_its_own_tag() {
    for failure in one_failure_per_store_variant() {
        let fields = store_error_value_fields(&failure);
        let rebuilt = into_universal(Error::Raised(Raised {
            kind: ErrorKind::Store,
            message: failure.to_string(),
            fields: fields.iter().cloned().collect(),
        }));
        let expected = crate::store_error(&failure);
        assert_eq!(rebuilt.code.tag(), failure.tag(), "{failure:?}");
        assert_eq!(
            (rebuilt.code, &rebuilt.facts),
            (expected.code, &expected.facts),
            "{failure:?}"
        );
        assert_eq!(rebuilt.message, failure.to_string(), "{failure:?}");
        assert_eq!(error_fields(&rebuilt), fields, "{failure:?}");
    }
}

#[test]
fn a_malformed_raised_store_table_degrades_to_the_closest_failure() {
    let backend = raised(
        ErrorKind::Store,
        "gone",
        &[("reason", string("determinism")), ("path", string("a.md"))],
    );
    assert_eq!((backend.code, backend.facts.len()), (ErrorCode::Backend, 0));
    let anchor = raised(ErrorKind::Store, "anchor", &[("reason", string("anchor"))]);
    assert_eq!(
        anchor.facts,
        BTreeMap::from([
            (FactKey::Path, Value::from("")),
            (FactKey::Anchor, Value::from("")),
            (FactKey::Count, Value::from(0)),
        ])
    );
    let invalid = raised(
        ErrorKind::Store,
        "invalid",
        &[
            ("reason", string("invalid_path")),
            ("rule", string("no such rule")),
        ],
    );
    assert_eq!(
        invalid.facts.get(&FactKey::Rule),
        Some(&Value::from(PathReason::Empty.tag()))
    );
    assert_eq!(
        raised(ErrorKind::Store, "untagged", &[]).code,
        ErrorCode::Backend
    );
}
