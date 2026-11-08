//! The task-operation request parsers: `spawn`'s target, seeds, and var
//! snapshot; the `timer` leaf request's author-supplied `seconds`, its
//! domain checks, and the `Duration` it parses into; `pending`'s refusal
//! of any argument; `receive` and `arrivals`, which have no fields; `send`'s
//! task and text; `reply`'s value, converted as a section's return value
//! is; and `concurrency`'s optional shim-validated limit.

use super::*;

#[test]
fn spawn_parses_target_seeds_and_var() {
    let lua = Lua::new();
    let table = lua_table(
        &lua,
        "{ op = 'spawn', target = '## Child', input = 'override', item = { name = 'alpha' }, \
           index = 3, var = { k = 1 } }",
    );
    let request = expect_request(Request::from_yield(&lua, &Value::Table(table)));
    match request {
        Request::Spawn {
            target,
            input,
            item,
            index,
            var,
        } => {
            assert_eq!(target, "## Child");
            assert_eq!(input.as_deref(), Some("override"));
            assert_eq!(item, Some(json!({ "name": "alpha" })));
            assert_eq!(index, Some(3));
            assert_eq!(var, json!({ "k": 1 }));
        }
        other => panic!("expected a spawn request, got {other:?}"),
    }
}

#[test]
fn spawn_without_options_parses_every_seed_as_absent() {
    let lua = Lua::new();
    let table = lua_table(
        &lua,
        "{ op = 'spawn', target = '## Child', var = { k = 1 } }",
    );
    match expect_request(Request::from_yield(&lua, &Value::Table(table))) {
        Request::Spawn {
            input, item, index, ..
        } => {
            assert_eq!(input, None);
            assert_eq!(item, None);
            assert_eq!(index, None);
        }
        other => panic!("expected a spawn request, got {other:?}"),
    }
}

#[test]
fn a_spawn_item_holding_json_null_parses_to_a_null_field() {
    let vm = crate::test_vm::TestVm::new().build();
    let table: mlua::Table = vm.eval(
        "return { op = 'spawn', target = '## Child', \
         item = { a = json.null }, var = { k = 1 } }",
    );
    match expect_request(Request::from_yield(vm.lua(), &Value::Table(table))) {
        Request::Spawn { item, .. } => assert_eq!(item, Some(json!({ "a": null }))),
        other => panic!("expected a spawn request, got {other:?}"),
    }
}

#[test]
fn spawn_seed_shape_errors_are_the_calls_error() {
    // `item` and `index` are author options: a wrong shape returns as
    // the call's answer so `tasks.spawn` raises it at the call site.
    let lua = Lua::new();
    let table = lua_table(
        &lua,
        "{ op = 'spawn', target = '## Child', item = function() end, \
           var = { k = 1 } }",
    );
    match Request::from_yield(&lua, &Value::Table(table)) {
        YieldParse::Refused(Error {
            code: ErrorCode::LuaRuntime,
            message,
            ..
        }) => {
            assert_eq!(message, "item must be JSON data, got function");
        }
        other => panic!("expected the item call error, got {other:?}"),
    }

    let table = lua_table(
        &lua,
        "{ op = 'spawn', target = '## Child', index = -1, var = { k = 1 } }",
    );
    match Request::from_yield(&lua, &Value::Table(table)) {
        YieldParse::Refused(Error {
            code: ErrorCode::LuaRuntime,
            message,
            ..
        }) => {
            assert_eq!(message, "index must be a non-negative integer, got -1");
        }
        other => panic!("expected the index call error, got {other:?}"),
    }
}

#[test]
fn an_unconvertible_spawn_item_is_the_calls_error_naming_the_conversion_failure() {
    let lua = Lua::new();
    let table = lua_table(
        &lua,
        "{ op = 'spawn', target = '## Child', item = { f = function() end }, \
           var = { k = 1 } }",
    );
    let item: Value = table
        .raw_get("item")
        .expect("the request table holds its item");
    let cause = lua
        .from_value::<serde_json::Value>(item)
        .expect_err("a table holding a function has no JSON form");
    match Request::from_yield(&lua, &Value::Table(table)) {
        YieldParse::Refused(Error {
            code: ErrorCode::LuaRuntime,
            message,
            ..
        }) => {
            assert_eq!(
                message,
                format!("item must be a JSON-representable value: {cause}")
            );
        }
        other => panic!("expected the item call error, got {other:?}"),
    }
}

#[test]
fn a_malformed_task_id_is_the_calls_error_naming_the_parse_failure() {
    let lua = Lua::new();
    let cause = "0.x".parse::<TaskId>().expect_err("`0.x` is not a task id");
    let table = lua_table(&lua, "{ op = 'ready', task = '0.x' }");
    match Request::from_yield(&lua, &Value::Table(table)) {
        YieldParse::Refused(Error {
            code: ErrorCode::LuaRuntime,
            message,
            ..
        }) => {
            assert_eq!(
                message,
                format!("`0.x` is not a task id such as `0.1`: {cause}")
            );
        }
        other => panic!("expected the task id call error, got {other:?}"),
    }
}

#[test]
fn pending_parses_without_fields() {
    let lua = Lua::new();
    let table = lua_table(&lua, "{ op = 'pending' }");
    let request = expect_request(Request::from_yield(&lua, &Value::Table(table)));
    assert!(matches!(request, Request::Pending), "got {request:?}");
}

#[test]
fn receive_parses_without_fields() {
    let lua = Lua::new();
    let table = lua_table(&lua, "{ op = 'receive' }");
    let request = expect_request(Request::from_yield(&lua, &Value::Table(table)));
    assert!(matches!(request, Request::Receive), "got {request:?}");
}

#[test]
fn arrivals_parses_without_fields() {
    let lua = Lua::new();
    let table = lua_table(&lua, "{ op = 'arrivals' }");
    let request = expect_request(Request::from_yield(&lua, &Value::Table(table)));
    assert!(matches!(request, Request::Arrivals), "got {request:?}");
}

#[test]
fn send_parses_its_task_and_its_text() {
    let lua = Lua::new();
    let table = lua_table(
        &lua,
        "{ op = 'send', task = '0.2', text = 'also cover the tests' }",
    );
    let request = expect_request(Request::from_yield(&lua, &Value::Table(table)));
    let Request::Send { task, text } = request else {
        panic!("a send request, got {request:?}");
    };
    assert_eq!(task, "0.2".parse::<TaskId>().expect("`0.2` is a task id"));
    assert_eq!(text, "also cover the tests");
}

#[test]
fn a_send_naming_no_task_id_is_the_calls_error() {
    let lua = Lua::new();
    let table = lua_table(&lua, "{ op = 'send', task = '0.x', text = 'hi' }");
    match Request::from_yield(&lua, &Value::Table(table)) {
        YieldParse::Refused(Error {
            code: ErrorCode::LuaRuntime,
            message,
            ..
        }) => {
            assert!(message.starts_with("`0.x` is not a task id"), "{message}");
        }
        other => panic!("expected the task id call error, got {other:?}"),
    }
}

#[test]
fn reply_parses_its_value_as_text_as_a_section_return_converts_it() {
    let lua = Lua::new();
    for (value, text) in [
        ("'summary'", "summary"),
        ("42", "42"),
        ("1.5", "1.5"),
        ("true", "true"),
        ("nil", ""),
    ] {
        let table = lua_table(&lua, &format!("{{ op = 'reply', value = {value} }}"));
        let request = expect_request(Request::from_yield(&lua, &Value::Table(table)));
        let Request::Reply { text: parsed } = request else {
            panic!("a reply request, got {request:?}");
        };
        assert_eq!(parsed, text, "the value {value}");
    }
}

#[test]
fn a_reply_whose_value_has_no_text_form_is_the_calls_error() {
    let lua = Lua::new();
    let table = lua_table(&lua, "{ op = 'reply', value = { done = true } }");
    match Request::from_yield(&lua, &Value::Table(table)) {
        YieldParse::Refused(Error {
            code: ErrorCode::LuaRuntime,
            message,
            ..
        }) => {
            assert!(
                message.contains("cannot return a table as a result"),
                "{message}"
            );
        }
        other => panic!("expected the scalar-return call error, got {other:?}"),
    }
}

#[test]
fn a_spawn_with_a_non_string_target_keeps_the_resolve_error() {
    let lua = Lua::new();
    let table = lua_table(&lua, "{ op = 'spawn', target = 42, var = { k = 1 } }");
    match Request::from_yield(&lua, &Value::Table(table)) {
        YieldParse::Refused(Error {
            code: ErrorCode::LuaRuntime,
            message,
            ..
        }) => {
            assert!(
                message.contains("section target must be a string, got integer"),
                "unexpected message: {message}"
            );
        }
        other => panic!("expected the resolve_section_target call error, got {other:?}"),
    }
}

#[test]
fn timer_parses_a_non_negative_finite_seconds_value_into_its_duration() {
    let lua = Lua::new();
    for (seconds, expected) in [
        ("1.5", std::time::Duration::from_millis(1_500)),
        ("30", std::time::Duration::from_secs(30)),
        ("0.0", std::time::Duration::ZERO),
    ] {
        let table = lua_table(&lua, &format!("{{ op = 'timer', seconds = {seconds} }}"));
        match expect_request(Request::from_yield(&lua, &Value::Table(table))) {
            Request::Timer { duration } => assert_eq!(duration, expected, "{seconds}"),
            other => panic!("expected a timer request for {seconds}, got {other:?}"),
        }
    }
}

#[test]
fn timer_seconds_out_of_domain_are_the_calls_error() {
    // `seconds` is the author's `opts.timeout`: a negative, non-finite,
    // out-of-range, or non-numeric value returns as the call's answer
    // so the wait shim raises it at the call site and starts no timer.
    let lua = Lua::new();
    let cases = [
        ("-1.0", "-1"),
        ("0/0", "NaN"),
        ("math.huge", "inf"),
        // Past `Duration`'s u64 seconds; Display renders the plain digits.
        ("1e20", "100000000000000000000"),
        ("nil", "nil"),
    ];
    for (seconds, needle) in cases {
        let table = lua_table(&lua, &format!("{{ op = 'timer', seconds = {seconds} }}"));
        match Request::from_yield(&lua, &Value::Table(table)) {
            YieldParse::Refused(Error {
                code: ErrorCode::LuaRuntime,
                message,
                ..
            }) => {
                assert!(
                    message.contains("timeout") && message.contains(needle),
                    "the message names the option and the value: {message}"
                );
            }
            other => panic!("expected the timeout call error for {needle}, got {other:?}"),
        }
    }
    let table = lua_table(&lua, "{ op = 'timer', seconds = 'soon' }");
    match Request::from_yield(&lua, &Value::Table(table)) {
        YieldParse::Refused(Error {
            code: ErrorCode::LuaRuntime,
            message,
            ..
        }) => {
            assert_eq!(message, "timeout must be a number, got string");
        }
        other => panic!("expected the timeout type error, got {other:?}"),
    }
}

#[test]
fn concurrency_parses_an_optional_positive_whole_number_limit() {
    // `limit` is shim-produced - the shim has already refused every
    // non-positive-whole-number argument at the call site - so an absent
    // field is the read-only form and a positive integer is a limit;
    // any other shape is a malformed yield.
    let lua = Lua::new();
    let table = lua_table(&lua, "{ op = 'concurrency' }");
    match expect_request(Request::from_yield(&lua, &Value::Table(table))) {
        Request::Concurrency { limit } => assert_eq!(limit, None, "no argument reads the limit"),
        other => panic!("expected a concurrency request, got {other:?}"),
    }
    for (limit, expected) in [("1", 1), ("16", 16)] {
        let table = lua_table(&lua, &format!("{{ op = 'concurrency', limit = {limit} }}"));
        match expect_request(Request::from_yield(&lua, &Value::Table(table))) {
            Request::Concurrency { limit } => {
                assert_eq!(limit.map(std::num::NonZeroU64::get), Some(expected));
            }
            other => panic!("expected a concurrency request, got {other:?}"),
        }
    }
    for limit in ["0", "-2", "2.5", "'two'"] {
        let table = lua_table(&lua, &format!("{{ op = 'concurrency', limit = {limit} }}"));
        match Request::from_yield(&lua, &Value::Table(table)) {
            YieldParse::Malformed(_) => {}
            other => panic!("expected a malformed yield for {limit}, got {other:?}"),
        }
    }
}

#[test]
fn concurrency_converts_a_whole_number_float_and_refuses_an_out_of_range_one() {
    // A float with an integral value is the author writing `8.0`: the
    // shim's whole-number check let it through, so the parser converts
    // it. A whole-number float past `u64` (an author's `1e20`) is the
    // call's error, raised at the call site like the shim's own refusal.
    let lua = Lua::new();
    let table = lua_table(&lua, "{ op = 'concurrency', limit = 8.0 }");
    match expect_request(Request::from_yield(&lua, &Value::Table(table))) {
        Request::Concurrency { limit } => {
            assert_eq!(limit.map(std::num::NonZeroU64::get), Some(8));
        }
        other => panic!("expected a concurrency request, got {other:?}"),
    }
    let table = lua_table(&lua, "{ op = 'concurrency', limit = 1e20 }");
    match Request::from_yield(&lua, &Value::Table(table)) {
        YieldParse::Refused(Error {
            code: ErrorCode::LuaRuntime,
            message,
            ..
        }) => assert!(
            message.contains("positive whole number") && message.contains("64-bit"),
            "the message names the requirement and the range: {message}"
        ),
        other => panic!("expected the concurrency call error, got {other:?}"),
    }
}

#[test]
fn a_timer_answer_resumes_the_task_id_as_its_path_text() {
    let lua = Lua::new();
    let task: TaskId = "0.3".parse().expect("`0.3` is a valid task id");
    let answer = Answer::Timer(task);
    let (envelope, retained) =
        answer_envelope(&lua, Ok(answer)).expect("a timer answer renders its envelope");
    assert!(retained.is_none());
    let (ok, value) = echo_through_lua(&lua, envelope);
    assert!(ok);
    assert_eq!(
        value,
        Value::String(
            lua.create_string("0.3")
                .expect("a fresh VM creates a short string")
        )
    );
}
