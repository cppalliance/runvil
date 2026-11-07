//! Tests for the crate's failure helpers: an `mlua` failure's text, a
//! refusal that crosses the `mlua` boundary as its typed value, and a tool
//! failure's message and code.
//!
//! Also where a failure crossing into a VM's Lua is placed and its author
//! prose: the prose's shape, the failure text it names, the file and the
//! call line a VM holds, the author's line on the running stack, and which
//! failures crossing into a VM's Lua get the prose. And the failures this
//! crate shares with the Engine: each one's text, its facts, and the table
//! kind and fields it renders.

use std::borrow::Cow;
use std::num::NonZeroU32;
use std::sync::Arc;

use mlua::{Function, Lua};
use runvil_types::error::{Error, ErrorCode, FactKey, Location, place};
use runvil_types::ids::TaskId;
use runvil_types::tools::tool_error;
use runvil_vfs::VfsError;
use serde_json::Value;

use super::{
    QuotaResource, author_line, author_refusal, authored, authored_at, call_line,
    context_exhausted, failure_text, internal, internal_at, lua, lua_compile, lua_quota,
    lua_runtime, prompt_file, set_call_line, set_prompt_file, task_canceled, task_consumed,
    task_not_owned, tool, tool_loop_exhausted,
};
use crate::error_value::{ErrorField, ErrorKind, error_fields, error_kind};

/// The line `n`.
fn line(n: u32) -> NonZeroU32 {
    NonZeroU32::new(n).expect("tests pass non-zero lines")
}

/// The string field `name` with `value`.
fn string(name: &str, value: &str) -> (String, ErrorField) {
    (name.to_owned(), ErrorField::String(value.to_owned()))
}

/// The task id `path` names.
fn task(path: &str) -> TaskId {
    path.parse().expect("the test task id parses")
}

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

#[test]
fn author_prose_names_what_failed_and_the_line_becomes_the_location() {
    let located = authored_at(
        lua_runtime("the message"),
        "agents/a.md",
        Some(line(12)),
        "boom",
    );
    assert_eq!(
        located.prose,
        format!("boom. {}", ErrorCode::LuaRuntime.prose())
    );
    assert_eq!(located.at, Some(Location::line("agents/a.md", line(12))));
    assert_eq!(located.message, "the message");
    let unlocated = authored_at(lua_runtime("the message"), "agents/a.md", None, "boom");
    assert_eq!(
        unlocated.prose, located.prose,
        "the prose never names a place"
    );
    assert_eq!(unlocated.at, None);
}

#[test]
fn the_failure_text_drops_the_positions_in_front_and_the_traceback() {
    assert_eq!(
        failure_text("[string \"line 3\"]:2: boom\nstack traceback:\n\t[C]: in ?"),
        "boom"
    );
    assert_eq!(
        failure_text("[string \"line 3\"]:2: [string \"line 9\"]:1: nested"),
        "nested"
    );
    assert_eq!(failure_text("a plain refusal"), "a plain refusal");
    assert_eq!(
        failure_text("[string \"line 3\"]: no line"),
        "[string \"line 3\"]: no line",
        "a marker with no line is no position"
    );
    assert_eq!(failure_text("a refusal\n"), "a refusal");
}

#[test]
fn a_vm_names_the_file_it_was_given_or_prompt() {
    let lua = Lua::new();
    assert_eq!(prompt_file(&lua), "prompt");
    set_prompt_file(&lua, "agents/a.md");
    assert_eq!(prompt_file(&lua), "agents/a.md");
}

#[test]
fn the_call_line_is_the_latest_recorded_and_none_clears_it() {
    let lua = Lua::new();
    assert_eq!(call_line(&lua), None);
    set_call_line(&lua, Some(line(7)));
    assert_eq!(call_line(&lua), Some(line(7)));
    assert_eq!(prompt_file(&lua), "prompt", "a call line names no file");
    set_prompt_file(&lua, "agents/a.md");
    assert_eq!(call_line(&lua), Some(line(7)), "naming the file keeps it");
    set_call_line(&lua, None);
    assert_eq!(call_line(&lua), None);
    assert_eq!(prompt_file(&lua), "agents/a.md");
}

#[test]
fn the_author_line_is_the_innermost_prompt_block_frames() {
    let lua = Lua::new();
    let probe = lua
        .create_function(|lua, ()| Ok(author_line(lua).map(NonZeroU32::get)))
        .expect("the probe builds");
    lua.globals()
        .set("probe", probe)
        .expect("the probe installs");
    let helper: Function = lua
        .load("return function() return probe() end")
        .set_name("@helper.lua")
        .eval()
        .expect("the helper builds");
    lua.globals()
        .set("helper", helper)
        .expect("the helper installs");
    let (direct, through_helper, nested): (u32, u32, u32) = lua
        .load("local x = 1\nreturn probe(), helper(), (function()\n  return probe()\nend)()")
        .set_name("line 40")
        .eval()
        .expect("the block runs");
    assert_eq!(
        (direct, through_helper, nested),
        (41, 41, 42),
        "a frame of another chunk is skipped, and a nested function reports its own line"
    );
    let unnamed: Option<u32> = lua
        .load("return probe()")
        .set_name("probe")
        .eval()
        .expect("the probe chunk runs");
    assert_eq!(unnamed, None, "a chunk that is no prompt block has no line");
    assert_eq!(author_line(&lua), None, "no Lua frame runs outside a call");
}

#[test]
fn a_failure_crossing_into_lua_is_placed_and_gets_author_prose_when_the_prompts_lua_caused_it() {
    let lua = Lua::new();
    set_prompt_file(&lua, "agents/a.md");
    set_call_line(&lua, Some(line(9)));
    let call = Some(Location::line("agents/a.md", line(9)));
    let refusal = authored(&lua, lua_runtime("messages must not be empty"));
    assert_eq!(
        refusal.prose,
        format!(
            "messages must not be empty. {}",
            ErrorCode::LuaRuntime.prose()
        )
    );
    assert_eq!(
        refusal.at, call,
        "outside any Lua call, the call line places it"
    );
    assert_eq!(refusal.message, "messages must not be empty");
    let task: TaskId = "0.1".parse().expect("the test task id parses");
    assert_eq!(
        authored(&lua, task_not_owned(&task)).prose,
        format!(
            "task `0.1` is not a task this chain owns. {}",
            ErrorCode::TaskNotOwned.prose()
        )
    );
    let exhausted = authored(&lua, tool_loop_exhausted());
    assert_eq!(
        exhausted.prose,
        tool_loop_exhausted().prose,
        "a tool failure keeps its remedy prose"
    );
    assert_eq!(exhausted.at, call, "and is placed all the same");
    let fault = internal("a test invariant broke");
    assert_eq!(
        authored(&lua, fault.clone()),
        fault,
        "an internal failure keeps its Rust location"
    );
    assert_eq!(fault.prose, ErrorCode::Internal.prose());
}

#[test]
fn an_authored_failure_placed_before_it_crosses_keeps_its_place_and_gets_author_prose() {
    let lua = Lua::new();
    set_prompt_file(&lua, "agents/a.md");
    set_call_line(&lua, Some(line(9)));
    let placed = place(lua_runtime("boom"), "agents/a.md", Some(line(4)));
    assert_eq!(placed.prose, ErrorCode::LuaRuntime.prose());
    let crossed = authored(&lua, placed);
    assert_eq!(
        crossed.at,
        Some(Location::line("agents/a.md", line(4))),
        "the first placement wins"
    );
    assert_eq!(
        crossed.prose,
        format!("boom. {}", ErrorCode::LuaRuntime.prose())
    );
    assert_eq!(
        authored(&lua, crossed.clone()),
        crossed,
        "crossing again changes nothing"
    );
}

#[test]
fn a_failure_crossing_inside_a_callback_is_placed_on_the_author_frames_line() {
    let lua = Lua::new();
    set_call_line(&lua, Some(line(9)));
    let cross = lua
        .create_function(|lua, ()| {
            Ok(authored(lua, lua_runtime("boom"))
                .at
                .map(|at| at.start_line.get()))
        })
        .expect("the callback builds");
    lua.globals()
        .set("cross", cross)
        .expect("the callback installs");
    let at: Option<u32> = lua
        .load("local x = 1\nreturn (cross())")
        .set_name("line 20")
        .eval()
        .expect("the block runs");
    assert_eq!(
        at,
        Some(21),
        "the running author frame wins over the call line"
    );
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
