//! `json.encode` and `json.encode_lines` refusing a value whose walk, shared
//! tables and repeated strings counted every time, outgrows the Lua heap.

use super::*;

/// A VM whose Lua heap is 4 MiB, so the walk budget is small and a refused
/// value costs little to build.
fn small_heap_vm() -> BuiltVm {
    TestVm::new()
        .limits(4 * 1024 * 1024, DEFAULT_LUA_LOG_EVENTS)
        .shims()
        .build()
}

/// Binds `t`, 19 levels of `{ t, t }` over one empty table: 20 tables in
/// memory and about 1 million values in the walk, more than the 524,288
/// that a 4 MiB heap allows.
fn bind_doubling_table(vm: &BuiltVm) {
    vm.exec("t = {} for i = 1, 19 do t = { t, t } end");
}

#[test]
fn a_shared_table_walked_past_the_value_budget_is_refused() {
    // Without a node count the walk visits the shared table once per
    // reference and encodes about 1 million values from 20 tables.
    let vm = small_heap_vm();
    bind_doubling_table(&vm);
    assert_eq!(
        caught(&vm, "json.encode(t)"),
        lua_error("json.encode: value is too large to encode")
    );
}

#[test]
fn a_repeated_string_walked_past_the_byte_budget_is_refused() {
    // 32 references to one 512 KiB string are 16 MiB of text against a
    // 4 MiB budget, though only one copy sits in the Lua heap.
    let vm = small_heap_vm();
    vm.exec("s = string.rep('x', 2^19) t = { s } for i = 1, 5 do t = { t, t } end");
    assert_eq!(
        caught(&vm, "json.encode(t)"),
        lua_error("json.encode: value is too large to encode")
    );
}

#[test]
fn debug_mode_raises_the_size_refusal_instead_of_a_placeholder() {
    // A placeholder per remaining value would still walk every value, so
    // the refusal skips debug mode's recovery.
    let vm = small_heap_vm();
    bind_doubling_table(&vm);
    assert_eq!(
        caught(&vm, "json.encode(t, { debug = true })"),
        lua_error("json.encode: value is too large to encode")
    );
}

#[test]
fn a_shared_chain_of_sealed_views_is_refused_by_its_hops_alone() {
    // `chain` is 1,000 sealed views over one empty table, so reading it
    // once costs about 1,000 hops. The doubling table reaches it through
    // 1,024 references: about 1 million hops against 524,288, while the
    // walk's own values number about 3,000. Only the charge per hop
    // refuses this.
    let vm = small_heap_vm();
    vm.exec(
        "chain = {}\n\
         for i = 1, 1000 do\n\
             chain = setmetatable({}, { __index = chain, __metatable = 'view' })\n\
         end\n\
         t = { chain } for i = 1, 10 do t = { t, t } end",
    );
    assert_eq!(encoded(&vm, "json.encode(chain)"), "{}");
    assert_eq!(
        caught(&vm, "json.encode(t)"),
        lua_error("json.encode: value is too large to encode")
    );
}

#[test]
fn encode_lines_refuses_an_oversized_item_under_its_own_name() {
    let vm = small_heap_vm();
    bind_doubling_table(&vm);
    assert_eq!(
        caught(&vm, "json.encode_lines({ t })"),
        lua_error("json.encode_lines: value is too large to encode")
    );
}

#[test]
fn a_list_longer_than_the_reference_stack_encodes_whole_instead_of_panicking() {
    // Every string value read at once holds one auxiliary reference slot,
    // and a Lua stack caps at 1,000,000 slots plus a few hundred that the
    // VM already uses, so mlua panics when the stack cannot grow. A list
    // of exactly 1,000,000 items still fits under that cap, so this one
    // goes 20% past it. Reading each value when its turn comes keeps the
    // live references to a handful. The list is far inside the 64 MiB
    // heap and the walk budget.
    let vm = json_vm();
    vm.exec("t = {} for i = 1, 1200000 do t[i] = 'a' end");
    // 1,200,000 items of `"a"`, 1,199,999 commas, and two brackets.
    assert_eq!(encoded(&vm, "json.encode(t)").len(), 4_800_001);
}

#[test]
fn sibling_objects_each_under_the_held_key_limit_encode_whole() {
    // The keys of a finished table stop counting once it is encoded:
    // 120,000 keys in all, but never more than 60,000 on one path.
    let vm = json_vm();
    vm.exec(
        "local function keyed(n) local o = {} for i = 1, n do o['k' .. i] = true end return o end\n\
         t = { keyed(60000), keyed(60000) }",
    );
    assert!(encoded(&vm, "json.encode(t)").starts_with("[{\"k1\":true,"));
}

#[test]
fn keys_held_by_tables_on_one_path_add_up_to_the_limit() {
    // A parent with 60,001 keys over a child with 60,000 holds 120,001
    // keys on the path to the child's values.
    let vm = json_vm();
    vm.exec(
        "local function keyed(n) local o = {} for i = 1, n do o['k' .. i] = true end return o end\n\
         t = keyed(60000) t.child = keyed(60000)",
    );
    assert_eq!(
        caught(&vm, "json.encode(t)"),
        lua_error("json.encode: value is too large to encode")
    );
}

#[test]
fn an_object_with_more_string_keys_than_one_path_may_hold_is_refused() {
    // 150,000 string keys are 150,000 live references, past the 100,000
    // that one encode path may hold. They would still fit the heap, so
    // only the held-key limit refuses this.
    let vm = json_vm();
    vm.exec("t = {} for i = 1, 150000 do t['k' .. i] = true end");
    assert_eq!(
        caught(&vm, "json.encode(t)"),
        lua_error("json.encode: value is too large to encode")
    );
}

#[test]
fn debug_mode_raises_the_held_key_refusal_instead_of_a_placeholder() {
    // Unlike a value that fails its own checks, a table that holds too
    // many keys cannot become a placeholder: reading its keys is the work
    // the limit refuses. The same 150,000-key object as above raises in
    // debug mode too.
    let vm = json_vm();
    vm.exec("t = {} for i = 1, 150000 do t['k' .. i] = true end");
    assert_eq!(
        caught(&vm, "json.encode(t, { debug = true })"),
        lua_error("json.encode: value is too large to encode")
    );
}

#[test]
fn an_options_table_with_more_string_keys_than_one_path_may_hold_is_refused() {
    // 100,001 string keys, one past the limit. The options table is read
    // before the value, so the size refusal replaces the `unknown option`
    // error that its first key would otherwise get.
    let vm = json_vm();
    vm.exec("o = {} for i = 1, 100001 do o['k' .. i] = true end");
    assert_eq!(
        caught(&vm, "json.encode({}, o)"),
        lua_error("json.encode: value is too large to encode")
    );
}
