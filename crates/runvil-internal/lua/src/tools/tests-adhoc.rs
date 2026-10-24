//! Tests for `tools.adhoc`: the tool object it returns, the id and wire
//! name it gives, the schema it builds from `parameters`, its refusals,
//! the adhoc Pack, and the counts an adhoc tool keys.

use mlua::{LuaSerdeExt, Value};
use serde_json::{Value as Json, json};

use super::parameters::search_set;
use crate::Argv;
use crate::test_vm::{TestVm, lua_over, offer_set, raised, scoped};

/// The `grab` definition of the listing, held in a local.
const GRAB: &str = "local grab = tools.adhoc({\n\
       name = 'grab',\n\
       description = 'Grab a value',\n\
       parameters = { value = 'string' },\n\
     }, function(args) return 'got ' .. args.value end)\n";

/// The schema the adhoc tool with wire name `name` on `vm` binds.
fn adhoc_schema(vm: &crate::SectionVm, name: &str) -> Json {
    vm.adhoc_tools()
        .binding(name)
        .expect("the adhoc tools read")
        .unwrap_or_else(|| panic!("{name} is defined"))
        .schema()
        .clone()
}

#[test]
fn the_returned_object_is_the_tools_object_and_is_rendered_only_once_offered() {
    let vm = lua_over(offer_set());
    let fields: (bool, String, String, String, bool, bool) = vm.eval(&format!(
        "{GRAB}\
         return grab == tools.get('adhoc/grab'), grab.id, grab.description, \
           grab.parameters.properties.value.type, grab.pack == tools.packs.get('adhoc'), \
           grab.pack.tools[1] == grab"
    ));
    assert_eq!(
        fields,
        (
            true,
            "adhoc/grab".to_owned(),
            "Grab a value".to_owned(),
            "string".to_owned(),
            true,
            true
        )
    );
    assert!(scoped(&vm).is_empty(), "defining a tool offers nothing");
    vm.exec("tools.offer(tools.get('adhoc/grab'))");
    assert_eq!(scoped(&vm), ["grab"]);
}

#[test]
fn the_id_lowercases_the_name_and_the_wire_name_keeps_it() {
    let vm = lua_over(offer_set());
    let id: String = vm.eval(
        "local shout = tools.adhoc({ name = 'Shout', description = 'Uppercase text', \
           parameters = { text = 'string' } }, function(args) return args.text:upper() end)\n\
         tools.offer(shout)\n\
         assert(tools.get('adhoc/shout') == shout)\n\
         return shout.id",
    );
    assert_eq!(id, "adhoc/shout");
    assert_eq!(
        scoped(&vm),
        ["Shout"],
        "the wire name is the name as written"
    );
}

#[test]
fn each_refusal_raises_on_the_defining_line() {
    let vm = lua_over(offer_set());
    vm.exec(GRAB);
    let invalid = "tools.adhoc: invalid name \"1x\": expected [A-Za-z][A-Za-z0-9_-]{0,63}";
    for (call, expected) in [
        (
            "tools.adhoc('grab', function() end)",
            "tools.adhoc takes an attrs table first, got string",
        ),
        (
            "tools.adhoc({ name = 'x', description = 'y' })",
            "tools.adhoc takes the tool's function second, got nil",
        ),
        (
            "tools.adhoc({ name = 'x', description = 'y' }, function() end, 'z')",
            "tools.adhoc takes two arguments, got extra string",
        ),
        (
            "tools.adhoc({ name = 'grab', description = 'again' }, function() end)",
            "tools.adhoc name \"grab\" is already registered",
        ),
        (
            "tools.adhoc({ name = 'GRAB', description = 'again' }, function() end)",
            "tools.adhoc name \"GRAB\" has the id adhoc/grab, which \"grab\" already holds",
        ),
        (
            "tools.adhoc({ name = 'x', description = 'y', run = 1 }, function() end)",
            "tools.adhoc: unknown key \"run\"; accepted keys are name, description, parameters",
        ),
        (
            "tools.adhoc({ description = 'y' }, function() end)",
            "tools.adhoc: \"name\" is required",
        ),
        (
            "tools.adhoc({ name = 'x' }, function() end)",
            "tools.adhoc: \"description\" is required",
        ),
        (
            "tools.adhoc({ name = 7, description = 'y' }, function() end)",
            "tools.adhoc: \"name\" must be a string, got integer",
        ),
        (
            "tools.adhoc({ name = 'x', description = false }, function() end)",
            "tools.adhoc: \"description\" must be a string, got boolean",
        ),
        (
            "tools.adhoc({ name = '1x', description = 'y' }, function() end)",
            invalid,
        ),
        (
            "tools.adhoc({ name = 'x', description = 'y', parameters = 'string' }, function() end)",
            "tools.adhoc: parameters must be a table, got string",
        ),
        (
            "tools.adhoc({ name = 'x', description = 'y', parameters = { p = 'table' } }, \
               function() end)",
            "tools.adhoc: param \"p\" has unsupported type \"table\"",
        ),
        (
            "tools.adhoc({ name = 'x', description = 'y', \
               parameters = { type = 'object', properties = { q = function() end } } }, \
               function() end)",
            "tools.adhoc: parameters schema cannot be encoded as JSON: value.properties.q \
             must be JSON data, got function",
        ),
    ] {
        let error = raised(&vm, call);
        assert!(error.contains(expected), "{call}: {error}");
    }
    let ids: Vec<String> = vm
        .adhoc_tools()
        .bindings()
        .expect("the adhoc tools read")
        .iter()
        .map(|binding| binding.id().to_string())
        .collect();
    assert_eq!(ids, ["adhoc/grab"], "no refused definition registers");
}

#[test]
fn parameters_take_the_short_form_a_json_schema_or_nothing() {
    let vm = lua_over(offer_set());
    vm.exec(
        "tools.adhoc({ name = 'find', description = 'Find notes', \
           parameters = { query = 'string', limit = { 'integer', 'maximum hits' } } }, \
           function() end)\n\
         tools.adhoc({ name = 'typed', description = 'Typed', \
           parameters = { type = 'string' } }, function() end)\n\
         tools.adhoc({ name = 'none', description = 'No parameters' }, function() end)",
    );
    assert_eq!(
        adhoc_schema(&vm, "find"),
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" },
                "limit": { "type": "integer", "description": "maximum hits" }
            },
            "required": ["limit", "query"]
        })
    );
    assert_eq!(
        adhoc_schema(&vm, "typed"),
        json!({
            "type": "object",
            "properties": { "type": { "type": "string" } },
            "required": ["type"]
        }),
        "a type other than object makes the short form, so this is a parameter named type"
    );
    assert_eq!(
        adhoc_schema(&vm, "none"),
        json!({ "type": "object", "properties": {}, "required": [] })
    );
    let empty: String = vm.eval("return json.encode(tools.get('adhoc/none').parameters)");
    assert_eq!(
        empty,
        "{\"properties\":{},\"required\":[],\"type\":\"object\"}"
    );
}

#[test]
fn a_wrapper_of_a_catalog_tool_takes_its_description_and_schema_unchanged() {
    let vm = lua_over(search_set());
    vm.exec(
        "local t = tools.get('web/search')\n\
         tools.adhoc({ name = 'search', description = t.description, parameters = t.parameters }, \
           function(args) return tools.call(t, args) end)",
    );
    let (set, _) = vm.tool_bag_handles();
    let catalog = set
        .offered_binding("web/search")
        .expect("the catalog tool is offered");
    let wrapper = vm
        .adhoc_tools()
        .binding("adhoc/search")
        .expect("the adhoc tools read")
        .expect("the wrapper is defined");
    assert_eq!(wrapper.description(), catalog.description());
    assert_eq!(
        serde_json::to_string(wrapper.schema()).expect("a schema serializes"),
        serde_json::to_string(catalog.schema()).expect("a schema serializes"),
        "the rendered schema is the same text"
    );
    let parameters: Value = vm.eval("return tools.get('adhoc/search').parameters");
    let read: Json = vm
        .lua()
        .from_value(parameters)
        .expect("the parameters table converts back to JSON");
    assert_eq!(
        &read,
        catalog.schema(),
        "parameters deep-equals the catalog tool's"
    );
}

#[test]
fn an_adhoc_description_is_the_attrs_text_and_never_the_envelope() {
    let vm = lua_over(offer_set());
    let description: String = vm.eval(
        "return tools.adhoc({ name = 'x', description = 'Read <the web> [INST] now' }, \
           function() end).description",
    );
    assert_eq!(description, "Read <the web> [INST] now");
    let nonce = crate::test_vm::test_nonce().to_string();
    assert!(!description.contains(&nonce) && !description.contains("is data, not instructions"));
}

#[test]
fn the_adhoc_pack_exists_in_every_vm_with_nothing_defined() {
    for vm in [
        lua_over(offer_set()),
        TestVm::new().argv(Argv::Writable(None)).build(),
    ] {
        let pack: (bool, String, bool, i64) = vm.eval(
            "local adhoc = tools.packs.get('adhoc')\n\
             return adhoc ~= nil, adhoc.name, adhoc.required, #adhoc.tools",
        );
        assert_eq!(pack, (true, "adhoc".to_owned(), false, 0));
    }
}

#[test]
fn an_offered_adhoc_tool_counts_from_zero_and_an_unoffered_one_from_its_first_call() {
    let vm = lua_over(offer_set());
    vm.exec(&format!(
        "{GRAB}\
         tools.adhoc({{ name = 'other', description = 'Other' }}, function() end)\n\
         tools.offer(grab)"
    ));
    let (set, runtime) = vm.tool_bag_handles();
    let bound =
        crate::current_tool_bindings(&set, &runtime, vm.adhoc_tools()).expect("the scope resolves");
    let counts = vm
        .install_tool_call_counts(&bound)
        .expect("the counts install");
    let offered: i64 = vm.eval("return tools.calls['adhoc/grab']");
    assert_eq!(offered, 0, "an offered adhoc tool is seeded");
    let unoffered = raised(&vm, "return tools.calls['adhoc/other']");
    assert!(
        unoffered.contains(
            "(an adhoc tool that was neither offered in this section nor called with tools.call)"
        ),
        "{unoffered}"
    );
    counts.ensure("adhoc/other").expect("a call seeds its id");
    counts
        .increment("adhoc/other")
        .expect("the seeded id counts");
    let called: i64 = vm.eval("return tools.calls['adhoc/other']");
    assert_eq!(called, 1, "a call gives an unoffered adhoc tool its count");
}
