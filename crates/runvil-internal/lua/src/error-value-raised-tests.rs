//! Tests for error tables in both directions: every table's `code`, a
//! table read back as the universal error it stands for, the code a
//! shim-built table and a table without a code get from their kind, and
//! the error a rendered table keeps beside it.

use mlua::{Function, Lua, Table, Value};
use runvil_types::error::{Error, ErrorCode, FactKey};
use runvil_types::ids::TaskId;

use super::super::tests::every_store_failure;
use super::super::{error_table, install_error_value, retained_error};
use super::raised_from;
use crate::error::{
    QuotaResource, context_exhausted, internal, lua_quota, lua_runtime, task_cancelled,
    task_consumed, task_not_owned, tool, tool_loop_exhausted,
};

/// The task id `path` names.
fn task(path: &str) -> TaskId {
    path.parse().expect("the test task id parses")
}

/// An error with `code`, `message`, a prose of its own, and `facts`.
fn failure(code: ErrorCode, facts: &[(FactKey, serde_json::Value)]) -> Error {
    let mut error = Error::new(code, "the message", "the prose");
    error.facts.extend(facts.iter().cloned());
    error
}

/// Errors across the families whose facts their tables' fields hold.
fn located_by_their_fields() -> Vec<Error> {
    let mut errors = vec![
        context_exhausted("manual"),
        failure(
            ErrorCode::EmptyReply,
            &[(FactKey::FinishReason, "length".into())],
        ),
        failure(ErrorCode::OutOfScopeTool, &[(FactKey::Name, "echo".into())]),
        failure(ErrorCode::UnboundTool, &[(FactKey::Name, "echo".into())]),
        failure(ErrorCode::ModelOnlyTool, &[(FactKey::Name, "echo".into())]),
        failure(
            ErrorCode::TasksLive,
            &[(FactKey::Tasks, serde_json::Value::from(vec!["1", "1.2"]))],
        ),
        task_not_owned(&task("0.4")),
        task_consumed(&task("0.5")),
        task_cancelled(&task("0.6")),
        lua_runtime("a refused edit"),
        tool_loop_exhausted(),
    ];
    errors.extend(
        every_store_failure()
            .iter()
            .map(|(failure, _)| crate::store_error(failure)),
    );
    errors
}

/// The string field `name` of `table`.
fn field(table: &Table, name: &str) -> Option<String> {
    table.raw_get(name).expect("the field reads")
}

/// The shim's `error_value(kind, fields)` capture on `lua`.
fn error_value(lua: &Lua) -> Function {
    install_error_value(lua).expect("the capture builds")
}

/// The table `error_value(kind, fields)` builds, with `fields` as strings.
fn shim_table(lua: &Lua, kind: &str, fields: &[(&str, &str)]) -> Table {
    let given = lua.create_table().expect("the fields build");
    for (name, value) in fields {
        given.raw_set(*name, *value).expect("a field sets");
    }
    error_value(lua)
        .call((kind, given))
        .expect("the shim builds the table")
}

/// What `table` reads back as.
fn read_back(lua: &Lua, table: &Table) -> Error {
    raised_from(lua, &Value::Table(table.clone()))
        .expect("the table reads")
        .expect("the table is an error table")
}

#[test]
fn every_table_carries_its_errors_code_beside_its_kind() {
    let lua = Lua::new();
    let mut errors = located_by_their_fields();
    errors.extend([
        lua_quota(QuotaResource::LogByte),
        tool(failure(ErrorCode::ToolBackend, &[])),
        failure(ErrorCode::Transport, &[]),
        failure(ErrorCode::CallDepthExceeded, &[]),
        internal("a test invariant broke"),
    ]);
    for error in errors {
        let table = error_table(&lua, &error).expect("the table renders");
        assert_eq!(
            field(&table, "code").as_deref(),
            Some(error.code.tag()),
            "{error:?}"
        );
        assert_eq!(
            field(&table, "message").as_deref(),
            Some(error.prose.as_str()),
            "the table's message is the prose: {error:?}"
        );
    }
}

#[test]
fn a_table_read_back_keeps_its_code_prose_and_facts() {
    let lua = Lua::new();
    for error in located_by_their_fields() {
        let table = error_table(&lua, &error).expect("the table renders");
        let back = read_back(&lua, &table);
        assert_eq!(
            (back.code, &back.prose, &back.facts),
            (error.code, &error.prose, &error.facts),
            "{error:?}"
        );
        assert_eq!(back.message, error.prose, "the message is the table's");
    }
}

#[test]
fn a_rendered_table_keeps_its_error_whole_and_a_shim_table_none() {
    let lua = Lua::new();
    let error = context_exhausted("precheck");
    let table = Value::Table(error_table(&lua, &error).expect("the table renders"));
    assert_eq!(
        retained_error(&lua, &table).expect("the table reads"),
        Some(error)
    );
    let shim = Value::Table(shim_table(&lua, "lua", &[("message", "x")]));
    assert_eq!(retained_error(&lua, &shim).expect("the table reads"), None);
    assert_eq!(
        retained_error(&lua, &Value::Integer(1)).expect("a value reads"),
        None
    );
}

#[test]
fn a_shim_built_table_gets_the_code_its_kind_gives_unless_its_fields_name_one() {
    let lua = Lua::new();
    for (kind, fields, code) in [
        ("lua", vec![("message", "x")], ErrorCode::LuaRuntime),
        ("store", vec![("reason", "not_found")], ErrorCode::NotFound),
        ("cancelled", vec![("task", "0.1")], ErrorCode::TaskCancelled),
        ("cancelled", vec![], ErrorCode::Interrupted),
        ("tool_loop_exhausted", vec![], ErrorCode::ToolLoopExhausted),
        ("lua", vec![("code", "lua_quota")], ErrorCode::LuaQuota),
    ] {
        let table = shim_table(&lua, kind, &fields);
        assert_eq!(
            field(&table, "code").as_deref(),
            Some(code.tag()),
            "{kind} {fields:?}"
        );
        assert_eq!(read_back(&lua, &table).code, code, "{kind} {fields:?}");
    }
    let table = shim_table(&lua, "lua", &[("message", "the shim's own text")]);
    let back = read_back(&lua, &table);
    assert_eq!(
        (back.message.as_str(), back.prose.as_str()),
        ("the shim's own text", "the shim's own text")
    );
}

#[test]
fn a_table_with_no_usable_code_reads_back_as_the_failure_its_kind_names() {
    let lua = Lua::new();
    for (kind, fields, code) in [
        (
            "context_exhausted",
            vec![("reason", "manual")],
            ErrorCode::ContextExhausted,
        ),
        ("context_exhausted", vec![], ErrorCode::LuaRuntime),
        (
            "cancelled",
            vec![("task", "not a task")],
            ErrorCode::Interrupted,
        ),
        (
            "task_consumed",
            vec![("task", "0.4")],
            ErrorCode::TaskConsumed,
        ),
        ("task_consumed", vec![], ErrorCode::LuaRuntime),
        ("empty_model_reply", vec![], ErrorCode::EmptyReply),
        ("tool", vec![], ErrorCode::ToolOther),
        ("store", vec![("reason", "determinism")], ErrorCode::Backend),
        ("store", vec![], ErrorCode::Backend),
        ("model_failure", vec![], ErrorCode::LuaRuntime),
        ("internal", vec![], ErrorCode::LuaRuntime),
    ] {
        let table = shim_table(&lua, kind, &fields);
        table
            .raw_set("code", "no such code")
            .expect("the code field sets");
        assert_eq!(read_back(&lua, &table).code, code, "{kind} {fields:?}");
    }
    let table = shim_table(&lua, "cancelled", &[("task", "0.1"), ("message", "gone")]);
    table
        .raw_set("code", Value::Nil)
        .expect("the code field clears");
    let back = read_back(&lua, &table);
    assert_eq!(back.code, ErrorCode::TaskCancelled);
    assert_eq!(back.facts.get(&FactKey::Task), Some(&"0.1".into()));
    assert_eq!(back.message, "gone");
}

#[test]
fn an_authors_own_table_is_no_error_table() {
    let lua = Lua::new();
    let own = lua.create_table().expect("the table builds");
    own.raw_set("kind", "lua").expect("a field sets");
    assert_eq!(
        raised_from(&lua, &Value::Table(own)).expect("the table reads"),
        None
    );
}
