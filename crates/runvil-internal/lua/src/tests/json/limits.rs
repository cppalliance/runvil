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
