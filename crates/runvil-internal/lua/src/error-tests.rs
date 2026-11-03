//! Tests for the crate's failure helpers: an `mlua` failure's text, a
//! refusal that crosses the `mlua` boundary as its typed value, and a tool
//! failure's message and code.

use std::sync::Arc;

use runvil_types::error::{Error, ErrorCode, FactKey};
use runvil_types::tools::tool_error;
use runvil_vfs::VfsError;
use serde_json::Value;

use super::{author_refusal, lua, lua_runtime, tool};

#[test]
fn an_mlua_failure_is_a_lua_runtime_failure_with_its_text() {
    let error = lua(mlua::Error::RuntimeError("boom".to_owned()));
    assert_eq!(error, lua_runtime("runtime error: boom"));
    assert_eq!(error.prose, ErrorCode::LuaRuntime.prose());
}

#[test]
fn a_refusal_crosses_the_mlua_boundary_as_its_typed_failure() {
    let raised = author_refusal("a refused edit".to_owned());
    let expected = lua_runtime("a refused edit");
    assert_eq!(raised.downcast_ref::<Error>(), Some(&expected));
    let framed = mlua::Error::CallbackError {
        traceback: "stack traceback:".to_owned(),
        cause: Arc::new(raised),
    };
    assert_eq!(
        framed.downcast_ref::<Error>(),
        Some(&expected),
        "a callback frame keeps the typed failure"
    );
}

#[test]
fn a_tool_failure_keeps_the_tools_error_under_a_message_that_names_the_call() {
    let mut failure = tool_error(ErrorCode::ToolBackend, "the backend refused");
    failure.facts.insert(FactKey::Path, Value::from("a.md"));
    let error = tool(failure.clone());
    assert_eq!(error.code, ErrorCode::ToolBackend);
    assert_eq!(
        error.message,
        format!("tool call failure: {}", failure.prose)
    );
    assert_eq!(
        (&error.prose, &error.facts),
        (&failure.prose, &failure.facts)
    );
}

#[test]
fn a_tool_failure_outside_the_tool_codes_is_tool_other() {
    let foreign = crate::store_error(&VfsError::NotFound {
        path: "a.md".to_owned(),
    });
    assert_eq!(tool(foreign).code, ErrorCode::ToolOther);
}
