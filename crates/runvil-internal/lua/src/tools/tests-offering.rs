//! Tests for the offer verbs, `tools.offer` and `tools.offer_dynamic`:
//! every accepted form, the string-shape rule, each refusal, the
//! rendered-list refusal first, and the `tools` table they sit on.

use std::collections::BTreeMap;

use crate::RenderedTools;
use crate::handles::ToolSet;
use crate::test_vm::{lua_over, offer_set, offers, raised, scoped, tool_at};

/// The adhoc tool `grab` the forms below offer as an object.
const GRAB: &str =
    "local grab = tools.adhoc({ name = 'grab', description = 'Grab a value' }, function() end)\n";

/// Every accepted form, each line through `verb`.
fn forms(verb: &str) -> String {
    format!(
        "{GRAB}\
         assert(select('#', {verb}('web/search')) == 0, 'the verb returns nothing')\n\
         {verb}('web')\n\
         {verb}(grab)\n\
         {verb}(tools.packs.get('web'))\n\
         {verb}(tools.get('web/search').pack)\n\
         {verb}({{ 'web/search', grab, 'subagents', tools.packs.get('user-input') }})\n\
         {verb}(tools.packs.get('web').tools)\n\
         {verb}(tools.packs.get('adhoc'))\n\
         {verb}(tools.packs.extras())\n\
         {verb}(tools.packs)\n\
         {verb}()\n\
         {verb}({{}})"
    )
}

#[test]
fn every_accepted_form_records_each_target_once_in_call_order_under_its_verbs_mode() {
    for (verb, mode) in [
        ("tools.offer", "static"),
        ("tools.offer_dynamic", "dynamic"),
    ] {
        let vm = lua_over(offer_set());
        vm.exec(&forms(verb));
        let expected: Vec<String> = [
            "web/search",
            "web",
            "adhoc/grab",
            "subagents",
            "user-input",
            "web/fetch",
            "adhoc",
            "gh",
        ]
        .iter()
        .map(|target| format!("{mode}:{target}"))
        .collect();
        assert_eq!(
            offers(&vm),
            expected,
            "{verb}: ids, Packs, objects, and arrays record their targets, an equal \
             offer is skipped, and no argument or an empty array records nothing"
        );
    }
}

#[test]
fn the_two_verbs_record_separately_so_the_same_target_takes_both_modes() {
    let vm = lua_over(offer_set());
    vm.exec(
        "tools.offer('web')\n\
         tools.offer_dynamic('web')\n\
         tools.offer('web')",
    );
    assert_eq!(offers(&vm), ["static:web", "dynamic:web"]);
}

#[test]
fn a_string_with_a_slash_is_a_tool_id_and_one_without_is_a_pack_id() {
    let vm = lua_over(offer_set());
    vm.exec(
        "tools.adhoc({ name = 'Shout', description = 'Uppercase text' }, function() end)\n\
         tools.offer('web/search')\n\
         tools.offer('gh')\n\
         tools.offer('adhoc/shout')\n\
         tools.offer('adhoc')",
    );
    assert_eq!(
        offers(&vm),
        [
            "static:web/search",
            "static:gh",
            "static:adhoc/shout",
            "static:adhoc"
        ]
    );
    let pack = |name: &str| {
        format!(
            "tools.offer: no Pack named \"{name}\" in this run; declare it under uses: to \
             require it, or offer it only when tools.packs.get(\"{name}\") returns it"
        )
    };
    for (call, expected) in [
        ("tools.offer('Web')", pack("Web")),
        (
            "tools.offer('Shout')",
            format!("{} (did you mean 'adhoc/shout'?)", pack("Shout")),
        ),
        (
            "tools.offer('web//x')",
            "tools.offer: \"web//x\" is not a tool in this run".to_owned(),
        ),
        (
            "tools.offer('adhoc/other')",
            "tools.offer: \"adhoc/other\" is not a tool in this run".to_owned(),
        ),
    ] {
        let error = raised(&vm, call);
        assert!(
            error.contains(&expected),
            "an invalid id takes the message of an unknown one of its shape: {call}: {error}"
        );
    }
}

#[test]
fn a_catalog_tool_object_under_the_adhoc_pack_is_refused_as_an_unknown_tool() {
    let catalog_adhoc = crate::ToolBinding::from_info("adhoc_x", &tool_at("adhoc/x"));
    let vm = lua_over(ToolSet::new(
        Vec::new(),
        BTreeMap::new(),
        vec![catalog_adhoc],
    ));
    let error: String = vm.eval(
        "local ok, err = pcall(tools.offer, tools.get('adhoc/x'))\n\
         assert(not ok, 'offering the catalog adhoc/x object raises')\n\
         return tostring(err)",
    );
    assert!(
        error.contains("tools.offer: \"adhoc/x\" is not a tool in this run"),
        "the object takes the string form's message: {error}"
    );
    assert!(offers(&vm).is_empty(), "the refused offer records nothing");
    assert!(
        scoped(&vm).is_empty(),
        "the scope resolves without an adhoc/x binding"
    );
}

#[test]
fn each_refusal_names_its_verb_and_a_refused_call_records_nothing() {
    for verb in ["tools.offer", "tools.offer_dynamic"] {
        let vm = lua_over(offer_set());
        let unknown_pack = |name: &str| {
            format!(
                "{verb}: no Pack named \"{name}\" in this run; declare it under uses: to \
                 require it, or offer it only when tools.packs.get(\"{name}\") returns it"
            )
        };
        let shape = format!(
            "{verb} takes tool ids, Pack ids, tool objects, Pack objects, or an array of them, got"
        );
        let entry = format!(
            "{verb} takes tool ids, Pack ids, tool objects, or Pack objects in an array, got"
        );
        let unavailable =
            format!("{verb}: the mcp Pack is unavailable in this run: the mcp server is down");
        for (call, expected) in [
            ("'wbe'", unknown_pack("wbe")),
            (
                "'web_search'",
                format!(
                    "{} (did you mean 'web/search'?)",
                    unknown_pack("web_search")
                ),
            ),
            (
                "'web/serch'",
                format!("{verb}: \"web/serch\" is not a tool in this run"),
            ),
            ("'mcp'", unavailable.clone()),
            (
                "'web/search', 'web/fetch'",
                format!("{verb} takes one tool or Pack, or an array of them, got extra string"),
            ),
            ("42", format!("{shape} integer")),
            ("nil", format!("{shape} nil")),
            ("tools.get('web/serch')", format!("{shape} nil")),
            ("messages", format!("{shape} userdata")),
            (
                "{ 'web', nil, 'mcp' }",
                format!("{verb} takes an array, got a table with the key 3"),
            ),
            (
                "setmetatable({ 'web' }, { __len = function() return 2 end })",
                format!("{entry} nil at index 2"),
            ),
            ("{ { 'web' } }", format!("{entry} table at index 1")),
            ("{ 'web', 42 }", format!("{entry} integer at index 2")),
            (
                "{ name = 'web' }",
                format!("{verb} takes an array, got a table with the key name"),
            ),
            (
                "setmetatable({ 'web' }, { __len = function() return 0 end })",
                format!("{verb} takes an array, got a table with the key 1"),
            ),
            ("{ 'web', 'mcp' }", unavailable.clone()),
            ("{ 'web/search', 'wbe' }", unknown_pack("wbe")),
        ] {
            let error = raised(&vm, &format!("{verb}({call})"));
            assert!(error.contains(&expected), "{verb}({call}): {error}");
        }
        assert!(
            offers(&vm).is_empty(),
            "{verb}: a refused call records no entry, its valid ones included"
        );
    }
}

#[test]
fn an_array_is_read_through_its_metamethods() {
    let vm = lua_over(offer_set());
    vm.exec(
        "local entries = { 'gh', 'web/fetch' }\n\
         tools.offer(setmetatable({}, {\n\
           __len = function() return #entries end,\n\
           __index = function(_, i) return entries[i] end,\n\
         }))",
    );
    assert_eq!(offers(&vm), ["static:gh", "static:web/fetch"]);
}

#[test]
fn the_render_refusal_comes_before_every_other_check_and_defining_still_works() {
    let vm = lua_over(offer_set());
    vm.exec("tools.offer('web/search')");
    vm.messages().attach_rendered_tools(RenderedTools {
        schemas: Vec::new(),
        dispatch: BTreeMap::new(),
        dynamic: Vec::new(),
    });
    for verb in ["tools.offer", "tools.offer_dynamic"] {
        for args in ["'gh'", "'web/search'", "'wbe'", "42", "", "'web', 'gh'"] {
            let error = raised(&vm, &format!("{verb}({args})"));
            assert!(
                error.contains(&format!(
                    "{verb}: the section's tool list is fixed at its first models.loop \
                     round; make every offer before the first loop, or offer in another section"
                )),
                "{verb}({args}) raises the fixed-list refusal first: {error}"
            );
        }
    }
    let defined: bool = vm.eval(
        "local late = tools.adhoc({ name = 'late', description = 'Late' }, function() end)\n\
         return tools.get('adhoc/late') == late",
    );
    assert!(defined, "defining an adhoc tool works after the render");
    assert_eq!(
        offers(&vm),
        ["static:web/search"],
        "no refused offer records"
    );
}

#[test]
fn offering_every_pack_or_the_extras_offers_their_tools() {
    let every = lua_over(offer_set());
    every.exec(
        "tools.adhoc({ name = 'grab', description = 'Grab a value' }, function() end)\n\
         tools.offer(tools.packs)",
    );
    assert_eq!(
        scoped(&every),
        [
            "grab",
            "gh_issues_list",
            "gh_search",
            "user-input_ask",
            "web_fetch",
            "web_search"
        ],
        "every Pack, the adhoc Pack included, in name order"
    );
    let extras = lua_over(offer_set());
    extras.exec(
        "tools.adhoc({ name = 'grab', description = 'Grab a value' }, function() end)\n\
         tools.offer(tools.packs.extras())",
    );
    assert_eq!(
        scoped(&extras),
        ["gh_issues_list", "gh_search"],
        "only the undeclared Packs, never the adhoc Pack"
    );
}

#[test]
fn a_tool_object_reads_its_id_and_catalog_description_and_refuses_assignment() {
    let vm = lua_over(offer_set());
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
fn tools_get_reads_one_object_by_id_and_nil_for_anything_else() {
    let vm = lua_over(offer_set());
    let result: (String, bool, bool, bool, bool, bool, String) = vm.eval(
        "local helper = tools.adhoc({ name = 'helper', description = 'A helper' }, function() end)\n\
         local ok, err = pcall(tools.get, 42)\n\
         assert(not ok, 'a non-string is refused')\n\
         return tools.get('gh/search').id, tools.get('adhoc/helper') == helper, \
           tools.get('gh/missing') == nil, tools.get('helper') == nil, \
           tools.get('Not An Id') == nil, tools.get('gh_search') == nil, tostring(err)",
    );
    assert_eq!(result.0, "gh/search");
    assert!(result.1, "an adhoc tool reads by its id");
    assert!(
        result.2 && result.3 && result.4 && result.5,
        "a miss, an adhoc tool's wire name, a malformed id, and a catalog wire name \
         all read nil: {result:?}"
    );
    assert!(
        result.6.contains("tools.get takes a tool id, got integer"),
        "the refusal names the type: {}",
        result.6
    );
}

#[test]
fn the_tools_table_holds_exactly_its_api() {
    let vm = lua_over(offer_set());
    let keys: String = vm.eval(
        "local keys = {}\n\
         for key, value in pairs(tools) do keys[#keys + 1] = key .. ':' .. type(value) end\n\
         table.sort(keys)\n\
         return table.concat(keys, ',')",
    );
    assert_eq!(
        keys, "adhoc:function,get:function,offer:function,offer_dynamic:function,packs:table",
        "the tools table holds exactly the namespace's members before the shim installs call"
    );
}
