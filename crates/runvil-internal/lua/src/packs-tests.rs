//! Tests for the `packs` table: the `required` and `extras` lists,
//! `packs.get`, the shared Pack objects and their `tools`, a tool
//! object's `pack` getter, and the roster's usable and unavailable
//! Packs.

use std::collections::BTreeMap;

use runvil_types::packs::PackId;

use crate::handles::ToolSet;
use crate::test_vm::{lua_over, scoped, tool_at};
use crate::{SectionVm, ToolBinding};

/// A set declaring `web` and then `mail`, which offers no tool, beside the
/// undeclared Packs `alpha` and `gh`, every tool in id order under its
/// wire name.
fn pack_set() -> ToolSet {
    let bind = |id: &str| ToolBinding::from_info(&id.replace(['/', '.'], "_"), &tool_at(id));
    ToolSet::new(
        vec![
            PackId::parse("web").expect("`web` is a valid Pack id"),
            PackId::parse("mail").expect("`mail` is a valid Pack id"),
        ],
        BTreeMap::new(),
        vec![
            bind("alpha/zed"),
            bind("gh/issues.list"),
            bind("gh/search"),
            bind("web/fetch"),
            bind("web/search"),
        ],
    )
}

/// [`pack_set`] with a roster: `web` and `gh`, which also offer tools,
/// and `subagents`, which offers none, can serve the run, and `mcp`
/// can't.
fn roster_set() -> ToolSet {
    let pack = |name: &str| PackId::parse(name).expect("each roster name is a valid Pack id");
    let base = pack_set();
    ToolSet::new(
        base.declared().to_vec(),
        [
            (pack("gh"), None),
            (pack("mcp"), Some("the mcp server is down".to_owned())),
            (pack("subagents"), None),
            (pack("web"), None),
        ]
        .into_iter()
        .collect(),
        base.offered().to_vec(),
    )
}

/// The Packs `packs.offer` recorded on `vm`, by name.
#[track_caller]
fn offered_packs(vm: &SectionVm) -> Vec<String> {
    vm.tool_runtime()
        .lock()
        .expect("the runtime mutex is not poisoned")
        .packs
        .iter()
        .map(ToString::to_string)
        .collect()
}

/// A Lua helper joining the `name` of each Pack object in a list.
const NAMES: &str = "local function names(list)\n\
       local out = {}\n\
       for i, pack in ipairs(list) do out[i] = pack.name end\n\
       return table.concat(out, ',')\n\
     end\n";

#[test]
fn required_lists_every_declared_pack_in_name_order_and_extras_the_rest() {
    let vm = lua_over(pack_set());
    let result: (String, String, i64, String, bool, bool, String) = vm.eval(&format!(
        "{NAMES}\
         local ok, err = pcall(packs.get, 42)\n\
         assert(not ok, 'a non-string is refused')\n\
         return names(packs.required()), names(packs.extras()), \
           #packs.get('mail').tools, packs.get('gh').name, \
           packs.get('missing') == nil, packs.get('web/fetch') == nil, \
           tostring(err)"
    ));
    assert_eq!(
        result.0, "mail,web",
        "every declared Pack is required, in name order"
    );
    assert_eq!(
        result.1, "alpha,gh",
        "every other Pack with offered tools is an extra, in name order"
    );
    assert_eq!(result.2, 0, "a declared Pack with no tool has an object");
    assert_eq!(result.3, "gh", "packs.get reads an extra by name");
    assert!(
        result.4 && result.5,
        "an unknown name and a tool id read nil: {result:?}"
    );
    assert!(
        result.6.contains("packs.get takes a Pack id, got integer"),
        "the refusal names the type: {}",
        result.6
    );
}

#[test]
fn packs_get_returns_the_one_object_required_and_extras_hand_out() {
    let vm = lua_over(pack_set());
    let result: (bool, bool, bool, bool, bool) = vm.eval(
        "local required = packs.required()\n\
         return packs.get('web') == packs.get('web'), \
           required[2] == packs.get('web'), \
           packs.extras()[2] == packs.get('gh'), \
           packs.required() ~= required, \
           packs.extras() ~= packs.extras()",
    );
    assert_eq!(
        result,
        (true, true, true, true, true),
        "one object per Pack, handed out in a fresh array per list call"
    );
}

#[test]
fn a_packs_tools_are_the_shared_tool_objects_in_a_fresh_array_that_offer_takes() {
    let vm = lua_over(pack_set());
    let result: (i64, bool, bool, bool) = vm.eval(
        "local web = packs.get('web')\n\
         local tools_list = web.tools\n\
         tools.offer(web.tools)\n\
         return #tools_list, tools_list[1] == tools.get('web/fetch'), \
           tools_list[2] == tools.get('web/search'), web.tools ~= tools_list",
    );
    assert_eq!(
        result,
        (2, true, true, true),
        "a Pack's tools are its shared tool objects, in id order, in a new array per read"
    );
    assert_eq!(
        scoped(&vm),
        ["web_fetch", "web_search"],
        "tools.offer takes a Pack's tools array"
    );
}

#[test]
fn assigning_a_field_on_a_pack_object_is_refused() {
    let vm = lua_over(pack_set());
    let (name, extra, read): (String, String, String) = vm.eval(
        "local web = packs.get('web')\n\
         local _, name = pcall(function() web.name = 'x' end)\n\
         local _, extra = pcall(function() web.extra = 1 end)\n\
         return tostring(name), tostring(extra), web.name",
    );
    assert!(
        name.contains("Pack objects are frozen: cannot assign field \"name\""),
        "the refusal names the field: {name}"
    );
    assert!(
        extra.contains("Pack objects are frozen: cannot assign field \"extra\""),
        "a new field is refused too: {extra}"
    );
    assert_eq!(read, "web", "the object keeps its name");
}

#[test]
fn every_tool_objects_pack_is_its_packs_object() {
    let vm = lua_over(pack_set());
    let mismatched: String = vm.eval(
        "local bad = {}\n\
         for _, list in ipairs({ tools.required(), tools.extras() }) do\n\
           for _, tool in ipairs(list) do\n\
             local name = tool.id:match('^[^/]+')\n\
             if tool.pack ~= packs.get(name) or tool.pack.name ~= name then\n\
               bad[#bad + 1] = tool.id\n\
             end\n\
           end\n\
         end\n\
         assert(#tools.required() + #tools.extras() == 5, 'every tool is walked')\n\
         return table.concat(bad, ',')",
    );
    assert_eq!(
        mismatched, "",
        "each tool object's pack is == to its Pack's object"
    );
}

#[test]
fn packs_is_the_engine_global_and_the_old_plugins_global_is_nil() {
    let vm = lua_over(pack_set());
    let (installed, gone, error): (bool, bool, String) = vm.eval(
        "local ok, err = pcall(function() return plugins['get']('web') end)\n\
         assert(not ok, 'the old global is not installed')\n\
         return type(packs) == 'table', plugins == nil, tostring(err)",
    );
    assert!(installed, "the packs table is installed");
    assert!(gone, "no plugins global is installed");
    assert!(
        error.contains("attempt to index a nil value"),
        "a script still calling plugins.* fails on the nil global: {error}"
    );
}

#[test]
fn a_tool_objects_pack_is_its_packs_object_and_offering_the_pack_scopes_its_tools() {
    let vm = lua_over(pack_set());
    let same: bool = vm.eval(
        "packs.offer('web')\n\
         return tools.get('web/search').pack == packs.get('web')",
    );
    assert!(same, "a tool object's pack is its Pack's object");
    assert_eq!(
        scoped(&vm),
        ["web_fetch", "web_search"],
        "offering a Pack scopes each of its tools"
    );
}

#[test]
fn offer_records_each_pack_once_from_a_name_an_object_or_a_list() {
    let vm = lua_over(pack_set());
    vm.exec(
        "packs.offer('gh')\n\
         packs.offer(packs.get('alpha'))\n\
         packs.offer({ 'gh', packs.get('web') })\n\
         packs.offer(packs.required())\n\
         packs.offer({})\n\
         assert(select('#', packs.offer('gh')) == 0, 'the verb returns nothing')",
    );
    assert_eq!(
        offered_packs(&vm),
        ["gh", "alpha", "web", "mail"],
        "each Pack is recorded once, in first-offer order"
    );
    assert!(
        vm.tool_runtime()
            .lock()
            .expect("the runtime mutex is not poisoned")
            .added
            .is_empty(),
        "a Pack offer never records a wire name"
    );
}

#[test]
fn offer_refuses_a_wrong_or_extra_argument_and_an_absent_pack_and_records_nothing() {
    let vm = lua_over(pack_set());
    let shape = "packs.offer takes a Pack id, a Pack object, or a list of them, got";
    let absent = "packs.offer: no Pack named \"mcp\" in this run; declare it under \
                  uses: to require it, or offer it only when packs.get(\"mcp\") returns it";
    for (call, expected) in [
        ("packs.offer(nil)", format!("{shape} nil")),
        ("packs.offer()", format!("{shape} nil")),
        ("packs.offer(42)", format!("{shape} integer")),
        (
            "packs.offer(tools.get('web/fetch'))",
            format!("{shape} userdata"),
        ),
        ("packs.offer({ 'web', 42 })", format!("{shape} integer")),
        (
            "packs.offer('web/fetch')",
            format!(
                "{shape} \"web/fetch\": {}",
                PackId::parse("web/fetch").expect_err("a tool id is not a Pack id")
            ),
        ),
        (
            "packs.offer('web', 'gh')",
            "packs.offer takes one Pack or a list of them, got extra string".to_owned(),
        ),
        ("packs.offer('mcp')", absent.to_owned()),
        ("packs.offer({ 'web', 'mcp' })", absent.to_owned()),
    ] {
        let error: String = vm
            .lua()
            .load(format!(
                "local ok, err = pcall(function() {call} end)\n\
                 assert(not ok, 'the call is refused')\n\
                 return tostring(err)"
            ))
            .eval()
            .unwrap_or_else(|error| panic!("{call}: {error}"));
        assert!(error.contains(&expected), "{call}: {error}");
    }
    assert!(
        offered_packs(&vm).is_empty(),
        "a refused call records no Pack"
    );
}

#[test]
fn the_scope_lists_section_tools_then_each_offered_packs_tools_each_name_once() {
    let vm = lua_over(pack_set());
    vm.exec(
        "tools.offer('web/search')\n\
         packs.offer({ 'gh', 'web', 'alpha' })",
    );
    let before = scoped(&vm);
    assert_eq!(
        before,
        [
            "web_search",
            "gh_issues_list",
            "gh_search",
            "web_fetch",
            "alpha_zed"
        ],
        "section tools first, then Packs in offer order with tools in id order, \
         each name at its first place"
    );
    vm.exec("packs.offer('mail')");
    assert_eq!(scoped(&vm), before, "a Pack with no tools adds nothing");
}

#[test]
fn a_usable_roster_pack_with_no_tools_has_an_object_and_is_an_extra_in_name_order() {
    let vm = lua_over(roster_set());
    let result: (String, String, String, i64) = vm.eval(&format!(
        "{NAMES}\
         packs.offer('subagents')\n\
         local stub = packs.get('subagents')\n\
         return names(packs.required()), names(packs.extras()), stub.name, #stub.tools"
    ));
    assert_eq!(
        result,
        (
            "mail,web".to_owned(),
            "alpha,gh,subagents".to_owned(),
            "subagents".to_owned(),
            0
        ),
        "a usable Pack with no tools is an extra once, beside the ones with tools"
    );
    assert_eq!(offered_packs(&vm), ["subagents"]);
    assert!(
        scoped(&vm).is_empty(),
        "offering a Pack with no tools adds nothing to a round"
    );
}

#[test]
fn an_unavailable_roster_pack_reads_nil_stays_out_of_the_lists_and_offer_raises_its_reason() {
    let vm = lua_over(roster_set());
    let listed: (bool, String, String) = vm.eval(&format!(
        "{NAMES}\
         return packs.get('mcp') == nil, names(packs.required()), \
           names(packs.extras())"
    ));
    assert_eq!(
        listed,
        (true, "mail,web".to_owned(), "alpha,gh,subagents".to_owned()),
        "an unavailable Pack has no object and is in neither list"
    );
    let unavailable = "packs.offer: the mcp Pack is unavailable in this run: \
                       the mcp server is down";
    for call in ["packs.offer('mcp')", "packs.offer({ 'web', 'mcp' })"] {
        let error: String = vm
            .lua()
            .load(format!(
                "local ok, err = pcall(function() {call} end)\n\
                 assert(not ok, 'the call is refused')\n\
                 return tostring(err)"
            ))
            .eval()
            .unwrap_or_else(|error| panic!("{call}: {error}"));
        assert!(error.contains(unavailable), "{call}: {error}");
    }
    assert!(
        offered_packs(&vm).is_empty(),
        "a refused call records no Pack"
    );
}
