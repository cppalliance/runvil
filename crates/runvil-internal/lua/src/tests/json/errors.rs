//! `json.encode` refusals: each strict message and its path, the order the
//! key rules report in, cycles, and the options argument.

use super::*;

#[test]
fn values_that_are_not_json_data_raise_naming_their_path_and_type() {
    let vm = json_vm();
    bind_foreign_values(&vm);
    vm.exec("messages:user('u')");
    for (call, message) in [
        (
            "json.encode({ a = { function() end } })",
            "json.encode: value.a[1] must be JSON data, got function",
        ),
        (
            "json.encode({ list = { 1, co } })",
            "json.encode: value.list[2] must be JSON data, got thread",
        ),
        (
            "json.encode(light)",
            "json.encode: value must be JSON data, got lightuserdata",
        ),
        (
            "json.encode({ m = messages[1] })",
            "json.encode: value.m must be JSON data, got userdata",
        ),
        (
            r"json.encode('\xff')",
            "json.encode: value is not valid UTF-8",
        ),
        (
            r"json.encode({ s = 'ok\xffno' })",
            "json.encode: value.s is not valid UTF-8",
        ),
        (
            r"json.encode({ ['\xff'] = 1 })",
            "json.encode: value.\u{FFFD} is not valid UTF-8",
        ),
        (
            "json.encode({ 1, 0/0 })",
            "json.encode: value[2] is not a finite number",
        ),
        (
            "json.encode({ x = math.huge })",
            "json.encode: value.x is not a finite number",
        ),
        (
            "json.encode(-math.huge)",
            "json.encode: value is not a finite number",
        ),
    ] {
        assert_eq!(caught(&vm, call), lua_error(message), "{call}");
    }
}

#[test]
fn tables_that_break_the_key_rules_raise_naming_the_rule() {
    let vm = json_vm();
    for (call, message) in [
        (
            "json.encode((function() local l = json.array({ 1 }); l.x = 2; return l end)())",
            "json.encode: value is marked as a list but has a string key",
        ),
        (
            "json.encode({ list = json.array({ [1.5] = 1 }) })",
            "json.encode: value.list is marked as a list but has a number key",
        ),
        (
            "json.encode(json.array({ [2] = 'b' }))",
            "json.encode: value is missing list item [1]",
        ),
        (
            "json.encode({ [true] = 1 })",
            "json.encode: value has a boolean key; object keys must be strings",
        ),
        (
            "json.encode({ { [1.5] = 1 } })",
            "json.encode: value[1] has a number key; object keys must be strings",
        ),
        (
            "json.encode({ 1, a = 2 })",
            "json.encode: value mixes list items and named keys",
        ),
    ] {
        assert_eq!(caught(&vm, call), lua_error(message), "{call}");
    }
}

#[test]
fn the_key_rules_report_in_order_and_before_any_value() {
    let vm = json_vm();
    for (call, message) in [
        (
            "json.encode({ 1, a = 2, [true] = 3 })",
            "json.encode: value has a boolean key; object keys must be strings",
        ),
        (
            "json.encode({ [1] = 'a', [3] = 'c', x = 1 })",
            "json.encode: value mixes list items and named keys",
        ),
        (
            "json.encode({ [1] = 'a', [3] = 'c' })",
            "json.encode: value is missing list item [2]",
        ),
        (
            "json.encode({ [0] = 'a' })",
            "json.encode: value is missing list item [1]",
        ),
        (
            "json.encode({ [-1] = 'a', [1] = 'b' })",
            "json.encode: value is missing list item [2]",
        ),
        (
            "json.encode({ [1] = function() end, a = 1 })",
            "json.encode: value mixes list items and named keys",
        ),
    ] {
        assert_eq!(caught(&vm, call), lua_error(message), "{call}");
    }
}

#[test]
fn a_table_with_a_table_key_and_a_function_key_reports_the_function_key() {
    let vm = json_vm();
    assert_eq!(
        caught(&vm, "json.encode({ [{}] = 1, [function() end] = 2 })"),
        lua_error("json.encode: value has a function key; object keys must be strings")
    );
}

/// The Lua source of `build(pairs_list, order, marked)`, which builds a
/// table from the `{ key, value }` pairs of `pairs_list`, inserted in the
/// index order `order` among filler keys that are then removed, so the
/// table's hash layout differs between orders, and marks it as a list when
/// `marked` is true.
const BUILD_IN_ORDER: &str = "\
function build(pairs_list, order, marked)
  local t = {}
  for i = 1, 40 do t['filler' .. i] = i end
  for _, index in ipairs(order) do
    local pair = pairs_list[index]
    t[pair[1]] = pair[2]
  end
  for i = 1, 40 do t['filler' .. i] = nil end
  if marked then json.array(t) end
  return t
end";

#[test]
fn non_utf8_and_infinite_keys_report_the_same_error_however_the_table_was_built() {
    let vm = json_vm();
    vm.exec(BUILD_IN_ORDER);
    for (pairs_list, marked, message) in [
        (
            r"{ { '\xff', 1 }, { 'a', function() end }, { 'b', 3 } }",
            false,
            "json.encode: value.\u{FFFD} is not valid UTF-8",
        ),
        (
            r"{ { '\xff', 1 }, { math.huge, 2 }, { 1, 3 } }",
            false,
            "json.encode: value has a number key; object keys must be strings",
        ),
        (
            r"{ { '\xff', 1 }, { -math.huge, 2 }, { 1, 3 } }",
            true,
            "json.encode: value is marked as a list but has a number key",
        ),
        (
            r"{ { '\xff', 1 }, { math.huge, 2 }, { {}, 3 } }",
            false,
            "json.encode: value has a number key; object keys must be strings",
        ),
    ] {
        for order in ["{ 1, 2, 3 }", "{ 3, 2, 1 }", "{ 2, 3, 1 }"] {
            let call = format!("json.encode(build({pairs_list}, {order}, {marked}))");
            assert_eq!(caught(&vm, &call), lua_error(message), "{call}");
        }
    }
}

#[test]
fn one_invalid_table_built_in_two_key_orders_in_two_fresh_vms_raises_one_message() {
    let pairs_list = "{ { {}, 1 }, { function() end, 2 }, { 'x', 3 }, { 2.5, 4 } }";
    let mut messages = Vec::new();
    for order in ["{ 1, 2, 3, 4 }", "{ 4, 3, 2, 1 }"] {
        for marked in [false, true] {
            let vm = json_vm();
            vm.exec(BUILD_IN_ORDER);
            messages.push(caught(
                &vm,
                &format!("json.encode(build({pairs_list}, {order}, {marked}))"),
            ));
        }
    }
    assert_eq!(
        messages,
        [
            lua_error("json.encode: value has a number key; object keys must be strings"),
            lua_error("json.encode: value is marked as a list but has a number key"),
            lua_error("json.encode: value has a number key; object keys must be strings"),
            lua_error("json.encode: value is marked as a list but has a number key"),
        ]
    );
}

#[test]
fn a_table_that_contains_itself_raises_but_a_shared_table_encodes() {
    let vm = json_vm();
    assert_eq!(
        caught(
            &vm,
            "(function() local t = {}; t.self = t; return json.encode(t) end)()"
        ),
        lua_error("json.encode: value.self contains itself")
    );
    assert_eq!(
        encoded(
            &vm,
            "(function() local s = { 1 }; return json.encode({ a = s, b = { s } }) end)()"
        ),
        r#"{"a":[1],"b":[[1]]}"#
    );
}

#[test]
fn a_loop_of_127_tables_repeating_at_level_128_reports_the_cycle() {
    let vm = json_vm();
    vm.exec(
        "loop = {}\n\
         local last = loop\n\
         for _ = 2, 127 do local next_table = {}; last[1] = next_table; last = next_table end\n\
         last[1] = loop",
    );
    assert_eq!(
        caught(&vm, "json.encode(loop)"),
        lua_error(&format!(
            "json.encode: {} contains itself",
            nested_path(127)
        ))
    );
}

#[test]
fn the_options_argument_is_checked_before_the_value() {
    let vm = json_vm();
    for (call, message) in [
        (
            "json.encode(1, 5)",
            "json.encode: options must be a table, got integer",
        ),
        (
            "json.encode(function() end, 5)",
            "json.encode: options must be a table, got integer",
        ),
        (
            "json.encode(1, 'debug')",
            "json.encode: options must be a table, got string",
        ),
        (
            "json.encode(1, { pretty = true })",
            "json.encode: unknown option pretty",
        ),
        (
            "json.encode(1, { [1] = true })",
            "json.encode: unknown option 1",
        ),
        (
            "json.encode(1, { [true] = true })",
            "json.encode: unknown option boolean",
        ),
        (
            "json.encode(1, { debug = 'yes' })",
            "json.encode: option debug must be a boolean, got string",
        ),
    ] {
        assert_eq!(caught(&vm, call), lua_error(message), "{call}");
    }
    assert_eq!(encoded(&vm, "json.encode(1, { debug = false })"), "1");
    assert_eq!(encoded(&vm, "json.encode(1, nil)"), "1");
}

#[test]
fn an_encode_error_under_pcall_has_kind_lua() {
    let vm = json_vm();
    let (kind, message): (String, String) = vm.eval(
        "local ok, err = pcall(json.encode, function() end)\n\
         return err.kind, tostring(err)",
    );
    assert_eq!(
        (kind.as_str(), message.as_str()),
        ("lua", "json.encode: value must be JSON data, got function")
    );
}
