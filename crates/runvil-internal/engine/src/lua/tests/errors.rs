//! The failure contract at the coroutine boundary: every failure that
//! reaches Lua is a `{ kind, message, ... }` table whose `tostring` is the
//! message, a Rust-raised error answered through the envelope arrives in
//! the same shape, an Engine failure answered there renders the table its
//! code names, an author's own raise passes through untouched, a typed
//! error substituted at the boundary keeps its kind, a kept table maps
//! back onto the failure it stands in for, and tracebacks through the shim
//! render the impl frames verbatim.

use std::num::NonZeroU32;

use mlua::MultiValue;

use runvil_lua::LuaProgram;
use runvil_lua::test_support::answer_envelope;
use runvil_types::error::ErrorCode;

use crate::lua::{CoroStep, LuaBlockResult};
use crate::model::ModelSet;
use crate::test_support::recording::null_emitter;

use super::{TEST_FILE, compile_block, scheduler_vm, start, test_models};

#[path = "errors-tables.rs"]
mod tables;

#[test]
fn models_infer_takes_a_prompt_or_a_model_and_a_prompt() {
    // `models.infer(prompt)` and `models.infer(model, prompt)` are the
    // whole signature; a third argument, or a second with no leading
    // handle, raises at the call site rather than being silently dropped.
    let vm = scheduler_vm(&test_models(), None);
    let program = compile_block(
        r#"local h = models.get("fast")
           local out = {}
           for _, call in ipairs({
             function() return models.infer("yo", { temperature = 0 }) end,
             function() return models.infer(h, "yo", { temperature = 0 }) end,
           }) do
             local ok, err = pcall(call)
             assert(not ok, "an argument past the acceptance shape must fail")
             out[#out + 1] = type(err) .. "|" .. tostring(err.kind) .. "|" .. tostring(err)
           end
           return table.concat(out, "\n")"#,
    );
    let message = "models.infer takes (prompt) or (model, prompt)";
    match vm.start_block_coro(&program).expect("the block runs") {
        CoroStep::Done(LuaBlockResult::Returned(Some(text))) => {
            let line = format!("table|lua|{message}");
            assert_eq!(text, [line.clone(), line].join("\n"));
        }
        other => panic!("expected the rejection return, got {other:?}"),
    }
}

#[test]
fn a_shim_argument_error_is_a_table_whose_tostring_is_the_message() {
    // Every failure that reaches Lua is a `{ kind, message, ... }` table:
    // `tostring` (and `..`) gives exactly the message, and a caller that
    // branches reads `kind`. A shim's own argument error is an authoring
    // error, so its kind is `lua`.
    let vm = scheduler_vm(&test_models(), None);
    let program = compile_block(
        r#"local h = models.get("fast")
           local ok, err = pcall(models.infer, h, "yo", { temperature = 0 })
           assert(not ok, "an argument past the prompt must fail")
           return type(err) .. "|" .. tostring(err.kind) .. "|" .. err"#,
    );
    match vm.start_block_coro(&program).expect("the block runs") {
        CoroStep::Done(LuaBlockResult::Returned(Some(text))) => {
            assert_eq!(
                text,
                "table|lua|models.infer takes (prompt) or (model, prompt)"
            );
        }
        other => panic!("expected the rejection return, got {other:?}"),
    }
}

#[test]
fn a_handle_carries_no_call_entry_of_its_own() {
    // A handle is a pure inspectable value: `infer` and `loop` are names
    // it does not have, and the namespace functions are the only way to run
    // a model.
    let vm = scheduler_vm(&test_models(), None);
    let program = compile_block(
        r#"local h = models.get("fast")
           local out = {}
           for _, name in ipairs({ "infer", "loop" }) do
             local ok = pcall(function() return h[name] end)
             out[#out + 1] = tostring(ok)
           end
           return type(h) .. "|" .. table.concat(out, "|")"#,
    );
    match vm.start_block_coro(&program).expect("the block runs") {
        CoroStep::Done(LuaBlockResult::Returned(Some(text))) => {
            assert_eq!(text, "userdata|false|false");
        }
        other => panic!("expected the rejection return, got {other:?}"),
    }
}

#[test]
fn a_failure_envelope_raises_a_table_holding_the_kind_and_fields() {
    // A Rust-raised error answered through the envelope reaches the
    // author's `pcall` in the same shape as a shim raise: `kind` names the
    // failure, the kind's fields appear beside it, and `tostring` is the
    // typed error's prose, a model failure's own remedy.
    for reason in ["provider", "manual"] {
        let vm = scheduler_vm(&ModelSet::default(), None);
        let program = compile_block(
            r#"local ok, err = pcall(models.infer, "hi")
               assert(not ok, "the failure envelope must raise")
               return type(err) .. "|" .. tostring(err.kind) .. "|" .. tostring(err.reason) .. "|" .. tostring(err)"#,
        );
        let CoroStep::Yielded(thread, _values) =
            vm.start_block_coro(&program).expect("the block suspends")
        else {
            panic!("the shim yield must suspend the block");
        };
        let error = crate::error::context_exhausted(reason);
        let display = error.prose.clone();
        assert_eq!(display, ErrorCode::ContextExhausted.prose());
        match vm
            .resume_block_coro_answer(&program, &thread, Err(error))
            .expect("the pcall'd block resumes")
        {
            CoroStep::Done(LuaBlockResult::Returned(Some(text))) => {
                assert_eq!(text, format!("table|context_exhausted|{reason}|{display}"));
            }
            other => panic!("expected the caught failure's rendering, got {other:?}"),
        }
    }
}

#[test]
fn an_engine_callback_failure_caught_by_pcall_is_the_same_error_table() {
    // An Engine call that fails directly from Rust - no envelope, no shim
    // raise - reaches the author's `pcall` as the same `{ kind, message }`
    // table: `kind` is readable at every call site, and `tostring` is the
    // author prose of the failure's own text in the VM's file (mlua's
    // label and appended traceback are not part of it). The
    // `sys` guard fails from a metamethod rather than a call, and
    // `xpcall`'s handler sees the same normalized value.
    let vm = scheduler_vm(&test_models(), None);
    let program = compile_block(
        r#"local ok, err = pcall(models.get, "missing")
           assert(not ok, "a label outside the five roles must fail")
           local first = type(err) .. "|" .. tostring(err.kind) .. "|" .. tostring(err)
           local ok2, err2 = pcall(function() return sys.nothing end)
           assert(not ok2, "an unknown sys field must fail")
           local second = type(err2) .. "|" .. tostring(err2.kind) .. "|" .. tostring(err2)
           local ok3, third = xpcall(models.get, function(e)
             return type(e) .. "|" .. tostring(e.kind)
           end, "missing")
           assert(not ok3, "the handler runs for the callback failure")
           return first .. "\n" .. second .. "\n" .. third"#,
    );
    let prose = |what: &str| format!("{TEST_FILE}: {what}. {}", ErrorCode::LuaRuntime.prose());
    match vm.start_block_coro(&program).expect("the block runs") {
        CoroStep::Done(LuaBlockResult::Returned(Some(text))) => {
            assert_eq!(
                text,
                format!(
                    "table|lua|{}\ntable|lua|{}\ntable|lua",
                    prose(
                        "models.get label \"missing\" is not a model role: expected default, \
                         strong, small, fast or creative"
                    ),
                    prose("unknown sys field 'nothing'")
                )
            );
        }
        other => panic!("expected the caught failures' rendering, got {other:?}"),
    }
}

#[test]
fn the_normalizing_pcall_leaves_lua_values_and_returns_unchanged() {
    // Only a Rust-raised failure is rewritten: an author's string error and
    // an author's own table come back exactly as raised, and a successful
    // call keeps every return value, nils included.
    let vm = scheduler_vm(&ModelSet::default(), None);
    let program = compile_block(
        r##"local ok, err = pcall(error, "plain", 0)
            assert(not ok and err == "plain", "a string error passes through")
            local own = { kind = "custom" }
            local ok2, err2 = pcall(error, own)
            assert(not ok2 and err2 == own, "an author's table passes through")
            local n = select("#", pcall(function() return 1, nil, 3 end))
            assert(n == 4, "pcall keeps the return count")
            local ok3, x, y, z = pcall(function() return 1, nil, 3 end)
            assert(ok3 and x == 1 and y == nil and z == 3, "pcall keeps the returns")
            return "unchanged""##,
    );
    match vm.start_block_coro(&program).expect("the block runs") {
        CoroStep::Done(LuaBlockResult::Returned(Some(text))) => assert_eq!(text, "unchanged"),
        other => panic!("expected the pass-through return, got {other:?}"),
    }
}

#[test]
fn an_authors_plain_table_with_a_kind_is_not_read_back_as_a_raise() {
    // The read-back recognizes an error table by the shared metatable, not
    // by shape: an author's own `error({ kind = ..., message = ... })` is
    // never mistaken for a shim raise and mapped onto the executor's typed
    // variant. It fails as an ordinary Lua runtime error.
    let vm = scheduler_vm(&ModelSet::default(), None);
    let program = compile_block(r#"error({ kind = "tool_loop_exhausted", message = "x" }, 0)"#);
    match vm.start_block_coro(&program) {
        Err(error) if error.code == ErrorCode::LuaRuntime => {}
        other => panic!("an author's table is an ordinary runtime failure, got {other:?}"),
    }
}

#[test]
fn an_uncaught_lua_kind_shim_raise_keeps_the_mapped_runtime_error() {
    // A `lua`-kind table that propagates out of the block is not kept as
    // the error it reads back as: the mapped runtime error already holds
    // the same message with its source and the prompt line the traceback
    // maps to, which is what an authoring error needs. The block is
    // compiled at prompt line 40 and fails at chunk line 2, so the mapped
    // author frame is line 41, where its author prose locates it.
    let vm = scheduler_vm(&test_models(), None);
    let program = LuaProgram::compile(
        "local h = models.get(\"fast\")\nmodels.infer(h, \"a\", \"b\")",
        "prompt",
        "section `Test` prologue",
        NonZeroU32::new(40).expect("40 is non-zero"),
        &null_emitter(),
        "Test",
    )
    .expect("the driver block compiles");
    match vm.start_block_coro(&program) {
        Err(error) if error.code == ErrorCode::LuaRuntime => {
            let message = &error.message;
            assert!(
                message.contains("models.infer takes (prompt) or (model, prompt)"),
                "the mapped error keeps the shim's message: {message}"
            );
            assert!(
                message.contains("section `Test` prologue:41: in"),
                "the author frame maps to the absolute prompt line: {message}"
            );
            assert_eq!(
                error.prose,
                format!(
                    "{TEST_FILE}:41: models.infer takes (prompt) or (model, prompt). {}",
                    ErrorCode::LuaRuntime.prose()
                )
            );
            assert_eq!(
                error.at,
                Some(runvil_types::error::Location::line(
                    TEST_FILE,
                    NonZeroU32::new(41).expect("41 is non-zero")
                )),
                "the failure is located on its mapped line of the setup's file"
            );
        }
        other => panic!("a lua-kind raise must surface as the mapped runtime error, got {other:?}"),
    }
}

#[test]
fn a_typed_error_substituted_at_the_coroutine_boundary_keeps_its_kind() {
    // The shim raises the envelope's table; when that raise surfaces as the
    // coroutine's failure, the driver receives the typed error it answered
    // with, not a string and not a generic Lua runtime error. This holds
    // whether the block let the raise propagate or caught and re-raised
    // the same table.
    for source in [
        "models.infer(\"hi\")",
        "local ok, err = pcall(models.infer, \"hi\")\nerror(err, 0)",
    ] {
        let vm = scheduler_vm(&ModelSet::default(), None);
        let program = compile_block(source);
        let CoroStep::Yielded(thread, _values) =
            vm.start_block_coro(&program).expect("the block suspends")
        else {
            panic!("the shim yield must suspend the block");
        };
        let quota = crate::error::lua_quota(runvil_lua::QuotaResource::LogEvent);
        let mut answered = quota.clone();
        answered.prose = format!(
            "{TEST_FILE}: lua log event quota exceeded. {}",
            ErrorCode::LuaQuota.prose()
        );
        match vm.resume_block_coro_answer(&program, &thread, Err(quota)) {
            Err(error) if error == answered => {}
            other => panic!(
                "block {source:?} must surface the typed quota error with its author prose, \
                 got {other:?}"
            ),
        }
    }
}

#[test]
fn a_structured_raise_surfacing_as_the_coroutine_failure_keeps_its_table() {
    // Without a retained typed error to substitute (the envelope was
    // rendered ahead of time, as a Lua-side raise would be), the failure
    // still arrives typed: the table the shim raised reads back as the
    // universal error it was rendered from, never flattened to the
    // message string.
    let vm = scheduler_vm(&ModelSet::default(), None);
    let program = compile_block("models.infer(\"hi\")");
    let CoroStep::Yielded(thread, _values) =
        vm.start_block_coro(&program).expect("the block suspends")
    else {
        panic!("the shim yield must suspend the block");
    };
    let (envelope, _retained) =
        answer_envelope(vm.lua(), Err(crate::error::context_exhausted("precheck")))
            .expect("the envelope renders");
    match vm.resume_block_coro(&program, &thread, envelope) {
        Err(error) => assert_eq!(
            error,
            crate::error::context_exhausted("precheck"),
            "the kept table reads back as its error, the message included"
        ),
        other => panic!("expected the kept table's failure, got {other:?}"),
    }
}

#[test]
fn an_error_envelope_raises_at_the_call_site_without_a_position_prefix() {
    let vm = scheduler_vm(&ModelSet::default(), None);
    let (thread, _yielded) = start(&vm, r#"return models.infer("hi")"#);
    let error = thread
        .resume::<MultiValue>((false, "model is down"))
        .expect_err("the shim raises the envelope's message");
    // The raised error's message line is exactly the envelope string:
    // `error(result, 0)` suppresses the position prefix. (mlua appends
    // the traceback to the payload; that is its own rendering, not a
    // prefix on the message.)
    let mlua::Error::RuntimeError(message) = &error else {
        panic!("expected a runtime error, got {error:?}");
    };
    let first_line = message.lines().next().expect("a message line");
    assert_eq!(first_line, "model is down");
}

#[test]
fn a_traceback_through_a_shim_shows_unmapped_impl_frames() {
    let vm = scheduler_vm(&ModelSet::default(), None);
    // The var_snapshot capture fails on a reassigned `var` global: an
    // unexpected shim error, whose frames must render verbatim.
    let program = LuaProgram::compile(
        "var = 5\ncall(\"## Child\")",
        "prompt",
        "section `Test` prologue",
        NonZeroU32::new(40).expect("40 is non-zero"),
        &null_emitter(),
        "Test",
    )
    .expect("the driver program compiles");
    let function = program.load(vm.lua()).expect("the driver program loads");
    let thread = vm
        .lua()
        .create_thread(function)
        .expect("the driver thread creates");
    let error = thread
        .resume::<MultiValue>(())
        .expect_err("the reassigned var fails the snapshot");
    let raw = error.to_string();
    assert!(
        raw.contains("crates/runvil-internal/lua/src/__impl_coro.lua:"),
        "the shim frame renders as a verbatim file:line: {raw}"
    );
    assert!(
        !raw.contains("[string \"@crates") && !raw.contains("[string \"crates"),
        "the shim frame renders bare, outside any [string \"...\"] wrapper: {raw}"
    );
    assert!(
        raw.contains("[string \"line 40\"]:2:"),
        "the author frame is present at chunk line 2: {raw}"
    );
    let mapped = program.map_runtime_error(&error).to_string();
    assert!(
        mapped.contains("crates/runvil-internal/lua/src/__impl_coro.lua:"),
        "the line mapper leaves the shim frame unmapped: {mapped}"
    );
    assert!(
        mapped.contains("section `Test` prologue:41: in"),
        "the author frame maps to the absolute prompt line: {mapped}"
    );
}
