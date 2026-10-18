//! `json.null` and the `json.array` list marker: what author code sees of
//! them, marking in place, decoded values staying writable, and the values
//! `json.array` refuses.

use super::*;

#[test]
fn json_null_is_a_truthy_userdata_equal_to_itself() {
    let vm = json_vm();
    let (kind, truthy, equal): (String, bool, bool) =
        vm.eval("return type(json.null), json.null and true or false, json.null == json.null");
    assert_eq!((kind.as_str(), truthy, equal), ("userdata", true, true));
}

#[test]
fn a_marked_list_hides_its_metatable_and_refuses_another() {
    let vm = json_vm();
    let (hidden, replaced, message): (bool, bool, String) = vm.eval(
        "local list = json.array()\n\
         local ok, err = pcall(setmetatable, list, {})\n\
         return getmetatable(list) == false, ok, tostring(err)",
    );
    assert!(hidden, "getmetatable on a marked list returns false");
    assert!(!replaced, "setmetatable on a marked list raises");
    assert_eq!(message, "cannot change a protected metatable");
}

#[test]
fn json_array_marks_a_table_in_place_and_returns_it() {
    let vm = json_vm();
    let (same, marked, count): (bool, bool, i64) = vm.eval(
        "local t = { 'a', 'b' }\n\
         return json.array(t) == t, getmetatable(t) == false, #t",
    );
    assert_eq!((same, marked, count), (true, true, 2));
    let (distinct, length, both_marked): (bool, i64, bool) = vm.eval(
        "local a, b = json.array(), json.array(nil)\n\
         return a ~= b, #a + #b, getmetatable(a) == false and getmetatable(b) == false",
    );
    assert_eq!(
        (distinct, length, both_marked),
        (true, 0, true),
        "no argument and nil each return a new empty marked list"
    );
}

#[test]
fn json_array_returns_a_decoded_list_unchanged() {
    let vm = json_vm();
    let (same, count, marked): (bool, i64, bool) = vm.eval(
        "local list = json.decode('[1,2]')\n\
         return json.array(list) == list, #list, getmetatable(list) == false",
    );
    assert_eq!((same, count, marked), (true, 2, true));
}

#[test]
fn a_decoded_list_emptied_with_table_remove_keeps_its_marker() {
    let vm = json_vm();
    let (count, marked): (i64, bool) = vm.eval(
        "local list = json.decode('[1]')\n\
         table.remove(list)\n\
         return #list, getmetatable(list) == false",
    );
    assert_eq!((count, marked), (0, true));
}

#[test]
fn decoded_objects_and_lists_take_writes() {
    let vm = json_vm();
    let (count, second, field, added): (i64, i64, String, bool) = vm.eval(
        r#"local v = json.decode('{"list":[1],"obj":{"a":1}}')
v.list[2] = 2
table.insert(v.list, 3)
v.obj.b = 'two'
v.added = true
return #v.list, v.list[2], v.obj.b, v.added"#,
    );
    assert_eq!((count, second, field.as_str(), added), (3, 2, "two", true));
}

#[test]
fn json_array_refuses_a_non_table_and_a_table_with_another_metatable() {
    let vm = json_vm();
    for (call, message) in [
        (
            "json.array(5)",
            "json.array expects a table or no argument, got integer",
        ),
        (
            "json.array(json.null)",
            "json.array expects a table or no argument, got lightuserdata",
        ),
        (
            "json.array(setmetatable({}, {}))",
            "json.array: the table already has a metatable",
        ),
        (
            "json.array(var)",
            "json.array: the table already has a metatable",
        ),
    ] {
        assert_eq!(caught(&vm, call), lua_error(message), "{call}");
    }
}
