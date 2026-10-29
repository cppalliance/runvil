//! Tests for the tools the section's offers name, which the merge takes:
//! the static offers in call order, Pack expansion at the freeze, offers
//! deduped by id only, the dynamic offers beside them, and the catalog
//! Packs those tools come from, in the order a render asks them.

use crate::test_vm::{dynamic, lua_over, offer_set, scoped};

#[test]
fn the_static_list_follows_call_order_whichever_comes_first() {
    for (source, expected) in [
        (
            "tools.offer('web')\ntools.offer('web/search')",
            vec!["web_fetch", "web_search"],
        ),
        (
            "tools.offer('web/search')\ntools.offer('web')",
            vec!["web_search", "web_fetch"],
        ),
        (
            "tools.offer(grab)\ntools.offer('web')",
            vec!["grab", "web_fetch", "web_search"],
        ),
    ] {
        let vm = lua_over(offer_set());
        vm.exec(&format!(
            "local grab = tools.adhoc({{ name = 'grab', description = 'Grab a value' }}, \
               function() end)\n{source}"
        ));
        assert_eq!(scoped(&vm), expected, "{source}");
    }
}

#[test]
fn repeated_offers_neither_duplicate_nor_move_a_tool() {
    let vm = lua_over(offer_set());
    vm.exec(
        "tools.offer('web/fetch')\n\
         tools.offer('gh')\n\
         tools.offer('web')\n\
         tools.offer('web/fetch')\n\
         tools.offer({ 'gh/search', tools.packs.get('gh') })",
    );
    assert_eq!(
        scoped(&vm),
        ["web_fetch", "gh_issues_list", "gh_search", "web_search"],
        "a tool followed by its Pack keeps the tool's place, and no repeat moves it"
    );
}

#[test]
fn the_adhoc_pack_expands_at_the_render_while_its_tools_array_is_read_at_the_call() {
    let by_pack = lua_over(offer_set());
    by_pack.exec(
        "tools.adhoc({ name = 'zeta', description = 'Z' }, function() end)\n\
         tools.offer(tools.packs.get('adhoc'))\n\
         tools.adhoc({ name = 'alpha', description = 'A' }, function() end)",
    );
    assert_eq!(
        scoped(&by_pack),
        ["alpha", "zeta"],
        "a Pack offer takes every adhoc tool defined before the render, in id order"
    );
    let by_tools = lua_over(offer_set());
    by_tools.exec(
        "tools.adhoc({ name = 'zeta', description = 'Z' }, function() end)\n\
         tools.offer(tools.packs.get('adhoc').tools)\n\
         tools.adhoc({ name = 'alpha', description = 'A' }, function() end)",
    );
    assert_eq!(
        scoped(&by_tools),
        ["zeta"],
        "a tools array takes only the tools defined when it was read"
    );
}

#[test]
fn offers_that_share_a_wire_name_all_reach_the_merge_and_only_an_id_dedupes() {
    let vm = lua_over(offer_set());
    vm.exec(
        "tools.offer('web')\n\
         tools.offer(tools.adhoc({ name = 'web_search', description = 'Mine' }, function() end))\n\
         tools.offer_dynamic('web')",
    );
    let (set, runtime) = vm.tool_bag_handles();
    let ids: Vec<String> = crate::current_static_set(&set, &runtime, vm.adhoc_tools())
        .expect("the static offers resolve")
        .iter()
        .map(|tool| tool.id().to_string())
        .collect();
    assert_eq!(
        ids,
        ["web/fetch", "web/search", "adhoc/web_search"],
        "an adhoc tool beside a catalog tool of its wire name drops neither"
    );
    assert!(
        dynamic(&vm).is_empty(),
        "a dynamic offer of ids the static offers hold adds nothing"
    );
}

#[test]
fn the_dynamic_list_keeps_what_the_static_list_lacks_in_offer_order_once() {
    for source in [
        "tools.offer('web/search')\ntools.offer_dynamic('web')",
        "tools.offer_dynamic('web')\ntools.offer('web/search')",
    ] {
        let vm = lua_over(offer_set());
        vm.exec(source);
        assert_eq!(scoped(&vm), ["web_search"], "{source}");
        assert_eq!(
            dynamic(&vm),
            ["web/fetch"],
            "static wins in either order: {source}"
        );
    }
    let vm = lua_over(offer_set());
    vm.exec(
        "tools.offer_dynamic('gh/search')\n\
         tools.offer_dynamic({ 'gh', 'gh/search' })\n\
         tools.offer_dynamic(tools.packs.get('adhoc'))\n\
         tools.adhoc({ name = 'late', description = 'Late' }, function() end)",
    );
    assert_eq!(
        dynamic(&vm),
        ["gh/search", "gh/issues.list", "adhoc/late"],
        "each tool once, in offer order, adhoc tools defined before the render included"
    );
    assert!(
        scoped(&vm).is_empty(),
        "dynamic offers never reach the static list"
    );
}

#[test]
fn each_catalog_pack_comes_once_where_one_of_its_tools_was_first_offered_into_either_set() {
    let vm = lua_over(offer_set());
    vm.exec(
        "tools.offer(tools.adhoc({ name = 'grab', description = 'Grab a value' }, function() end))\n\
         tools.offer_dynamic('gh/search')\n\
         tools.offer('web')\n\
         tools.offer('gh/search')\n\
         tools.offer_dynamic('web/fetch')",
    );
    let (set, runtime) = vm.tool_bag_handles();
    let packs: Vec<String> = crate::current_catalog_packs(&set, &runtime, vm.adhoc_tools())
        .expect("the offers resolve")
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        packs,
        ["gh", "web"],
        "an adhoc tool names no catalog Pack, and gh keeps the place of its first offer"
    );
    assert_eq!(
        scoped(&vm),
        ["grab", "web_fetch", "web_search", "gh_search"],
        "though the static offer that came later holds its tool"
    );
    assert!(dynamic(&vm).is_empty());
}

#[test]
fn dynamic_offers_leave_the_static_bindings_unchanged() {
    let plain = lua_over(offer_set());
    plain.exec("tools.offer({ 'web/search', 'gh' })");
    let with_dynamic = lua_over(offer_set());
    with_dynamic.exec(
        "tools.offer_dynamic('web')\n\
         tools.offer({ 'web/search', 'gh' })\n\
         tools.offer_dynamic(tools.packs)",
    );
    let bindings = |vm: &crate::SectionVm| {
        let (set, runtime) = vm.tool_bag_handles();
        crate::current_static_set(&set, &runtime, vm.adhoc_tools()).expect("the scope resolves")
    };
    assert_eq!(bindings(&plain), bindings(&with_dynamic));
}

#[test]
fn the_same_offers_give_the_same_lists_in_every_vm() {
    let source = "tools.adhoc({ name = 'b', description = 'B' }, function() end)\n\
                  tools.adhoc({ name = 'a', description = 'A' }, function() end)\n\
                  tools.offer(tools.packs)\n\
                  tools.offer_dynamic(tools.packs.extras())";
    let lists: Vec<(Vec<String>, Vec<String>)> = (0..4)
        .map(|_| {
            let vm = lua_over(offer_set());
            vm.exec(source);
            (scoped(&vm), dynamic(&vm))
        })
        .collect();
    assert!(
        lists.windows(2).all(|pair| pair[0] == pair[1]),
        "nothing is hash-ordered: {lists:?}"
    );
    assert_eq!(lists[0].0[..2], ["a", "b"]);
}
