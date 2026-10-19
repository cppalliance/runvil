//! Compatibility-chunk returns, the read-only `sys` table, and the guarded
//! `var` table.

use super::*;

#[test]
fn returns_args_verbatim() {
    assert_eq!(
        run("return args", "hello").unwrap().returned.as_deref(),
        Some("hello")
    );
}

#[test]
fn expression_only_compatibility_chunk_returns_its_value() {
    assert_eq!(run("42", "").unwrap().returned.as_deref(), Some("42"));
}

#[test]
fn no_return_is_none() {
    assert_eq!(run("local x = 1", "hello").unwrap().returned, None);
}

#[test]
fn reads_sys() {
    assert_eq!(
        run("return sys.id", "").unwrap().returned.as_deref(),
        Some("1")
    );
    assert_eq!(
        run("return sys.when", "").unwrap().returned.as_deref(),
        Some("t")
    );
}

#[test]
fn unknown_sys_field_is_a_lua_error() {
    let error = run("return sys.bogus", "").expect_err("missing sys field must fail");
    assert!(
        error.to_string().contains("unknown sys field 'bogus'"),
        "error was {error}"
    );
}

#[test]
fn writing_sys_field_is_a_lua_error() {
    let existing = run("sys.when = 'x'", "").expect_err("writing an existing sys field must fail");
    assert!(
        existing
            .to_string()
            .contains("sys is read-only; cannot set 'when'"),
        "error was {existing}"
    );

    let created = run("sys.extra = 1", "").expect_err("creating a sys field must fail");
    assert!(
        created
            .to_string()
            .contains("sys is read-only; cannot set 'extra'"),
        "error was {created}"
    );
}

#[test]
fn a_sys_write_raises_the_whole_refusal_for_a_string_integer_or_other_key() {
    for (target, refusal) in [
        ("sys.extra", "sys is read-only; cannot set 'extra'"),
        ("sys[1]", "sys is read-only; cannot set '1'"),
        ("sys[true]", "sys is read-only; cannot set 'boolean'"),
    ] {
        let source =
            format!("local ok, err = pcall(function() {target} = 1 end)\nreturn tostring(err)");
        let out = run(&source, "").expect("the pcall catches the refusal");
        let caught = out.returned.expect("the chunk returns the caught error");
        assert_eq!(refusal_line(&caught), Some(refusal), "writing {target}");
    }
    let label = run("return getmetatable(sys)", "").unwrap();
    assert_eq!(label.returned.as_deref(), Some("sys is sealed"));
}

#[test]
fn var_is_read_back() {
    let out = run("var.greeting = 'hi ' .. args", "bob").unwrap();
    assert_eq!(
        out.var.get("greeting").and_then(|v| v.as_str()),
        Some("hi bob")
    );
}

#[test]
fn var_guard_allows_json_data_and_reads_back() {
    let out = run(
        "var.n = 1\nvar.s = 'x'\nvar.t = { a = {1, 2} }\nvar.b = true",
        "",
    )
    .expect("JSON data writes must pass the guard");
    assert_eq!(
        out.var,
        json!({ "n": 1, "s": "x", "t": { "a": [1, 2] }, "b": true })
    );
}

#[test]
fn var_rejects_a_function_at_the_assigning_line() {
    let error = run("var.f = function() end", "")
        .expect_err("a function assigned into var must fail at the assigning line");
    assert!(
        error
            .to_string()
            .contains("var.f must be JSON data, got function"),
        "error was {error}"
    );
}

#[test]
fn var_rejects_a_nested_function_at_the_assigning_line() {
    let error = run("var.t = { f = function() end }", "")
        .expect_err("a nested function must fail the deep check at the assigning line");
    assert!(
        error.to_string().contains("function"),
        "the bridge error must name the offending type: {error}"
    );
}

#[test]
fn var_guard_error_is_catchable_at_the_assigning_line() {
    // A pcall around the write catches the guard's error, proving the failure
    // is raised by that statement rather than later at serialization. The
    // caught value is mlua's error userdata, so stringify before matching.
    let out = run(
        "local ok, err = pcall(function() var.f = function() end end)\n\
         assert(not ok, 'the write must fail')\n\
         assert(tostring(err):match('must be JSON data'), tostring(err))\n\
         var.kept = 'yes'\n\
         return var.kept",
        "",
    )
    .expect("the caught guard error must not fail the chunk");
    assert_eq!(out.returned.as_deref(), Some("yes"));
    assert_eq!(out.var.get("kept").and_then(|v| v.as_str()), Some("yes"));
}

#[test]
fn var_guard_rejects_incremental_nested_function_writes_at_the_assigning_line() {
    let out = run(
        "var.t = {}\n\
         local ok, err = pcall(function() var.t.f = function() end end)\n\
         assert(not ok, 'the nested write must fail')\n\
         assert(tostring(err):match('var.t.f must be JSON data'), tostring(err))\n\
         var.t.kept = 'yes'\n\
         return var.t.kept",
        "",
    )
    .expect("the nested guard error must remain catchable");
    assert_eq!(out.returned.as_deref(), Some("yes"));
    assert_eq!(out.var, json!({ "t": { "kept": "yes" } }));
}

#[test]
fn assigning_nil_to_a_var_field_removes_it_from_var_and_its_snapshot() {
    let lua = Lua::new();
    let var = guarded_var(&lua, Some(&json!({ "k": 1, "kept": "yes" })))
        .expect("the guarded var builds from a JSON object");
    lua.globals()
        .raw_set("var", var)
        .expect("a fresh VM's globals take the var table");
    let reads_nil: bool = lua
        .load("var.k = nil\nreturn var.k == nil")
        .eval()
        .expect("assigning nil passes the guard");
    assert!(reads_nil, "var.k must read as nil after `var.k = nil`");
    let snapshot = var_snapshot_table(&lua).expect("the snapshot reads back");
    let keys = snapshot
        .pairs::<String, Value>()
        .map(|pair| pair.map(|(key, _)| key))
        .collect::<mlua::Result<Vec<_>>>()
        .expect("the snapshot iterates");
    assert_eq!(keys, ["kept"], "the snapshot must drop the removed key");
    assert_eq!(
        var_to_json(&lua).expect("var reads back"),
        json!({ "kept": "yes" })
    );
}

#[test]
fn an_empty_marked_list_in_var_reads_back_as_an_empty_array() {
    for source in ["var.items = json.array()", "var.items = json.decode('[]')"] {
        let out = run(source, "").expect("a marked list passes the guard");
        assert_eq!(out.var, json!({ "items": [] }), "after {source}");
    }
}

#[test]
fn a_nested_marked_list_reads_back_as_an_array_and_a_plain_empty_table_as_an_object() {
    let nested = run("var.t = { list = json.array() }", "").expect("the nested list passes");
    assert_eq!(nested.var, json!({ "t": { "list": [] } }));
    let plain = run("var.items = {}", "").expect("an empty table passes the guard");
    assert_eq!(plain.var, json!({ "items": {} }));
}

#[test]
fn an_empty_list_seeded_from_rust_reads_back_as_an_empty_array() {
    let seed = json!({ "items": [] });
    let vm = TestVm::new().var(&seed).build();
    assert_eq!(vm.var().expect("var reads back"), json!({ "items": [] }));
}

#[test]
fn json_null_in_var_reads_back_as_null_and_equals_json_null_in_lua() {
    let out = run("var.x = json.null\nreturn tostring(var.x == json.null)", "")
        .expect("json.null passes the guard");
    assert_eq!(out.returned.as_deref(), Some("true"));
    assert_eq!(out.var, json!({ "x": null }));
}

#[test]
fn reassigning_the_var_global_fails_read_back() {
    // `var = 5` drops the guarded proxy from reach; read-back must reject it
    // rather than silently roll the pre-reassignment data forward.
    let error = run("var = 5", "").expect_err("reassigning the var global must fail at read-back");
    assert!(
        error.to_string().contains("`var` global was reassigned"),
        "the error must name the reassignment: {error}"
    );
}

/// A VM whose Lua heap is 4 MiB, so a write may read at most 524,288 values
/// and 4 MiB of text.
fn small_heap_vm() -> BuiltVm {
    TestVm::new()
        .limits(4 * 1024 * 1024, DEFAULT_LUA_LOG_EVENTS)
        .shims()
        .build()
}

/// Runs `var.x = t` on `vm` under `pcall` and returns whether it raised, the
/// error's kind, and its message.
fn var_write_of_t(vm: &BuiltVm) -> (bool, String, String) {
    vm.eval(
        "local ok, err = pcall(function() var.x = t end)\n\
         if ok then return false, '', '' end\n\
         return true, tostring(err.kind), tostring(err)",
    )
}

#[test]
fn a_var_write_of_a_shared_table_past_the_budget_is_refused() {
    // `t` is 19 levels of `{ t, t }` over one empty table: 20 tables in
    // memory, but about 1 million values in a walk that reads a shared table
    // once per reference, against the 524,288 that a 4 MiB heap allows.
    // Without the check the conversion builds the whole tree on the Rust
    // heap first and then fails differently, on a Lua memory error from
    // rebuilding it.
    let vm = small_heap_vm();
    vm.eval::<()>("t = {} for i = 1, 19 do t = { t, t } end");
    let (raised, kind, message) = var_write_of_t(&vm);
    assert!(raised, "the oversized write must raise, not store");
    assert_eq!(kind, "lua");
    assert_eq!(refusal_line(&message), Some("var.x is too large to store"));
}

#[test]
fn a_var_write_of_a_repeated_string_past_the_byte_budget_is_refused() {
    // 32 references to one 512 KiB string are 16 MiB of text against a 4 MiB
    // budget, though only one copy sits in the Lua heap and the walk has
    // about 63 values. Only the charge for string values refuses this.
    let vm = small_heap_vm();
    vm.eval::<()>("s = string.rep('x', 2^19) t = { s } for i = 1, 5 do t = { t, t } end");
    let (raised, kind, message) = var_write_of_t(&vm);
    assert!(raised, "the oversized write must raise, not store");
    assert_eq!(kind, "lua");
    assert_eq!(refusal_line(&message), Some("var.x is too large to store"));
}

#[test]
fn a_var_write_of_a_repeated_string_key_past_the_byte_budget_is_refused() {
    // 32 references to one table whose only key is 512 KiB are 16 MiB of key
    // text against a 4 MiB budget, and the value under the key is a number.
    // Only the charge for string keys refuses this.
    let vm = small_heap_vm();
    vm.eval::<()>(
        "local key = string.rep('x', 2^19)\n\
         t = { { [key] = 1 } } for i = 1, 5 do t = { t, t } end",
    );
    let (raised, kind, message) = var_write_of_t(&vm);
    assert!(raised, "the oversized write must raise, not store");
    assert_eq!(kind, "lua");
    assert_eq!(refusal_line(&message), Some("var.x is too large to store"));
}

#[test]
fn a_var_write_of_a_table_that_contains_itself_keeps_the_recursion_error() {
    // `t` holds itself twice, so a walk that followed the cycle would branch
    // two ways at each of 200 levels and run out of budget, reporting the
    // write as too large. The walk stops at the table already on its path
    // and leaves the cycle to the conversion, whose recursion error stays.
    let vm = small_heap_vm();
    vm.eval::<()>("t = {} t.a = t t.b = t");
    let (raised, _kind, message) = var_write_of_t(&vm);
    assert!(raised, "a cyclic write must raise, not store");
    assert!(
        message.contains("recursive table"),
        "the conversion's own error must stay, got {message}"
    );
    assert!(
        !message.contains("too large to store"),
        "a cycle is not oversized, got {message}"
    );
}
