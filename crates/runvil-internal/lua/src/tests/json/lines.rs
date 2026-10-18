//! `json.decode_lines`: a stream of values split on any whitespace, the
//! marked list it returns, and the errors it raises. `json.encode_lines`:
//! one encoded item per line, and the errors it raises.

use super::*;

#[test]
fn values_split_on_blank_lines_and_crlf() {
    let vm = json_vm();
    let (count, first, second, third, marked): (i64, i64, i64, i64, bool) = vm.eval(
        r#"local list = json.decode_lines('{"n":1}\n\n{"n":2}\r\n{"n":3}\r\n')
return #list, list[1].n, list[2].n, list[3].n, getmetatable(list) == false"#,
    );
    assert_eq!((count, first, second, third, marked), (3, 1, 2, 3, true));
}

#[test]
fn values_may_share_a_line_and_one_value_may_span_lines() {
    let vm = json_vm();
    let (count, first, second, nested): (i64, i64, String, i64) = vm.eval(
        r#"local list = json.decode_lines('1 "two"\n{"a":\n[3,\n4]}')
return #list, list[1], list[2], list[3].a[2]"#,
    );
    assert_eq!((count, first, second.as_str(), nested), (3, 1, "two", 4));
}

#[test]
fn empty_and_blank_text_return_an_empty_marked_list() {
    let vm = json_vm();
    for text in ["''", r"' \n\r\n\t '"] {
        let (count, marked): (i64, bool) = vm.eval(&format!(
            "local list = json.decode_lines({text})\n\
             return #list, getmetatable(list) == false"
        ));
        assert_eq!((count, marked), (0, true), "decoding {text}");
    }
}

#[test]
fn an_error_names_its_line_and_column_across_the_whole_text() {
    let vm = json_vm();
    let (kind, message) = caught(&vm, r#"json.decode_lines('{"a":1}\n{"b":2}\n{"c":}')"#);
    assert_eq!(kind, "lua");
    assert!(
        message.contains("at line 3"),
        "required the line counted across the whole text, actual {message}"
    );
    assert_eq!(
        message,
        "json.decode_lines: expected value at line 3 column 6"
    );
}

#[test]
fn the_argument_and_cap_errors_carry_the_decode_lines_prefix() {
    let vm = json_vm();
    for (call, message) in [
        (
            "json.decode_lines(5)",
            "json.decode_lines expects a string, got integer",
        ),
        (
            "json.decode_lines()",
            "json.decode_lines expects a string, got nil",
        ),
        (
            "json.decode_lines(string.rep(' ', 8388608) .. '1')",
            "json.decode_lines: input exceeds the 8388608-byte limit",
        ),
    ] {
        assert_eq!(caught(&vm, call), lua_error(message), "{call}");
    }
    let count: i64 = vm.eval("return #json.decode_lines(string.rep(' ', 8388607) .. '1')");
    assert_eq!(count, 1, "an 8388608-byte stream decodes");
}

#[test]
fn encode_lines_writes_each_item_on_its_own_line() {
    let vm = json_vm();
    for (call, text) in [
        (
            "json.encode_lines({ { a = 1 }, { a = 2 } })",
            "{\"a\":1}\n{\"a\":2}\n",
        ),
        (
            r"json.encode_lines({ 'x\ny', json.null, json.array() })",
            "\"x\\ny\"\nnull\n[]\n",
        ),
        ("json.encode_lines({})", ""),
        ("json.encode_lines(json.array())", ""),
        ("json.encode_lines(json.decode('[]'))", ""),
    ] {
        assert_eq!(encoded(&vm, call), text, "{call}");
    }
}

#[test]
fn encode_lines_reads_a_list_through_var() {
    let seed = json!({ "rows": [{ "n": 1 }, { "n": 2 }] });
    let vm = TestVm::new().var(&seed).shims().build();
    assert_eq!(
        encoded(&vm, "json.encode_lines(var.rows)"),
        "{\"n\":1}\n{\"n\":2}\n"
    );
}

#[test]
fn decode_lines_reads_back_what_encode_lines_writes() {
    let vm = json_vm();
    let (count, first_name, first_n, second_tag, second_null): (i64, String, i64, String, bool) =
        vm.eval(
            "local rows = { { name = 'a', n = 1 }, { name = 'b', tags = { 'x' }, gone = json.null } }\n\
             local back = json.decode_lines(json.encode_lines(rows))\n\
             return #back, back[1].name, back[1].n, back[2].tags[1], back[2].gone == json.null",
        );
    assert_eq!(
        (
            count,
            first_name.as_str(),
            first_n,
            second_tag.as_str(),
            second_null
        ),
        (2, "a", 1, "x", true)
    );
}

#[test]
fn encode_lines_refuses_what_is_not_a_list() {
    let vm = json_vm();
    for (call, message) in [
        (
            "json.encode_lines('x')",
            "json.encode_lines expects a list, got string",
        ),
        (
            "json.encode_lines({ a = 1 })",
            "json.encode_lines expects a list, got a table with a string key",
        ),
        (
            "json.encode_lines({ [true] = 1, a = 1 })",
            "json.encode_lines expects a list, got a table with a boolean key",
        ),
        (
            "json.encode_lines({ 1, a = 2 })",
            "json.encode_lines: value mixes list items and named keys",
        ),
        (
            "json.encode_lines({ [1] = 1, [3] = 3 })",
            "json.encode_lines: value is missing list item [2]",
        ),
        (
            "json.encode_lines(json.array({ x = 1 }))",
            "json.encode_lines: value is marked as a list but has a string key",
        ),
        (
            "json.encode_lines(sys)",
            "json.encode_lines: value cannot be read (sys is sealed); \
             copy the fields you need into a plain table",
        ),
        (
            "json.encode_lines({}, { pretty = true })",
            "json.encode_lines: unknown option pretty",
        ),
        (
            "json.encode_lines(5, 5)",
            "json.encode_lines: options must be a table, got integer",
        ),
        (
            "json.encode_lines(5, { debug = true })",
            "json.encode_lines expects a list, got integer",
        ),
        (
            "json.encode_lines({ a = 1 }, { debug = true })",
            "json.encode_lines expects a list, got a table with a string key",
        ),
    ] {
        assert_eq!(caught(&vm, call), lua_error(message), "{call}");
    }
}

#[test]
fn encode_lines_names_a_failing_item_under_the_list_path() {
    let vm = json_vm();
    for (call, message) in [
        (
            "json.encode_lines({ { name = 'a' }, { name = function() end } })",
            "json.encode_lines: value[2].name must be JSON data, got function",
        ),
        (
            "(function() local t = {}; t[1] = t; return json.encode_lines(t) end)()",
            "json.encode_lines: value[1] contains itself",
        ),
    ] {
        assert_eq!(caught(&vm, call), lua_error(message), "{call}");
    }
}

#[test]
fn encode_lines_in_debug_mode_writes_placeholders_inside_items() {
    let vm = json_vm();
    assert_eq!(
        encoded(
            &vm,
            "json.encode_lines({ function() end, { f = function() end }, 0/0 }, { debug = true })"
        ),
        "\"[function]\"\n{\"f\":\"[function]\"}\n\"NaN\"\n"
    );
    assert_eq!(
        encoded(
            &vm,
            "(function() local t = {}; t[1] = t; return json.encode_lines(t, { debug = true }) end)()"
        ),
        "\"[cycle]\"\n"
    );
}
