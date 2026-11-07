//! Compiled programs and their failures: bytecode round trips, chunk-line
//! mapping to prompt lines, compile and runtime error reporting, and reads
//! that fail closed on a poisoned lock.

use super::*;
use crate::adhoc::AdhocTools;
use crate::chunk_line::map_chunk_line_to_absolute;

#[test]
fn lua_program_retains_source_and_round_trips_bytecode() {
    let source = "return greeting .. ' world'";
    let program = LuaProgram::compile(
        source,
        "prompt",
        "section Gather prologue",
        NonZeroU32::new(1).expect("compile source line is non-zero"),
        &null_emitter(),
        "Gather",
    )
    .expect("valid Lua must compile");
    assert_eq!(program.source(), source);

    for greeting in ["hello", "goodbye"] {
        let lua = Lua::new();
        lua.globals()
            .set("greeting", greeting)
            .expect("the test global must install");
        let function = program.load(&lua).expect("bytecode must load");
        let returned: String = function.call(()).expect("bytecode must execute");
        assert_eq!(returned, format!("{greeting} world"));
    }
}

#[test]
fn runtime_assert_failure_reports_chunk_name_and_line() {
    let location = "section `Web Search` epilog";
    let program = LuaProgram::compile(
        "local x = 1\nassert(false)\nreturn x",
        "prompt",
        location,
        NonZeroU32::new(1).expect("compile source line is non-zero"),
        &null_emitter(),
        "Web Search",
    )
    .expect("valid Lua must compile");
    let lua = Lua::new();
    let function = program.load(&lua).expect("bytecode must load");
    let error = function
        .call::<()>(())
        .expect_err("assert(false) must fail at runtime");
    let message = error.to_string();
    assert!(
        message.contains(r#"[string "line 1"]:2:"#),
        "runtime error must name the chunk by its first prompt line and give the failing line: {message}"
    );
    assert!(
        !message.contains(location),
        "the display location must not be the chunk name: {message}"
    );
    assert!(
        !message.contains("?:"),
        "stripped debug info must not leave '?:' in the traceback: {message}"
    );
}

#[test]
fn adhoc_tools_list_and_lookup_reads_fail_closed_on_poison() {
    let adhoc = AdhocTools::default();
    let info = runvil_types::tools::ToolInfo::new(
        runvil_types::tools::ToolId::parse("adhoc/grab").expect("a valid adhoc id"),
        "Grab a value",
        serde_json::json!({ "type": "object", "properties": {} }),
    );
    adhoc
        .register(crate::ToolBinding::from_info("grab", &info))
        .expect("registration succeeds before the poison");
    let handle = adhoc.entries_handle();
    let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = handle.lock().expect("first lock is not poisoned");
        panic!("poison the adhoc tools registry");
    }));
    assert!(poisoned.is_err(), "the panic must poison the registry");

    let listed = adhoc
        .bindings()
        .expect_err("list reads must fail on a poisoned registry");
    let looked_up = adhoc
        .by_id(&runvil_types::tools::ToolId::parse("adhoc/grab").expect("a valid adhoc id"))
        .expect_err("lookups must fail on a poisoned registry");
    for error in [listed, looked_up] {
        assert!(
            error
                .to_string()
                .contains("adhoc tools registry was poisoned"),
            "the concrete poison error must surface: {error}"
        );
    }
}

#[test]
fn map_chunk_line_to_absolute_rewrites_line_numbers() {
    let location = "section `Web Search` epilog";
    let msg = r#"[string "line 50"]:2: assertion failed!"#;
    let result = map_chunk_line_to_absolute(
        msg,
        &[(
            "line 50",
            location,
            NonZeroU32::new(50).expect("50 is non-zero"),
        )],
    );
    assert_eq!(
        result,
        (
            "section `Web Search` epilog:51: section `Web Search` epilog:51: assertion failed!"
                .to_owned(),
            NonZeroU32::new(51)
        )
    );
}

#[test]
fn map_chunk_line_to_absolute_only_rewrites_matching_chunk() {
    let msg = r#"[string "line 50"]:51: assertion failed!
stack traceback:
        [string "line 22"]:3: in main chunk"#;
    let (result, line) = map_chunk_line_to_absolute(
        msg,
        &[(
            "line 22",
            "section `Main` prologue",
            NonZeroU32::new(22).expect("22 is non-zero"),
        )],
    );
    assert_eq!(
        line,
        NonZeroU32::new(24),
        "the tag's line is the first rewritten"
    );
    assert!(
        result.contains("[string \"line 50\"]:51:"),
        "child absolute line must stay intact: {result}"
    );
    assert!(
        result.starts_with("section `Main` prologue:24: ")
            && result.contains("\n        section `Main` prologue:24: in main chunk"),
        "parent chunk line must map with parent source_line: {result}"
    );
    assert!(
        !result.contains("[string \"line 22\"]"),
        "parent chunk-relative marker must be rewritten: {result}"
    );
}

#[test]
fn map_chunk_line_to_absolute_leaves_an_already_mapped_marker_alone() {
    let lib = (
        "line 50",
        "section `lib` prologue",
        NonZeroU32::new(50).expect("50 is non-zero"),
    );
    let (child, _) = map_chunk_line_to_absolute(r#"[string "line 50"]:2: boom"#, &[lib]);
    assert_eq!(
        child,
        "section `lib` prologue:51: section `lib` prologue:51: boom"
    );
    let parent = map_chunk_line_to_absolute(&child, &[lib]);
    assert_eq!(
        parent,
        (child, None),
        "a second pass through the same owner must not rewrite line 51 again"
    );
}

#[test]
fn map_chunk_line_to_absolute_keeps_original_digits_on_overflow() {
    // source_line + chunk_line - 1 must not wrap; on overflow the original
    // chunk-relative digits are preserved rather than a wrong absolute line.
    let msg = r#"[string "x"]:5: boom"#;
    let (result, line) =
        map_chunk_line_to_absolute(msg, &[("x", "section `x` prologue", NonZeroU32::MAX)]);
    assert_eq!(line, None, "an overflowing mapping names no line");
    assert!(
        result.contains(r#"[string "x"]:5:"#),
        "overflowing mapping must keep the original line 5: {result}"
    );
    assert!(
        !result.contains(":4294967300:"),
        "no wrapped absolute line may appear: {result}"
    );
}

#[test]
fn map_chunk_line_to_absolute_no_match_passthrough() {
    let msg = "some other error without chunk info";
    let result = map_chunk_line_to_absolute(
        msg,
        &[(
            "line 10",
            "section `Main` prologue",
            NonZeroU32::new(10).expect("10 is non-zero"),
        )],
    );
    assert_eq!(result, (msg.to_owned(), None));
}

#[test]
fn map_chunk_line_to_absolute_maps_each_owners_markers_once_under_one_tag() {
    let msg = r#"[string "line 50"]:2: boom
stack traceback:
        [string "line 50"]:2: in function 'boom'
        [string "line 30"]:3: in main chunk"#;
    let owners = [
        (
            "line 30",
            "caller",
            NonZeroU32::new(30).expect("30 is non-zero"),
        ),
        (
            "line 50",
            "lib",
            NonZeroU32::new(50).expect("50 is non-zero"),
        ),
    ];
    assert_eq!(
        map_chunk_line_to_absolute(msg, &owners),
        (
            "lib:51: lib:51: boom
stack traceback:
        lib:51: in function 'boom'
        caller:32: in main chunk"
                .to_owned(),
            NonZeroU32::new(51)
        )
    );
}

#[test]
fn runtime_error_maps_to_absolute_prompt_line() {
    let location = "section `Web Search` epilog";
    let source_line = NonZeroU32::new(50).expect("50 is non-zero");
    let program = LuaProgram::compile(
        "local x = 1\nassert(false)\nreturn x",
        "prompt",
        location,
        source_line,
        &null_emitter(),
        "Web Search",
    )
    .expect("valid Lua must compile");

    let lua = Lua::new();
    let function = program.load(&lua).expect("bytecode must load");
    let raw_error = function
        .call::<()>(())
        .expect_err("assert(false) must fail at runtime");

    let mapped = program.map_runtime_error(&raw_error);
    let msg = mapped.to_string();
    // chunk line 2 + source_line 50 - 1 = 51
    assert!(
        msg.contains(":51:"),
        "mapped error must contain absolute line 51: {msg}"
    );
    assert!(
        msg.contains(location),
        "mapped error must preserve the chunk name: {msg}"
    );
}

#[test]
fn a_location_longer_than_lua_keeps_in_a_chunk_name_still_maps() {
    // Lua keeps a string chunk name whole only under 45 bytes.
    let location = "section `Shared Research Helper Functions` (line 1200) prologue";
    assert!(location.len() >= 60, "the fixture must exceed Lua's cut");
    let program = LuaProgram::compile(
        "assert(false)",
        "prompt",
        location,
        NonZeroU32::new(40).expect("40 is non-zero"),
        &null_emitter(),
        "Shared Research Helper Functions",
    )
    .expect("valid Lua must compile");
    let lua = Lua::new();
    let raw_error = program
        .load(&lua)
        .expect("bytecode must load")
        .call::<()>(())
        .expect_err("assert(false) must fail at runtime");
    let mapped = program.map_runtime_error(&raw_error).to_string();
    assert!(
        mapped.contains(&format!("{location}:40:")),
        "the long location must map to absolute line 40: {mapped}"
    );
}

#[test]
fn a_block_failure_names_the_vms_file_and_its_mapped_line() {
    let vm = TestVm::new().shims().build();
    crate::error::set_prompt_file(vm.lua(), "agents/a.md");
    let error = match vm.start_block_coro(&program("local x = 1\nerror('boom')")) {
        Err(error) => error,
        other => panic!("the block must fail, got {other:?}"),
    };
    assert_eq!(error.code, ErrorCode::LuaRuntime);
    assert!(
        error
            .message
            .starts_with("test program:2: runtime error: test program:2: boom"),
        "the message keeps its mapped text: {}",
        error.message
    );
    assert_eq!(
        error.at,
        Some(runvil_types::error::SourceLoc::line(
            "agents/a.md",
            NonZeroU32::new(2).expect("2 is non-zero")
        ))
    );
    assert_eq!(
        error.prose,
        format!("boom. {}", ErrorCode::LuaRuntime.prose())
    );
    vm.teardown();
}

#[test]
fn malformed_lua_reports_its_region_and_the_diagnostic() {
    let source = "local secret =\nreturn secret";
    let location = "section Gather prologue";
    let error = LuaProgram::compile(
        source,
        "prompt",
        location,
        NonZeroU32::new(1).expect("compile source line is non-zero"),
        &null_emitter(),
        "Gather",
    )
    .expect_err("malformed Lua must not compile");

    assert_eq!(error.code, ErrorCode::LuaCompile);
    assert_eq!(
        error.facts.get(&runvil_types::error::ErrorFact::Section),
        Some(&Json::from(location))
    );
    assert!(
        error.message.contains(r#"[string "line 1"]"#),
        "the Lua diagnostic must name the chunk by its first prompt line: {error}"
    );
    assert!(
        error.to_string().contains(location),
        "the displayed error must identify its source region"
    );
}

#[test]
fn lua_compilation_reports_are_ordered_exact_and_payload_free() {
    let recorder = Recorder::default();
    let source = "return 'private source payload'";
    let location = "private/location";
    LuaProgram::compile(
        source,
        "prompt",
        location,
        NonZeroU32::new(1).expect("compile source line is non-zero"),
        recorder.emitter(),
        "Gather",
    )
    .expect("valid Lua must compile");
    assert_eq!(
        recorder.observations(),
        vec![
            (
                "Gather".to_owned(),
                boundary::LUA_COMPILATION_STARTED.clone(),
            ),
            (
                "Gather".to_owned(),
                boundary::LUA_COMPILATION_SUCCEEDED.clone(),
            ),
        ]
    );
    assert_identity_only(&recorder);

    let recorder = Recorder::default();
    LuaProgram::compile(
        "local private =",
        "prompt",
        location,
        NonZeroU32::new(1).expect("compile source line is non-zero"),
        recorder.emitter(),
        "Gather",
    )
    .expect_err("malformed Lua must fail");
    assert_eq!(
        recorder.observations(),
        vec![
            (
                "Gather".to_owned(),
                boundary::LUA_COMPILATION_STARTED.clone(),
            ),
            (
                "Gather".to_owned(),
                boundary::LUA_COMPILATION_FAILED.clone(),
            ),
        ]
    );
    assert_identity_only(&recorder);
}
