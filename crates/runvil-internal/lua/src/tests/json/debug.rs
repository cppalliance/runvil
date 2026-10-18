//! `json.encode` in debug mode: the placeholder each failure becomes, the
//! object a table that breaks the key rules becomes, and the option errors
//! that still raise.

use super::*;

/// Debug-mode output of the Lua expression `value` on `vm`.
#[track_caller]
fn debug_text(vm: &BuiltVm, value: &str) -> String {
    encoded(vm, &format!("json.encode({value}, {{ debug = true }})"))
}

#[test]
fn values_that_are_not_json_data_become_type_placeholders() {
    let vm = json_vm();
    bind_foreign_values(&vm);
    let described: String = vm.eval("return tostring(described)");
    assert_eq!(described, "probe", "the fixture defines __tostring");
    for (value, text) in [
        ("function() end", r#""[function]""#),
        ("co", r#""[thread]""#),
        ("light", r#""[userdata]""#),
        ("described", r#""[userdata]""#),
        ("messages.new():user('u')[1]", r#""[userdata]""#),
        (
            "{ f = function() end, list = { co, described } }",
            r#"{"f":"[function]","list":["[thread]","[userdata]"]}"#,
        ),
    ] {
        let output = debug_text(&vm, value);
        assert_eq!(output, text, "{value}");
        assert!(!output.contains("0x"), "no address in {output}");
    }
}

#[test]
fn a_function_field_becomes_the_function_placeholder() {
    let vm = json_vm();
    assert_eq!(
        encoded(&vm, "json.encode({ f = function() end }, { debug = true })"),
        r#"{"f":"[function]"}"#
    );
}

#[test]
fn cycles_and_tables_past_the_depth_limit_become_placeholders() {
    let vm = json_vm();
    vm.exec("loop = {}\nloop.self = loop\nloop.n = 1");
    assert_eq!(debug_text(&vm, "loop"), r#"{"n":1,"self":"[cycle]"}"#);
    vm.exec("deep = 1\nfor _ = 1, 128 do deep = { deep } end");
    assert_eq!(
        debug_text(&vm, "deep"),
        format!("{}\"[too deep]\"{}", "[".repeat(127), "]".repeat(127))
    );
}

#[test]
fn non_finite_numbers_and_invalid_utf8_become_text() {
    let vm = json_vm();
    for (value, text) in [
        (
            "{ 0/0, math.huge, -math.huge }",
            r#"["NaN","Infinity","-Infinity"]"#,
        ),
        (r"'ok\xffno'", "\"ok\u{FFFD}no\""),
        (r"{ ['k\xff'] = 'v' }", "{\"k\u{FFFD}\":\"v\"}"),
    ] {
        assert_eq!(debug_text(&vm, value), text, "{value}");
    }
}

#[test]
fn a_table_that_breaks_the_key_rules_becomes_an_object_with_text_keys() {
    let vm = json_vm();
    assert_eq!(
        debug_text(
            &vm,
            "{ [2] = 'i', [2.5] = 'f', [1e300] = 'e', [math.huge] = 'p', [-math.huge] = 'n', \
             [true] = 't', [false] = 'u', x = 'x', [{}] = 'table', [function() end] = 'fn' }"
        ),
        r#"{"-Infinity":"n","1e+300":"e","2":"i","2.5":"f","Infinity":"p","false":"u","true":"t","x":"x"}"#
    );
    assert_eq!(
        encoded(&vm, "json.encode({ 2, 2.5, 1e300 })"),
        "[2,2.5,1e+300]",
        "each number key is the text its value encodes as"
    );
    assert_eq!(
        debug_text(&vm, "json.array({ 'a', x = 1 })"),
        r#"{"1":"a","x":1}"#,
        "a marked list that breaks the key rules becomes an object too"
    );
    assert_eq!(
        debug_text(&vm, "{ [1] = 'a', [3] = 'c' }"),
        r#"{"1":"a","3":"c"}"#
    );
}

#[test]
fn a_number_key_colliding_with_its_text_loses_to_the_later_string_key() {
    let vm = json_vm();
    vm.exec(
        "function build(first_string)\n\
           local t = {}\n\
           if first_string then t['1'] = 'string'; t[1] = 'integer'\n\
           else t[1] = 'integer'; t['1'] = 'string' end\n\
           return t\n\
         end",
    );
    for first_string in [true, false] {
        assert_eq!(
            debug_text(&vm, &format!("build({first_string})")),
            r#"{"1":"string"}"#,
            "built with the string key first: {first_string}"
        );
    }
}

#[test]
fn non_utf8_and_infinite_keys_order_the_same_way_every_time() {
    for order in [[1, 2, 3, 4], [4, 3, 2, 1], [2, 4, 1, 3]] {
        let vm = json_vm();
        let source = format!(
            "local pairs_list = {{ {{ math.huge, 'number' }}, {{ 'Infinity', 'string' }}, \
             {{ '\\xfe', 'low' }}, {{ '\\xff', 'high' }} }}\n\
             local t = {{ [true] = 'mixed' }}\n\
             for _, index in ipairs({{ {} }}) do\n\
               local pair = pairs_list[index]\n\
               t[pair[1]] = pair[2]\n\
             end\n\
             return json.encode(t, {{ debug = true }})",
            order.map(|index| index.to_string()).join(", ")
        );
        let text: String = vm.eval(&source);
        assert_eq!(
            text, "{\"Infinity\":\"string\",\"true\":\"mixed\",\"\u{FFFD}\":\"high\"}",
            "inserted in the order {order:?}"
        );
    }
}

#[test]
fn unreadable_sealed_views_become_cannot_read_placeholders() {
    let vm = json_vm();
    for (value, text) in [
        ("sys", r#""[cannot read: sys is sealed]""#),
        (
            "{ s = setmetatable({}, { __index = function() end, __metatable = {} }) }",
            r#"{"s":"[cannot read]"}"#,
        ),
        (
            "setmetatable({}, { __index = function() end, __metatable = false })",
            r#""[cannot read]""#,
        ),
    ] {
        assert_eq!(debug_text(&vm, value), text, "{value}");
    }
}

#[test]
fn option_errors_still_raise_in_debug_mode() {
    let vm = json_vm();
    for (call, message) in [
        (
            "json.encode(function() end, { debug = 1 })",
            "json.encode: option debug must be a boolean, got integer",
        ),
        (
            "json.encode(function() end, { debug = true, extra = 1 })",
            "json.encode: unknown option extra",
        ),
    ] {
        assert_eq!(caught(&vm, call), lua_error(message), "{call}");
    }
}
