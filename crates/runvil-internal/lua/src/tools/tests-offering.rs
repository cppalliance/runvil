//! Tests for the id-keyed `tools` namespace: tool objects, `tools.offer`
//! over ids and tool objects, the `required` and `extras` lists,
//! `tools.get`, and `tools.offer_local`.

use std::collections::BTreeMap;

use mlua::{Lua, Value, Variadic};
use runvil_types::packs::PackId;

use crate::handles::ToolSet;
use crate::test_vm::{lua_over, scoped, tool_at};
use crate::tools::decode::collect_offer_entries;
use crate::{RenderedTools, ToolBinding};

/// A set declaring the Pack `web` and offering its two tools beside the
/// two of the undeclared Pack `gh`, in id order, under their wire names.
fn offering_set() -> ToolSet {
    ToolSet::new(
        vec![PackId::parse("web").expect("`web` is a valid Pack id")],
        std::collections::BTreeMap::new(),
        vec![
            ToolBinding::from_info("gh_issues_list", &tool_at("gh/issues.list")),
            ToolBinding::from_info("gh_search", &tool_at("gh/search")),
            ToolBinding::from_info("web_fetch", &tool_at("web/fetch")),
            ToolBinding::from_info("web_search", &tool_at("web/search")),
        ],
    )
}

#[test]
fn a_tool_object_reads_its_id_and_catalog_description_and_refuses_assignment() {
    let vm = lua_over(offering_set());
    let result: (String, String, bool, String) = vm.eval(
        "local tool = tools.get('gh/search')\n\
         tools.offer(tool)\n\
         local ok, err = pcall(function() tool.description = 'x' end)\n\
         return tool.id, tool.description, ok, tostring(err)",
    );
    assert_eq!(result.0, "gh/search");
    assert_eq!(
        result.1, "gh/search tool",
        "description is the catalog text"
    );
    assert!(!result.2, "assigning a field is refused");
    assert!(
        result
            .3
            .contains("Tool objects are frozen: cannot assign field \"description\""),
        "the refusal names the field: {}",
        result.3
    );
}

#[test]
fn offer_decodes_ids_tool_objects_and_arrays_into_wire_names() {
    let vm = lua_over(offering_set());
    vm.exec(
        "tools.offer('web/search')\n\
         tools.offer(tools.get('gh/search'))\n\
         tools.offer({ 'gh/issues.list', tools.get('web/fetch'), 'web/search' })\n\
         tools.offer()",
    );
    assert_eq!(
        scoped(&vm),
        ["web_search", "gh_search", "gh_issues_list", "web_fetch"],
        "each tool enters the scope once, under its wire name, in first-offer order"
    );
}

#[test]
fn offer_records_nothing_when_any_entry_is_not_a_catalog_tool() {
    for bad in ["gh/missing", "Not An Id", "gh_search", "helper"] {
        let vm = lua_over(offering_set());
        vm.exec("tools.offer_local('helper', 'a local helper', {}, function() return '' end)");
        let error: String = vm.eval(&format!(
            "local ok, err = pcall(tools.offer, {{ 'web/search', {bad:?} }})\n\
             assert(not ok, 'the bad entry is refused')\n\
             return tostring(err)"
        ));
        assert!(
            error.contains(&format!(
                "tools.offer: {bad:?} is not a catalog tool in this run"
            )),
            "tools.offer names {bad:?}: {error}"
        );
        assert!(
            scoped(&vm).is_empty(),
            "a refused call records no entry, the valid one included ({bad:?})"
        );
    }
}

#[test]
fn offering_a_tool_object_matches_offering_its_id() {
    let by_object = lua_over(offering_set());
    by_object.exec("tools.offer(tools.get('gh/search'))");
    let by_id = lua_over(offering_set());
    by_id.exec("tools.offer('gh/search')");
    assert_eq!(scoped(&by_object), scoped(&by_id));
    assert_eq!(scoped(&by_id), ["gh_search"]);
}

#[test]
fn required_and_extras_split_the_offering_by_declared_pack_and_share_objects() {
    let vm = lua_over(offering_set());
    let result: (String, String, bool, bool, bool, bool) = vm.eval(
        "local function ids(list)\n\
           local out = {}\n\
           for i, tool in ipairs(list) do out[i] = tool.id end\n\
           return table.concat(out, ',')\n\
         end\n\
         local required, extras = tools.required(), tools.extras()\n\
         return ids(required), ids(extras), \
           required[1] == tools.get('web/fetch'), \
           extras[2] == tools.get('gh/search'), \
           tools.required() ~= required, \
           tools.required()[2] == required[2]",
    );
    assert_eq!(
        result,
        (
            "web/fetch,web/search".to_owned(),
            "gh/issues.list,gh/search".to_owned(),
            true,
            true,
            true,
            true,
        ),
        "a declared Pack's tools are required, the rest extras, both in id \
         order, every list a fresh array of the same shared objects"
    );
}

#[test]
fn tools_get_reads_one_object_by_id_and_nil_for_anything_else() {
    let vm = lua_over(offering_set());
    let result: (String, bool, bool, bool, bool, String) = vm.eval(
        "tools.offer_local('helper', 'a local helper', {}, function() return '' end)\n\
         local ok, err = pcall(tools.get, 42)\n\
         assert(not ok, 'a non-string is refused')\n\
         return tools.get('gh/search').id, tools.get('gh/missing') == nil, \
           tools.get('helper') == nil, tools.get('Not An Id') == nil, \
           tools.get('gh_search') == nil, tostring(err)",
    );
    assert_eq!(result.0, "gh/search");
    assert!(
        result.1 && result.2 && result.3 && result.4,
        "a miss, a local alias, a malformed id, and a wire name all read nil: {result:?}"
    );
    assert!(
        result.5.contains("tools.get takes a tool id, got integer"),
        "the refusal names the type: {}",
        result.5
    );
}

#[test]
fn offer_local_registers_a_local_tool_and_names_itself_in_its_errors() {
    let vm = lua_over(offering_set());
    let errors: (String, String, String) = vm.eval(
        "tools.offer_local('gh_search', 'a local search', {}, function() return '' end)\n\
         local _, duplicate = pcall(tools.offer_local, 'gh_search', 'again', {}, function() end)\n\
         local _, param = pcall(tools.offer_local, 'other', 'x', { p = 'table' }, function() end)\n\
         local _, alias = pcall(tools.offer_local, 'web/search', 'x', {}, function() end)\n\
         return tostring(duplicate), tostring(param), tostring(alias)",
    );
    assert!(
        errors
            .0
            .contains("tools.offer_local alias \"gh_search\" is already registered"),
        "{}",
        errors.0
    );
    assert!(
        errors
            .1
            .contains("tools.offer_local param \"p\" has unsupported type \"table\""),
        "{}",
        errors.1
    );
    assert!(
        errors.2.contains("invalid alias \"web/search\""),
        "a local alias keeps the alias grammar: {}",
        errors.2
    );
}

#[test]
fn every_offer_function_raises_once_the_tool_list_is_rendered_before_any_other_check() {
    let vm = lua_over(offering_set());
    vm.exec(
        "tools.offer('web/search')\n\
         packs.offer('web')\n\
         tools.offer_local('helper', 'a local helper', {}, function() return '' end)",
    );
    let before = scoped(&vm);
    vm.messages().attach_rendered_tools(RenderedTools {
        schemas: Vec::new(),
        dispatch: BTreeMap::new(),
    });
    let refusals: Vec<String> = vm.eval(
        "local out = {}\n\
         for _, offer in ipairs({\n\
           function() tools.offer('gh/search') end,\n\
           function() tools.offer('web/search') end,\n\
           function() tools.offer('gh/missing') end,\n\
           function() packs.offer('web') end,\n\
           function() packs.offer('nope') end,\n\
           function() tools.offer_local('other', 'x', {}, function() return '' end) end,\n\
           function() tools.offer_local('helper', 'again', {}, function() return '' end) end,\n\
           function() tools.offer_local() end,\n\
         }) do\n\
           local ok, err = pcall(offer)\n\
           out[#out + 1] = ok and 'accepted' or tostring(err)\n\
         end\n\
         return out",
    );
    let fixed = "the section's tool list is fixed at its first models.loop round; make every \
                 offer before the first loop, or offer in another section";
    let calls = [
        "tools.offer",
        "tools.offer",
        "tools.offer",
        "packs.offer",
        "packs.offer",
        "tools.offer_local",
        "tools.offer_local",
        "tools.offer_local",
    ];
    assert_eq!(refusals.len(), calls.len(), "{refusals:?}");
    for (refusal, call) in refusals.iter().zip(calls) {
        assert!(
            refusal.contains(&format!("{call}: {fixed}")),
            "a new, a repeated, and an unknown entry, and a missing argument, all \
             raise the fixed-list refusal first: {refusal}"
        );
    }
    assert_eq!(scoped(&vm), before, "no refused offer reaches the scope");
    assert!(
        !vm.has_local_tool("other")
            .expect("the local-tools registry reads"),
        "no refused local offer registers"
    );
}

#[test]
fn the_tools_namespace_holds_exactly_its_five_functions() {
    let vm = lua_over(offering_set());
    let keys: String = vm.eval(
        "local keys = {}\n\
         for key, value in pairs(tools) do keys[#keys + 1] = key .. ':' .. type(value) end\n\
         table.sort(keys)\n\
         return table.concat(keys, ',')",
    );
    assert_eq!(
        keys, "extras:function,get:function,offer:function,offer_local:function,required:function",
        "the tools table holds exactly the namespace's functions"
    );
}

#[test]
fn tools_entries_name_the_calling_function_in_every_shape_error() {
    let lua = Lua::new();
    let string = |text: &str| {
        Value::String(
            lua.create_string(text)
                .expect("a fresh VM creates a short string"),
        )
    };
    let array = lua
        .create_sequence_from(vec![string("web/search")])
        .expect("a fresh VM creates the offer array");
    for (args, fragment) in [
        (
            vec![Value::Integer(42)],
            "tools.offer takes tool ids, tool objects, or arrays of them, got integer",
        ),
        (
            vec![string("web/search"), string("x")],
            "tools.offer takes one tool or a list of them, got extra string",
        ),
        (
            vec![Value::Table(array), string("x")],
            "tools.offer takes one tool or a list of them, got extra string",
        ),
        (
            vec![string("web/search"), Value::Integer(42)],
            "tools.offer takes one tool or a list of them, got extra integer",
        ),
    ] {
        let error = collect_offer_entries("tools.offer", Variadic::from_iter(args))
            .expect_err("the shape is refused");
        assert!(error.to_string().contains(fragment), "{fragment}: {error}");
    }
}
