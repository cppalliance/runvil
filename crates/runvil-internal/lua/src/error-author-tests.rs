//! Tests for author prose: its shapes with and without a line, the failure
//! text it names, the file a VM names, and which failures crossing into a
//! VM's Lua get it.

use std::num::NonZeroU32;

use mlua::Lua;
use runvil_types::error::{ErrorCode, Location};
use runvil_types::ids::TaskId;

use super::{authored, authored_at, failure_text, prompt_file, set_prompt_file};
use crate::error::{internal, lua_runtime, task_not_owned, tool_loop_exhausted};

/// The line `n`.
fn line(n: u32) -> NonZeroU32 {
    NonZeroU32::new(n).expect("tests pass non-zero lines")
}

#[test]
fn author_prose_names_the_file_and_the_line_when_it_is_known() {
    let located = authored_at(
        lua_runtime("the message"),
        "agents/a.md",
        Some(line(12)),
        "boom",
    );
    assert_eq!(
        located.prose,
        format!("agents/a.md:12: boom. {}", ErrorCode::LuaRuntime.prose())
    );
    assert_eq!(located.at, Some(Location::line("agents/a.md", line(12))));
    assert_eq!(located.message, "the message");
    let unlocated = authored_at(lua_runtime("the message"), "agents/a.md", None, "boom");
    assert_eq!(
        unlocated.prose,
        format!("agents/a.md: boom. {}", ErrorCode::LuaRuntime.prose())
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
fn a_failure_crossing_into_lua_gets_author_prose_only_when_the_prompts_lua_caused_it() {
    let lua = Lua::new();
    set_prompt_file(&lua, "agents/a.md");
    let refusal = authored(&lua, lua_runtime("messages must not be empty"));
    assert_eq!(
        refusal.prose,
        format!(
            "agents/a.md: messages must not be empty. {}",
            ErrorCode::LuaRuntime.prose()
        )
    );
    assert_eq!(refusal.message, "messages must not be empty");
    let task: TaskId = "0.1".parse().expect("the test task id parses");
    assert_eq!(
        authored(&lua, task_not_owned(&task)).prose,
        format!(
            "agents/a.md: task `0.1` is not a task this chain owns. {}",
            ErrorCode::TaskNotOwned.prose()
        )
    );
    assert_eq!(
        authored(&lua, tool_loop_exhausted()),
        tool_loop_exhausted(),
        "a tool failure keeps its remedy prose"
    );
    let located = authored_at(lua_runtime("boom"), "agents/a.md", Some(line(4)), "boom");
    assert_eq!(
        authored(&lua, located.clone()),
        located,
        "a located failure keeps its prose"
    );
    let fault = internal("a test invariant broke");
    assert_eq!(authored(&lua, fault.clone()), fault);
    assert_eq!(fault.prose, ErrorCode::Internal.prose());
}
