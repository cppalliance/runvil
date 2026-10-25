//! Tests for the Pack objects and `tools.packs`: the walk in name order,
//! `get` and `extras`, the frozen proxy, `required`, shared objects, names
//! that collide with members, and the roster's usable and unavailable
//! Packs.

use std::collections::BTreeMap;

use runvil_types::packs::PackId;
use runvil_types::tools::ToolCatalog;

use crate::handles::ToolSet;
use crate::test_vm::{lua_over, offer_set, offers, raised, scoped, tool_at};

/// A Lua helper joining the `name` of each Pack object in a list.
const NAMES: &str = "local function names(list)\n\
       local out = {}\n\
       for i, pack in ipairs(list) do out[i] = pack.name end\n\
       return table.concat(out, ',')\n\
     end\n";

/// A set declaring `mail`, which has no tool, beside the undeclared
/// Packs `get`, `extras`, and `tools`, whose names a member of
/// `tools.packs` or a Pack object also has.
fn colliding_set() -> ToolSet {
    ToolSet::new(
        vec![PackId::parse("mail").expect("`mail` is a valid Pack id")],
        BTreeMap::new(),
        ToolCatalog::new(["extras/list", "get/one", "tools/tools"].map(tool_at))
            .expect("the fixture ids differ"),
        BTreeMap::new(),
    )
}

#[test]
fn tools_packs_walks_every_pack_object_in_name_order_the_adhoc_pack_included() {
    let vm = lua_over(offer_set());
    let walked: (String, String, String, i64, bool) = vm.eval(&format!(
        "{NAMES}\
         local indexed = {{}}\n\
         for i = 1, #tools.packs do indexed[i] = tools.packs[i] end\n\
         local paired, keys = {{}}, {{}}\n\
         for key, pack in pairs(tools.packs) do\n\
           keys[#keys + 1] = type(key)\n\
           paired[#paired + 1] = pack\n\
         end\n\
         return names(tools.packs), names(indexed), names(paired), #tools.packs, \
           table.concat(keys, ',') == 'number,number,number,number,number'"
    ));
    let order = "adhoc,gh,subagents,user-input,web";
    assert_eq!(
        walked,
        (
            order.to_owned(),
            order.to_owned(),
            order.to_owned(),
            5,
            true
        ),
        "ipairs, integer keys, and pairs walk the Pack objects in name order, and \
         pairs yields only Packs"
    );
}

#[test]
fn get_returns_a_shared_object_or_nil_and_refuses_anything_but_one_pack_id() {
    let vm = lua_over(offer_set());
    let read: (bool, bool, bool, bool, bool) = vm.eval(
        "local web = tools.packs.get('web')\n\
         return web == tools.packs.get('web'), web == tools.packs[5], \
           tools.get('web/search').pack == web, tools.packs.get('missing') == nil, \
           tools.packs.get('web/search') == nil",
    );
    assert_eq!(
        read,
        (true, true, true, true, true),
        "one object per Pack across get, integer keys, and tool.pack; nil for an \
         unknown name and a tool id"
    );
    for (call, kind) in [
        ("tools.packs.get(42)", "integer"),
        ("tools.packs.get({ 'web', 'subagents' })", "table"),
        ("tools.packs.get()", "nil"),
    ] {
        let error = raised(&vm, call);
        assert!(
            error.contains(&format!("tools.packs.get takes one Pack id, got {kind}")),
            "{call}: {error}"
        );
    }
}

#[test]
fn tools_packs_is_a_frozen_proxy_whose_other_keys_read_nil() {
    let vm = lua_over(offer_set());
    let reads: (bool, bool, bool, String) = vm.eval(
        "return tools.packs['web'] == nil, tools.packs['name'] == nil, tools.packs[99] == nil, \
           tostring(getmetatable(tools.packs))",
    );
    assert_eq!(
        reads,
        (true, true, true, "tools.packs is read-only".to_owned())
    );
    for write in [
        "tools.packs.get = nil",
        "tools.packs[1] = 'web'",
        "tools.packs['web'] = 1",
    ] {
        let error = raised(&vm, write);
        assert!(
            error.contains("tools.packs is read-only"),
            "{write}: {error}"
        );
    }
    let error = raised(&vm, "setmetatable(tools.packs, nil)");
    assert!(
        error.contains("cannot change a protected metatable"),
        "the metatable is protected: {error}"
    );
    let still: String = vm.eval("return tools.packs.get('web').name");
    assert_eq!(still, "web");
}

#[test]
fn extras_are_the_usable_undeclared_packs_in_a_fresh_array() {
    let vm = lua_over(offer_set());
    let result: (String, bool, bool) = vm.eval(&format!(
        "{NAMES}\
         local extras = tools.packs.extras()\n\
         return names(extras), extras ~= tools.packs.extras(), \
           extras[1] == tools.packs.get('gh')"
    ));
    assert_eq!(
        result,
        ("gh,subagents".to_owned(), true, true),
        "never a declared Pack, the adhoc Pack, or a Pack that cannot serve"
    );
}

#[test]
fn required_is_true_only_for_a_declared_pack() {
    let vm = lua_over(colliding_set());
    let required: (bool, bool, bool, bool, bool) = vm.eval(
        "return tools.packs.get('mail').required, tools.packs.get('tools').required, \
           tools.packs.get('adhoc').required, #tools.packs.get('tools').tools > 0, \
           #tools.packs.get('mail').tools > 0",
    );
    assert_eq!(
        required,
        (true, false, false, true, false),
        "a declared Pack is required even with no tools, and #pack.tools tells \
         whether it has tools"
    );
}

#[test]
fn packs_whose_names_collide_with_members_are_reached_through_get_and_offered_by_id() {
    let vm = lua_over(colliding_set());
    let reads: (String, String, String, String, bool, bool) = vm.eval(
        "return tools.packs.get('get').name, tools.packs.get('extras').name, \
           tools.packs.get('tools').name, tools.packs.get('tools').tools[1].id, \
           type(tools.packs.get) == 'function', type(tools.packs.extras) == 'function'",
    );
    assert_eq!(
        reads,
        (
            "get".to_owned(),
            "extras".to_owned(),
            "tools".to_owned(),
            "tools/tools".to_owned(),
            true,
            true
        )
    );
    vm.exec("tools.offer({ 'get', 'extras', 'tools' })");
    assert_eq!(offers(&vm), ["static:get", "static:extras", "static:tools"]);
    assert_eq!(scoped(&vm), ["get_one", "extras_list", "tools_tools"]);
}

#[test]
fn pack_and_tool_objects_hold_no_field_beyond_their_own() {
    let vm = lua_over(colliding_set());
    let extra: Option<String> = vm.eval(
        "local probes = {\n\
           { tools.packs.get('tools'), { 'get', 'extras', 'id', 'vendor', 'pack' } },\n\
           { tools.get('tools/tools'), { 'tools', 'name', 'required', 'vendor', 'alias' } },\n\
         }\n\
         for _, probe in ipairs(probes) do\n\
           for _, field in ipairs(probe[2]) do\n\
             local ok, value = pcall(function() return probe[1][field] end)\n\
             if ok and value ~= nil then return field end\n\
           end\n\
         end\n\
         return nil",
    );
    assert_eq!(extra, None);
}

#[test]
fn assigning_a_field_on_a_pack_object_is_refused() {
    let vm = lua_over(offer_set());
    let (name, extra, read): (String, String, String) = vm.eval(
        "local web = tools.packs.get('web')\n\
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
fn a_packs_tools_are_the_shared_tool_objects_in_a_fresh_array_that_offer_takes() {
    let vm = lua_over(offer_set());
    let result: (i64, bool, bool, bool) = vm.eval(
        "local web = tools.packs.get('web')\n\
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
fn every_tool_objects_pack_is_its_packs_object() {
    let vm = lua_over(offer_set());
    let mismatched: String = vm.eval(
        "local bad, walked = {}, 0\n\
         for _, pack in ipairs(tools.packs) do\n\
           for _, tool in ipairs(pack.tools) do\n\
             walked = walked + 1\n\
             if tool.pack ~= pack or tool.pack.name ~= tool.id:match('^[^/]+') then\n\
               bad[#bad + 1] = tool.id\n\
             end\n\
           end\n\
         end\n\
         assert(walked == 5, 'every tool is walked')\n\
         return table.concat(bad, ',')",
    );
    assert_eq!(
        mismatched, "",
        "each tool object's pack is == to its Pack's object"
    );
}

#[test]
fn the_render_lists_each_offer_in_call_order_each_tool_once() {
    let vm = lua_over(offer_set());
    vm.exec(
        "tools.offer('web/search')\n\
         tools.offer({ 'gh', 'web', 'user-input' })",
    );
    let before = scoped(&vm);
    assert_eq!(
        before,
        [
            "web_search",
            "gh_issues_list",
            "gh_search",
            "web_fetch",
            "user-input_ask"
        ],
        "each offer in call order, a Pack's tools in id order, each tool at its first place"
    );
    vm.exec("tools.offer('subagents')");
    assert_eq!(scoped(&vm), before, "a Pack with no tools adds nothing");
}

#[test]
fn a_usable_roster_pack_with_no_tools_has_an_object_and_is_an_extra() {
    let vm = lua_over(offer_set());
    let result: (String, i64, bool) = vm.eval(
        "tools.offer('subagents')\n\
         local stub = tools.packs.get('subagents')\n\
         return stub.name, #stub.tools, stub.required",
    );
    assert_eq!(result, ("subagents".to_owned(), 0, false));
    assert_eq!(offers(&vm), ["static:subagents"]);
    assert!(
        scoped(&vm).is_empty(),
        "offering a Pack with no tools adds nothing to a round"
    );
}

#[test]
fn an_unavailable_roster_pack_has_no_object_and_offering_it_raises_its_reason() {
    let vm = lua_over(offer_set());
    let missing: bool = vm.eval("return tools.packs.get('mcp') == nil");
    assert!(missing, "an unavailable Pack has no object");
    let unavailable = "tools.offer: the mcp Pack is unavailable in this run: \
                       the mcp server is down";
    for call in ["tools.offer('mcp')", "tools.offer({ 'web', 'mcp' })"] {
        let error = raised(&vm, call);
        assert!(error.contains(unavailable), "{call}: {error}");
    }
    assert!(offers(&vm).is_empty(), "a refused call records no Pack");
}
