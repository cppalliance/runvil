//! Tests for what the section's offers render: the static list in call
//! order, Pack expansion at the render, adhoc shadowing among offered
//! tools, and the dynamic list beside it.

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
fn an_offered_adhoc_tool_shadows_an_offered_catalog_tool_of_its_wire_name() {
    let shadowed = lua_over(offer_set());
    shadowed.exec(
        "tools.offer('web')\n\
         tools.offer(tools.adhoc({ name = 'web_search', description = 'Mine' }, function() end))",
    );
    assert_eq!(scoped(&shadowed), ["web_fetch", "web_search"]);
    let (set, runtime) = shadowed.tool_bag_handles();
    let bound = crate::current_tool_bindings(&set, &runtime, shadowed.adhoc_tools())
        .expect("the scope resolves");
    assert_eq!(
        bound[1].id().to_string(),
        "adhoc/web_search",
        "the adhoc tool is rendered and the catalog tool dropped where it was offered"
    );
    let unoffered = lua_over(offer_set());
    unoffered.exec(
        "tools.adhoc({ name = 'web_search', description = 'Mine' }, function() end)\n\
         tools.offer('web')",
    );
    let (set, runtime) = unoffered.tool_bag_handles();
    let ids: Vec<String> = crate::current_tool_bindings(&set, &runtime, unoffered.adhoc_tools())
        .expect("the scope resolves")
        .iter()
        .map(|binding| binding.id().to_string())
        .collect();
    assert_eq!(
        ids,
        ["web/fetch", "web/search"],
        "a defined but unoffered adhoc tool shadows nothing"
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
fn a_catalog_tool_a_static_adhoc_tool_shadows_stays_out_of_the_dynamic_list() {
    let vm = lua_over(offer_set());
    vm.exec(
        "tools.offer(tools.adhoc({ name = 'web_search', description = 'Mine' }, function() end))\n\
         tools.offer_dynamic('web')",
    );
    assert_eq!(scoped(&vm), ["web_search"]);
    assert_eq!(
        dynamic(&vm),
        ["web/fetch"],
        "a tool whose wire name the static list holds is skipped"
    );
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
        crate::current_tool_bindings(&set, &runtime, vm.adhoc_tools()).expect("the scope resolves")
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
