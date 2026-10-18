//! `json.encode`: compact text with sorted keys, lists against objects,
//! numbers, nulls, round trips through `json.decode`, and the depth limit.

use super::*;

#[test]
fn empty_tables_lists_and_objects_encode_compactly_with_sorted_keys() {
    let vm = json_vm();
    for (call, text) in [
        ("json.encode({})", "{}"),
        ("json.encode(json.array())", "[]"),
        ("json.encode(json.decode('[]'))", "[]"),
        ("json.encode({ b = 1, a = 2 })", r#"{"a":2,"b":1}"#),
        (
            "json.encode({ z = { 3, 2, 1 }, a = { y = true, x = false } })",
            r#"{"a":{"x":false,"y":true},"z":[3,2,1]}"#,
        ),
    ] {
        assert_eq!(encoded(&vm, call), text, "{call}");
    }
}

#[test]
fn scalars_nested_data_and_non_ascii_text_round_trip_through_decode() {
    let vm = json_vm();
    for text in [
        "1",
        "-5",
        "1.5",
        "true",
        "false",
        "null",
        r#""hi""#,
        r#""quote \" backslash \\ tab \t""#,
        "[[],{}]",
        r#"{"a":[1,{"b":null}],"c":"Zoë ✓ 日本","d":{"e":[true,false]}}"#,
    ] {
        let lua_text = text.replace('\\', "\\\\").replace('\'', "\\'");
        assert_eq!(
            encoded(&vm, &format!("json.encode(json.decode('{lua_text}'))")),
            text,
            "round trip of {text}"
        );
    }
}

#[test]
fn integer_valued_floats_in_the_i64_range_encode_as_integers() {
    let vm = json_vm();
    for (call, text) in [
        ("json.encode(2.0)", "2"),
        ("json.encode(-0.0)", "0"),
        ("json.encode(1.5)", "1.5"),
        ("json.encode(-2^63)", "-9223372036854775808"),
        ("json.encode(math.maxinteger)", "9223372036854775807"),
    ] {
        assert_eq!(encoded(&vm, call), text, "{call}");
    }
    let (kind, equal): (String, bool) = vm.eval(
        "local back = json.decode(json.encode(2^63))\n\
         return math.type(back), back == 2^63",
    );
    assert_eq!(
        (kind.as_str(), equal),
        ("float", true),
        "2^63 stays a float whose text reads back exactly"
    );
}

#[test]
fn nil_and_json_null_encode_as_null_at_the_top_and_inside_tables() {
    let vm = json_vm();
    for (call, text) in [
        ("json.encode()", "null"),
        ("json.encode(nil)", "null"),
        ("json.encode(json.null)", "null"),
        ("json.encode({ a = json.null })", r#"{"a":null}"#),
        ("json.encode({ json.null, 1 })", "[null,1]"),
        ("json.encode(json.decode('[1,null]'))", "[1,null]"),
    ] {
        assert_eq!(encoded(&vm, call), text, "{call}");
    }
}

#[test]
fn a_decoded_list_emptied_with_table_remove_encodes_as_an_empty_list() {
    let vm = json_vm();
    let text: String = vm.eval(
        "local list = json.decode('[1]')\n\
         table.remove(list)\n\
         return json.encode(list)",
    );
    assert_eq!(text, "[]");
}

#[test]
fn a_string_holding_a_newline_encodes_on_one_line() {
    let vm = json_vm();
    let text = encoded(&vm, r"json.encode({ s = 'a\nb' })");
    assert_eq!(text, r#"{"s":"a\nb"}"#);
    assert!(!text.contains('\n'), "required one line, actual {text:?}");
}

#[test]
fn tables_127_deep_encode_and_128_deep_raise_the_depth_message() {
    let vm = json_vm();
    let text = encoded(
        &vm,
        "(function()\n\
           local t = 1\n\
           for _ = 1, 127 do t = { t } end\n\
           return json.encode(t)\n\
         end)()",
    );
    assert_eq!(text, format!("{}1{}", "[".repeat(127), "]".repeat(127)));
    vm.exec("deep = 1\nfor _ = 1, 128 do deep = { deep } end");
    assert_eq!(
        caught(&vm, "json.encode(deep)"),
        lua_error(&format!(
            "json.encode: {} is nested more than 127 levels deep",
            nested_path(127)
        ))
    );
}
