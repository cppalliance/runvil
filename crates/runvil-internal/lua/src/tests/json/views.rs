//! `json.encode` reading through sealed views: `var`, frozen `argv`, and
//! the views it refuses, `sys` and message-record views.

use super::*;

#[test]
fn var_and_its_tables_encode_their_stored_data() {
    let seed = json!({
        "items": [],
        "cfg": { "mode": "fast", "list": [1, 2] },
    });
    let vm = TestVm::new().var(&seed).shims().build();
    for (call, text) in [
        ("json.encode(var.items)", "[]"),
        ("json.encode(var.cfg.list)", "[1,2]"),
        (
            "json.encode(var)",
            r#"{"cfg":{"list":[1,2],"mode":"fast"},"items":[]}"#,
        ),
    ] {
        assert_eq!(encoded(&vm, call), text, "{call}");
    }
    vm.exec("var.rows = { { n = 1 }, json.array() }\nvar.empty = json.array()");
    assert_eq!(encoded(&vm, "json.encode(var.rows)"), r#"[{"n":1},[]]"#);
    assert_eq!(encoded(&vm, "json.encode(var.empty)"), "[]");
}

#[test]
fn frozen_argv_encodes_its_stored_data() {
    let argv = json!({ "query": "papers", "tags": [], "nested": { "hits": [1, 2] } });
    let vm = TestVm::new()
        .argv(Argv::Frozen(Some(&argv)))
        .shims()
        .build();
    for (call, text) in [
        ("json.encode(argv.tags)", "[]"),
        (
            "json.encode(argv)",
            r#"{"nested":{"hits":[1,2]},"query":"papers","tags":[]}"#,
        ),
    ] {
        assert_eq!(encoded(&vm, call), text, "{call}");
    }
}

#[test]
fn frozen_and_writable_argv_encode_nested_nulls_alike() {
    let argv = json!({ "ids": [1, null, 3], "tail": [1, null], "a": null });
    let frozen = TestVm::new()
        .argv(Argv::Frozen(Some(&argv)))
        .shims()
        .build();
    let writable = TestVm::new()
        .argv(Argv::Writable(Some(&argv)))
        .shims()
        .build();
    for (mode, vm) in [("frozen", &frozen), ("writable", &writable)] {
        assert_eq!(
            encoded(vm, "json.encode(argv)"),
            r#"{"a":null,"ids":[1,null,3],"tail":[1,null]}"#,
            "{mode} argv"
        );
    }
}

#[test]
fn a_var_value_127_levels_deep_encodes_because_views_add_no_level() {
    let mut deep = json!(1);
    for _ in 0..127 {
        deep = json!([deep]);
    }
    let seed = json!({ "deep": deep });
    let vm = TestVm::new().var(&seed).shims().build();
    assert_eq!(
        encoded(&vm, "json.encode(var.deep)"),
        format!("{}1{}", "[".repeat(127), "]".repeat(127))
    );
}

#[test]
fn sys_cannot_be_read_and_debug_mode_names_its_label() {
    let vm = json_vm();
    assert_eq!(
        caught(&vm, "json.encode(sys)"),
        lua_error(
            "json.encode: value cannot be read (sys is sealed); \
             copy the fields you need into a plain table"
        )
    );
    assert_eq!(
        caught(&vm, "json.encode({ meta = sys })"),
        lua_error(
            "json.encode: value.meta cannot be read (sys is sealed); \
             copy the fields you need into a plain table"
        )
    );
    assert_eq!(
        caught(
            &vm,
            "json.encode(setmetatable({}, { __index = function() end, __metatable = {} }))"
        ),
        lua_error("json.encode: value cannot be read; copy the fields you need into a plain table")
    );
    assert_eq!(
        encoded(&vm, "json.encode(sys, { debug = true })"),
        r#""[cannot read: sys is sealed]""#
    );
}

#[test]
fn a_message_record_view_is_not_json_data() {
    let vm = json_vm();
    vm.exec("messages:user('u')\nrecord = messages[1]");
    assert_eq!(
        caught(&vm, "json.encode(record)"),
        lua_error("json.encode: value must be JSON data, got userdata")
    );
    assert_eq!(
        encoded(&vm, "json.encode(record, { debug = true })"),
        r#""[userdata]""#
    );
}

#[test]
fn a_table_whose_index_is_neither_a_table_nor_a_function_encodes_its_own_entries() {
    let vm = json_vm();
    for (call, text) in [
        (
            "json.encode(setmetatable({}, { __index = 5, __metatable = 'locked' }))",
            "{}",
        ),
        (
            "json.encode(setmetatable({ a = 1 }, { __index = { b = 2 }, __metatable = 'locked' }))",
            r#"{"a":1}"#,
        ),
        (
            "json.encode(setmetatable({}, { __index = { b = 2 } }))",
            "{}",
        ),
        (
            "json.encode(setmetatable({}, { __index = { b = 2 }, __metatable = 'view' }))",
            r#"{"b":2}"#,
        ),
    ] {
        assert_eq!(encoded(&vm, call), text, "{call}");
    }
}

#[test]
fn a_view_whose_data_table_contains_the_view_reports_the_cycle() {
    let vm = json_vm();
    vm.exec(
        "data = {}\n\
         view = setmetatable({}, { __index = data, __metatable = 'view' })\n\
         data.again = view",
    );
    assert_eq!(
        caught(&vm, "json.encode(view)"),
        lua_error("json.encode: value.again contains itself")
    );
    assert_eq!(
        caught(&vm, "json.encode({ view, view })"),
        lua_error("json.encode: value[1].again contains itself")
    );
}
