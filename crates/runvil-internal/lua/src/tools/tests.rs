//! Tests for the `tools` namespace installers, tool-name decoding, and the
//! short-form `parameters` schema.

use std::collections::BTreeMap;

use mlua::{Lua, Value};
use serde_json::json;

use super::decode::{adhoc_params_schema, model_tool_name, tool_name};
use super::userdata::LuaToolHandle;
use crate::test_vm::{TestVm, echo_tool, lua_over, tool_at};
use crate::{ToolBinding, ToolSet};

#[path = "tests-adhoc.rs"]
mod adhoc;
#[path = "tests-offer-limits.rs"]
mod offer_limits;
#[path = "tests-offering.rs"]
mod offering;
#[path = "tests-parameters.rs"]
mod parameters;
#[path = "tests-render.rs"]
mod render;

/// A userdata that is not a Tool object, for the foreign-userdata rejection.
struct Foreign;

impl mlua::UserData for Foreign {}

fn echo_handle() -> LuaToolHandle {
    LuaToolHandle::from_binding(&ToolBinding::from_info("tools_echo", &echo_tool()))
}

fn string(lua: &Lua, text: &str) -> Value {
    Value::String(
        lua.create_string(text)
            .expect("a fresh VM creates a short string"),
    )
}

#[test]
fn tool_name_accepts_an_id_and_refuses_a_string_without_a_slash() {
    let lua = Lua::new();
    assert_eq!(
        tool_name(&string(&lua, "tools/echo")).expect("an id decodes"),
        "tools/echo"
    );
    let error = tool_name(&string(&lua, "grab")).expect_err("a bare name is not an id");
    assert!(
        error
            .to_string()
            .contains("tools.call: \"grab\" is not a tool id"),
        "the refusal names the string: {error}"
    );
}

#[test]
fn a_model_issued_name_is_read_verbatim() {
    let lua = Lua::new();
    assert_eq!(
        model_tool_name(&string(&lua, "web_search")).expect("a wire name decodes"),
        "web_search",
        "a model names a tool by the wire name it was sent"
    );
}

#[test]
fn a_model_issued_call_refuses_a_tool_object() {
    let lua = Lua::new();
    let userdata = lua
        .create_userdata(echo_handle())
        .expect("a fresh VM creates a tool object");
    let error = model_tool_name(&Value::UserData(userdata))
        .expect_err("a model-issued call names a wire name, not an object");
    assert!(
        error
            .to_string()
            .contains("a model-issued tool call names a tool by its wire name, got userdata"),
        "the refusal names the wire-name form: {error}"
    );
}

#[test]
fn tool_name_reads_the_id_off_a_tool_object() {
    let lua = Lua::new();
    let userdata = lua
        .create_userdata(echo_handle())
        .expect("a fresh VM creates a tool object");
    assert_eq!(
        tool_name(&Value::UserData(userdata)).expect("a tool object decodes"),
        "tools/echo",
        "a tool object stands for its id, not its wire name"
    );
}

#[test]
fn tool_name_rejects_tables_other_types_and_other_userdata() {
    let lua = Lua::new();
    let number = tool_name(&Value::Integer(42)).expect_err("a number is not a tool");
    assert!(
        number
            .to_string()
            .contains("tools.call takes a tool id or a tool object, got integer"),
        "the rejection names the accepted forms: {number}"
    );
    let record = lua
        .load("{ id = 'tools/echo', name = 'tools_echo' }")
        .eval::<mlua::Table>()
        .expect("the record evaluates");
    let table = tool_name(&Value::Table(record)).expect_err("a record is not a tool");
    assert!(
        table.to_string().contains("got table"),
        "a plain record is not a tool: {table}"
    );
    // A userdata that is not a tool object takes the same rejection; the
    // borrow failure must not leak mlua's type-mismatch wording.
    let foreign = lua
        .create_userdata(Foreign)
        .expect("a fresh VM creates a foreign userdata");
    let other = tool_name(&Value::UserData(foreign)).expect_err("not a tool object");
    assert!(
        other
            .to_string()
            .contains("tools.call takes a tool id or a tool object, got userdata"),
        "a foreign userdata gets the same rejection: {other}"
    );
}

#[test]
fn adhoc_params_schema_builds_object_schema_with_required_fields() {
    let lua = Lua::new();
    let params = lua
        .load("{ query = 'string', limit = { 'integer', 'maximum hits' } }")
        .eval::<mlua::Table>()
        .expect("params table evaluates");
    let schema = adhoc_params_schema(&params).expect("the schema builds");
    assert_eq!(
        schema["properties"],
        json!({
            "query": { "type": "string" },
            "limit": { "type": "integer", "description": "maximum hits" },
        })
    );
    assert_eq!(
        schema["required"],
        json!(["limit", "query"]),
        "required is emitted in sorted order, not Lua hash order"
    );
    assert_eq!(schema["type"], "object");
}

#[test]
fn adhoc_params_schema_sorts_required_so_the_schema_text_is_deterministic() {
    // `required` is the one Rust-side `table.pairs` walk whose output is an
    // ordered array rather than a table or map, so it must impose an order
    // itself: left as the walk produced it, two VMs emit different schema
    // text for the same params, and a stored tool definition stops being
    // replayable.
    let lua = Lua::new();
    let params = lua
        .load(
            "{ zulu = 'string', alpha = 'string', mike = 'string', bravo = 'string', \
               yankee = 'string', charlie = 'string', oscar = 'string', delta = 'string' }",
        )
        .eval::<mlua::Table>()
        .expect("params table evaluates");
    let schema = adhoc_params_schema(&params).expect("the schema builds");
    assert_eq!(
        schema["required"],
        json!([
            "alpha", "bravo", "charlie", "delta", "mike", "oscar", "yankee", "zulu"
        ]),
        "required must be sorted, not left in Lua hash order"
    );
}

#[test]
fn adhoc_params_schema_rejects_an_unsupported_type_under_the_adhoc_prefix() {
    let lua = Lua::new();
    let params = lua
        .load("{ payload = 'table' }")
        .eval::<mlua::Table>()
        .expect("params table evaluates");
    let error = adhoc_params_schema(&params).expect_err("an unsupported type fails");
    assert!(
        error
            .to_string()
            .contains("tools.adhoc: param \"payload\" has unsupported type \"table\""),
        "the rejection names the bad type: {error}"
    );
}

#[test]
fn the_tools_namespace_exposes_its_verbs_without_bind_or_call() {
    // `call` is absent here on purpose: it suspends, so the coroutine shim
    // prelude installs it - this table exposes exactly the non-suspending
    // operations. `bind` is absent too.
    let vm = TestVm::new().build();
    let shapes: (bool, bool, bool, bool, bool, bool) = vm.eval(
        "return type(tools.offer) == 'function', \
                type(tools.offer_dynamic) == 'function', \
                type(tools.adhoc) == 'function', \
                type(tools.packs) == 'table', \
                tools.call == nil, \
                tools.bind == nil",
    );
    assert_eq!(shapes, (true, true, true, true, true, true));
}

#[test]
fn the_shim_prelude_installs_tools_call_and_no_bare_global() {
    let vm = TestVm::new().shims().build();
    let (call_is_function, bare_is_nil): (bool, bool) =
        vm.eval("return type(tools.call) == 'function', tool_call == nil");
    assert!(call_is_function, "tools.call installs on the tools table");
    assert!(bare_is_nil, "the bare tool_call global is gone");
    vm.teardown();
}

#[test]
fn tool_call_counts_seed_by_id_and_explain_an_unseeded_key() {
    let echo = ToolBinding::from_info("tools_echo", &echo_tool());
    let vm = lua_over(ToolSet::new(
        Vec::new(),
        BTreeMap::new(),
        vec![
            echo.clone(),
            ToolBinding::from_info("tools_other", &tool_at("tools/other")),
        ],
    ));
    let counts = vm
        .install_tool_call_counts(&[echo])
        .expect("the counts install");
    assert_eq!(
        counts
            .get("tools/echo")
            .expect("the installed counts read any key"),
        Some(0)
    );
    assert_eq!(
        counts
            .get("tools_echo")
            .expect("the installed counts read any key"),
        None,
        "counts key a catalog tool by its id, never its wire name"
    );
    vm.exec("tools.adhoc({ name = 'grab', description = 'Grab a value' }, function() end)");
    let read = |key: &str| -> String {
        vm.eval(&format!(
            "local ok, err = pcall(function() return tools.calls[{key:?}] end); return tostring(err)"
        ))
    };
    let offered = read("tools/other");
    assert!(
        offered.contains(
            "tools.calls: \"tools/other\" has no seeded count; seeded names: [\"tools/echo\"] \
             (a catalog tool that was neither offered in this section nor called with \
             tools.call)"
        ),
        "an offered id that was never seeded says so: {offered}"
    );
    let adhoc = read("adhoc/grab");
    assert!(
        adhoc.contains(
            "tools.calls: \"adhoc/grab\" has no seeded count; seeded names: [\"tools/echo\"] \
             (an adhoc tool that was neither offered in this section nor called with \
             tools.call)"
        ),
        "a defined adhoc tool's id that was never seeded says so, even when defined \
         after the counts install: {adhoc}"
    );
    for key in ["ghost", "tools_echo", "grab"] {
        let error = read(key);
        assert!(
            error.contains(&format!("tools.calls: {key:?} has no seeded count"))
                && error.contains(" - check for typos or offer it with tools.offer"),
            "an unknown key names itself and the remedy: {error}"
        );
    }
}
