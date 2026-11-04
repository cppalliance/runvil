//! Tests for the compile path's location, the runtime mapping's location
//! and author prose, a quota refusal's among them, the typed failures the
//! mapping keeps, and the prompt line a block's chunk position names.

use std::num::NonZeroU32;

use mlua::Lua;
use runvil_types::emitter::{Emitter, EventSink};
use runvil_types::error::{ErrorCode, FactKey, Location};
use serde_json::Value;

use super::LuaProgram;
use crate::chunk_line::{compile_line, first_prompt_line, map_chunk_line_to_absolute, prompt_line};
use crate::error::{QuotaResource, context_exhausted, internal, lua_quota};

/// The file every program here comes from.
const FILE: &str = "agents/a.md";

/// The region every program here is.
const REGION: &str = "section `S` prologue";

/// The line `n`.
fn line(n: u32) -> NonZeroU32 {
    NonZeroU32::new(n).expect("tests pass non-zero lines")
}

/// `source` compiled as the block of [`REGION`] in [`FILE`] that starts on
/// `start`.
fn compiled(source: &str, start: u32) -> LuaProgram {
    let emitter = Emitter::root(EventSink::default(), "test", false);
    LuaProgram::compile(source, FILE, REGION, line(start), &emitter, "S")
        .expect("the block compiles")
}

/// The failure running `program` in `lua` raises.
fn raised(program: &LuaProgram, lua: &Lua) -> mlua::Error {
    program
        .load(lua)
        .expect("the bytecode loads")
        .call::<()>(())
        .expect_err("the block fails")
}

#[test]
fn a_compile_line_maps_the_chunk_line_or_keeps_the_block_line() {
    let diagnostic = "[string \"line 40\"]:3: unexpected symbol near <eof>";
    assert_eq!(compile_line("line 40", line(40), diagnostic), line(42));
    assert_eq!(
        compile_line("line 41", line(41), diagnostic),
        line(41),
        "another chunk's line is not this block's"
    );
    assert_eq!(compile_line("line 40", line(40), "no chunk line"), line(40));
    let chunk = format!("line {}", u32::MAX);
    assert_eq!(
        compile_line(
            &chunk,
            NonZeroU32::MAX,
            &format!("[string \"{chunk}\"]:2: x")
        ),
        NonZeroU32::MAX,
        "an overflowing mapping keeps the block line"
    );
}

#[test]
fn a_compile_failure_is_located_on_its_diagnostics_line_with_author_prose() {
    let emitter = Emitter::root(EventSink::default(), "test", false);
    let error = LuaProgram::compile(
        "local a = 1\nlocal b =",
        FILE,
        REGION,
        line(11),
        &emitter,
        "S",
    )
    .expect_err("the block must not compile");
    let diagnostic = "[string \"line 11\"]:2: unexpected symbol near <eof>";
    assert_eq!(error.code, ErrorCode::LuaCompile);
    assert_eq!(
        error.message,
        format!("lua compilation error at {REGION} (line 11): syntax error: {diagnostic}")
    );
    assert_eq!(error.at, Some(Location::line(FILE, line(12))));
    assert_eq!(
        error.facts.get(&FactKey::Section),
        Some(&Value::from(REGION))
    );
    assert_eq!(
        error.prose,
        format!(
            "unexpected symbol near <eof>. {}",
            ErrorCode::LuaCompile.prose()
        )
    );
}

#[test]
fn a_block_keeps_its_source_and_its_lines_behind_the_frame_guard() {
    let source = "local x = 1\nreturn x";
    let program = compiled(source, 30);
    assert_eq!(program.source(), source);
    let lua = Lua::new();
    let raw = raised(&compiled("local x = 1\nerror('boom')", 30), &lua);
    assert!(
        raw.to_string().contains("[string \"line 30\"]:2: boom"),
        "the guard adds no line: {raw}"
    );
}

#[test]
fn a_runtime_failure_is_located_on_its_mapped_line_with_author_prose() {
    let program = compiled("local x = 1\nerror('boom')", 50);
    let lua = Lua::new();
    let raw = raised(&program, &lua);
    let mapped = program.map_runtime_error_among(&raw, &[], FILE);
    let (message, _) =
        map_chunk_line_to_absolute(&raw.to_string(), &[("line 50", REGION, line(50))]);
    assert_eq!(mapped.code, ErrorCode::LuaRuntime);
    assert_eq!(mapped.message, message, "the message keeps its mapped text");
    assert_eq!(mapped.at, Some(Location::line(FILE, line(51))));
    assert_eq!(
        mapped.prose,
        format!("boom. {}", ErrorCode::LuaRuntime.prose())
    );
}

/// The failure running `program` raises in a Lua VM whose global `fail`
/// raises `error` from Rust.
fn raised_through(program: &LuaProgram, error: &runvil_types::error::Error) -> mlua::Error {
    let lua = Lua::new();
    let raise = error.clone();
    let fail = lua
        .create_function(move |_, ()| -> mlua::Result<()> {
            Err(mlua::Error::external(raise.clone()))
        })
        .expect("the callback builds");
    lua.globals()
        .set("fail", fail)
        .expect("the global installs");
    raised(program, &lua)
}

#[test]
fn a_raised_internal_failure_keeps_its_rust_location_through_the_mapping() {
    let fault = internal("a test invariant broke");
    assert!(fault.at.is_some(), "an internal failure is located in Rust");
    let program = compiled("local x = 1\nfail()", 5);
    let mapped = program.map_runtime_error_among(&raised_through(&program, &fault), &[], FILE);
    assert_eq!(
        mapped, fault,
        "it is not recoded at the line that raised it"
    );
}

#[test]
fn a_raised_context_exhaustion_is_placed_on_its_mapped_line() {
    let exhausted = context_exhausted("precheck");
    let program = compiled("local x = 1\nfail()", 5);
    let mapped = program.map_runtime_error_among(&raised_through(&program, &exhausted), &[], FILE);
    assert_eq!(
        (mapped.code, &mapped.message, &mapped.prose),
        (exhausted.code, &exhausted.message, &exhausted.prose),
        "it keeps its own failure and its prose"
    );
    assert_eq!(mapped.at, Some(Location::line(FILE, line(6))));
}

#[test]
fn a_prompt_line_is_named_only_by_a_prompt_blocks_chunk() {
    assert_eq!(prompt_line("line 40", 3), Some(line(42)));
    assert_eq!(prompt_line("line 40", 0), Some(line(39)));
    for other in [
        "line 0",
        "line +4",
        "line ",
        "@pack:web",
        "probe",
        "lines 4",
    ] {
        assert_eq!(prompt_line(other, 3), None, "{other:?}");
    }
    assert_eq!(
        prompt_line(&format!("line {}", u32::MAX), 2),
        None,
        "an overflowing line is none"
    );
}

#[test]
fn the_first_prompt_position_skips_function_markers_and_other_chunks() {
    let traceback = "stack traceback:\n\t[C]: in ?\n\
                     \t@crates/runvil-internal/lua/src/__impl_coro.lua:91: in function 'pcall'\n\
                     \t[string \"probe\"]:3: in function <[string \"line 7\"]:2>\n\
                     \t[string \"line 7\"]:4: in main chunk";
    assert_eq!(first_prompt_line(traceback), Some(line(10)));
    assert_eq!(
        first_prompt_line("in function <[string \"line 7\"]:2>"),
        None,
        "a function marker is no position"
    );
    assert_eq!(first_prompt_line("[string \"line 7\"]:: x"), None);
}

#[test]
fn a_quota_refusal_raised_through_lua_is_recognized_at_the_boundary() {
    let lua = Lua::new();
    let log = lua
        .create_function(|_, ()| -> mlua::Result<()> {
            Err(mlua::Error::external(lua_quota(QuotaResource::LogEvent)))
        })
        .expect("the callback builds");
    lua.globals().set("log", log).expect("the global installs");
    let program = compiled("local x = 1\nlog()", 7);
    let mapped = program.map_runtime_error_among(&raised(&program, &lua), &[], FILE);
    let quota = lua_quota(QuotaResource::LogEvent);
    assert_eq!(
        (mapped.code, &mapped.message, &mapped.facts),
        (ErrorCode::LuaQuota, &quota.message, &quota.facts)
    );
    assert_eq!(mapped.at, Some(Location::line(FILE, line(8))));
    assert_eq!(
        mapped.prose,
        format!(
            "lua log event quota exceeded. {}",
            ErrorCode::LuaQuota.prose()
        )
    );
    let mimic = compiled("error('lua log event quota exceeded')", 7);
    assert_eq!(
        mimic
            .map_runtime_error_among(&raised(&mimic, &lua), &[], FILE)
            .code,
        ErrorCode::LuaRuntime,
        "an author who raises the same text fails as a runtime error"
    );
}
