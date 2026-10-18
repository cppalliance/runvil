//! `json.decode_lines`: a stream of values split on any whitespace, the
//! marked list it returns, and the errors it raises.

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
