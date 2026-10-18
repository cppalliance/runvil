//! `json.decode`: the Lua values one document decodes to, the size and
//! nesting limits, and the errors it raises.

use super::*;

/// The error a Rust callback returned, under mlua's wrapping layers.
fn callback_cause(error: &mlua::Error) -> &mlua::Error {
    match error {
        mlua::Error::CallbackError { cause, .. } | mlua::Error::WithContext { cause, .. } => {
            callback_cause(cause)
        }
        other => other,
    }
}

#[test]
fn numbers_decode_as_integers_in_the_i64_range_and_as_floats_otherwise() {
    let vm = json_vm();
    let described: String = vm.eval(
        "local out = {}\n\
         for _, text in ipairs({ '2', '-9223372036854775808', '1.5', '1.0', \
         '9223372036854775808' }) do\n\
           local value = json.decode(text)\n\
           out[#out + 1] = math.type(value) .. ' ' .. tostring(value)\n\
         end\n\
         return table.concat(out, ', ')",
    );
    assert_eq!(
        described,
        "integer 2, integer -9223372036854775808, float 1.5, float 1.0, \
         float 9.2233720368547758e+18"
    );
}

#[test]
fn strings_booleans_nested_data_and_non_ascii_text_decode_to_lua_values() {
    let vm = json_vm();
    let (name, escaped, tags, second, ok, deep, flag): (
        String,
        String,
        i64,
        String,
        bool,
        String,
        bool,
    ) = vm.eval(
        r#"local v = json.decode('{"name":"Zoë ✓ 日本","escaped":"caf\\u00e9","tags":["a","b"],"meta":{"ok":true,"deep":{"x":"y"}},"flag":false}')
return v.name, v.escaped, #v.tags, v.tags[2], v.meta.ok, v.meta.deep.x, v.flag"#,
    );
    assert_eq!(
        (name.as_str(), escaped.as_str(), tags, second.as_str()),
        ("Zoë ✓ 日本", "café", 2, "b")
    );
    assert_eq!((ok, deep.as_str(), flag), (true, "y", false));
}

#[test]
fn every_json_null_decodes_to_json_null() {
    let vm = json_vm();
    let (count, second, field, whole): (i64, bool, bool, bool) = vm.eval(
        r#"local list = json.decode('[1,null,3]')
local object = json.decode('{"a":null}')
return #list, list[2] == json.null, object.a == json.null, json.decode('null') == json.null"#,
    );
    assert_eq!((count, second, field, whole), (3, true, true, true));
}

#[test]
fn a_document_at_the_byte_cap_decodes_and_one_byte_more_raises() {
    let vm = json_vm();
    let length: i64 = vm.eval(r#"return #json.decode('"' .. string.rep('a', 8388606) .. '"')"#);
    assert_eq!(length, 8_388_606, "an 8388608-byte document decodes");
    assert_eq!(
        caught(
            &vm,
            r#"json.decode('"' .. string.rep('a', 8388607) .. '"')"#
        ),
        lua_error("json.decode: input exceeds the 8388608-byte limit")
    );
}

#[test]
fn nesting_127_deep_decodes_and_128_deep_raises_the_recursion_message() {
    let vm = json_vm();
    let depth: i64 = vm.eval(
        "local value = json.decode(string.rep('[', 127) .. string.rep(']', 127))\n\
         local depth = 0\n\
         while type(value) == 'table' do depth = depth + 1; value = value[1] end\n\
         return depth",
    );
    assert_eq!(depth, 127);
    assert_eq!(
        caught(
            &vm,
            "json.decode(string.rep('[', 128) .. string.rep(']', 128))"
        ),
        lua_error("json.decode: recursion limit exceeded at line 1 column 128")
    );
}

#[test]
fn parser_and_argument_failures_raise_kind_lua_naming_the_function() {
    let vm = json_vm();
    for (call, message) in [
        (
            "json.decode('x')",
            "json.decode: expected value at line 1 column 1",
        ),
        (
            "json.decode('1 2')",
            "json.decode: trailing characters at line 1 column 3",
        ),
        (
            "json.decode(5)",
            "json.decode expects a string, got integer",
        ),
        (
            "json.decode(1.5)",
            "json.decode expects a string, got number",
        ),
        ("json.decode()", "json.decode expects a string, got nil"),
    ] {
        assert_eq!(caught(&vm, call), lua_error(message), "{call}");
    }
}

#[test]
fn the_memory_limit_failing_mid_build_raises_the_memory_error_unprefixed() {
    let vm = TestVm::new()
        .limits(4 * 1024 * 1024, DEFAULT_LUA_LOG_EVENTS)
        .build();
    vm.exec("doc = '[' .. string.rep('[]', 200000, ',') .. ']'");
    let error = vm.eval_err("json.decode(doc)");
    let cause = callback_cause(&error);
    assert!(
        matches!(cause, mlua::Error::MemoryError(_)),
        "required the memory error, actual {error:?}"
    );
    assert!(
        !cause.to_string().contains("json.decode"),
        "required the memory error unprefixed, actual {cause}"
    );
}
