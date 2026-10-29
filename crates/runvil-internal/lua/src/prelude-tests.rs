//! Tests for Pack prelude install: the restricted environment, the
//! collision checks, the top-level seal, and load failures.

use std::num::NonZeroU32;
use std::sync::Arc;

use mlua::Value;
use runvil_types::packs::{PackId, Prelude};
use serde_json::json;

use super::install_preludes;
use crate::test_vm::{BuiltVm, TEST_SECTION, TestVm};
use crate::tests::recording::null_emitter;
use crate::tests::refusal_line;
use crate::{Argv, CoroStep, LuaProgram, Request, SectionVm, YieldParse, install_ui};

#[path = "prelude-tests-environment.rs"]
mod environment;

/// Builds a section VM through section setup up to where preludes install:
/// Engine injection with a bound `argv`, the Engine globals, the control
/// globals, and every coroutine yield shim, whose install puts
/// `models.loop` on `models` too.
#[track_caller]
fn prelude_vm(setup: TestVm<'_>) -> BuiltVm {
    let argv = json!({ "mode": "argv" });
    setup.argv(Argv::Frozen(Some(&argv))).shims().build()
}

/// [`section_vm`] with the guarded `var` seeded from `var`.
#[track_caller]
fn section_vm_with_var(var: &serde_json::Value) -> BuiltVm {
    prelude_vm(TestVm::new().var(var))
}

#[track_caller]
fn section_vm() -> BuiltVm {
    prelude_vm(TestVm::new())
}

fn prelude(pack: &str, source: &str) -> Prelude {
    Prelude::new(PackId::parse(pack).expect("a valid Pack id"), source)
}

/// Installs `preludes` and returns the failure's message.
fn install_failure(vm: &SectionVm, preludes: &[Prelude]) -> String {
    install_preludes(vm.lua(), preludes)
        .expect_err("the install must fail")
        .to_string()
}

/// Every name bound in the VM's `_G`, sorted.
fn global_names(vm: &SectionVm) -> Vec<String> {
    let mut names: Vec<String> = vm
        .lua()
        .globals()
        .pairs::<Value, Value>()
        .map(|pair| {
            let (key, _) = pair.expect("the globals walk");
            match key {
                Value::String(name) => name.to_string_lossy(),
                other => format!("{other:?}"),
            }
        })
        .collect();
    names.sort();
    names
}

/// The part of a failure message after `stack traceback:`.
fn traceback(message: &str) -> &str {
    message
        .split_once("stack traceback:")
        .map_or("", |(_, traceback)| traceback)
}

#[test]
fn a_table_global_is_sealed_at_its_top_level() {
    let vm = section_vm();
    install_preludes(
        vm.lua(),
        &[prelude(
            "kit",
            "kit = {}\nfunction kit.greet(name) return 'hi ' .. name end",
        )],
    )
    .expect("the prelude installs");

    let greeting: String = vm.eval("return kit.greet('ada')");
    assert_eq!(
        greeting, "hi ada",
        "the sealed table still reads its fields"
    );
    let (ok, message): (bool, String) =
        vm.eval("local ok, err = pcall(function() kit.extra = 1 end)\nreturn ok, tostring(err)");
    assert!(!ok, "assigning a field of a sealed table raises");
    assert!(
        message.contains("kit is read-only")
            && message.contains("Pack `kit`")
            && message.contains("'extra'"),
        "the refusal names the global, its Pack, and the field: {message}"
    );
    let seal: String = vm.eval("return getmetatable(kit)");
    assert_eq!(seal, "kit is sealed");
    let seen: i64 = vm.eval("local n = 0\nfor _ in pairs(kit) do n = n + 1 end\nreturn n");
    assert_eq!(seen, 0, "pairs over the sealed proxy sees nothing");
}

#[test]
fn a_sealed_global_raises_the_whole_refusal_for_a_string_integer_or_other_key() {
    let vm = section_vm();
    install_preludes(vm.lua(), &[prelude("kit", "kit = {}")]).expect("the prelude installs");
    for (target, refusal) in [
        (
            "kit.extra",
            "kit is read-only: Pack `kit` defines it; cannot set 'extra'",
        ),
        (
            "kit[1]",
            "kit is read-only: Pack `kit` defines it; cannot set '1'",
        ),
        (
            "kit[true]",
            "kit is read-only: Pack `kit` defines it; cannot set 'boolean'",
        ),
    ] {
        let message: String = vm.eval(&format!(
            "local ok, err = pcall(function() {target} = 1 end)\nreturn tostring(err)"
        ));
        assert_eq!(refusal_line(&message), Some(refusal), "writing {target}");
    }
}

#[test]
fn a_non_table_global_installs_as_it_is() {
    let vm = section_vm();
    install_preludes(
        vm.lua(),
        &[prelude(
            "kit",
            "answer = 42\nlabel = 'kit'\nfunction shout(text) return string.upper(text) end",
        )],
    )
    .expect("the prelude installs");

    let (answer, label, kind, shouted): (i64, String, String, String) =
        vm.eval("return answer, label, type(shout), shout('hey')");
    assert_eq!(
        (answer, label.as_str(), kind.as_str(), shouted.as_str()),
        (42, "kit", "function", "HEY")
    );
}

#[test]
fn a_prelude_receives_its_packs_name_as_its_chunk_argument() {
    let vm = section_vm();
    install_preludes(
        vm.lua(),
        &[
            prelude("kit", "local pack = ...\nkit_name = pack"),
            prelude("other-kit", "other_name = ..."),
        ],
    )
    .expect("the preludes install");
    let (kit, other): (String, String) = vm.eval("return kit_name, other_name");
    assert_eq!(
        (kit.as_str(), other.as_str()),
        ("kit", "other-kit"),
        "each prelude reads its own Pack's id from `...`"
    );
}

#[test]
fn a_prelude_function_reads_the_json_global() {
    let vm = section_vm();
    install_preludes(
        vm.lua(),
        &[prelude(
            "kit",
            "kit = {}\nfunction kit.probe() return type(json), json.decode('[1]')[1] end",
        )],
    )
    .expect("the prelude installs");
    let (kind, first): (String, i64) = vm.eval("return kit.probe()");
    assert_eq!((kind.as_str(), first), ("table", 1));
}

#[test]
fn a_prelude_function_calls_json_encode() {
    let vm = section_vm();
    install_preludes(
        vm.lua(),
        &[prelude(
            "kit",
            "kit = {}\nfunction kit.probe() return json.encode({ b = 1, a = { 1, 2 } }) end",
        )],
    )
    .expect("the prelude installs");
    let text: String = vm.eval("return kit.probe()");
    assert_eq!(text, r#"{"a":[1,2],"b":1}"#);
}

#[test]
fn json_encode_of_var_inside_a_prelude_function_cannot_read_the_view() {
    let vm = section_vm_with_var(&json!({ "mode": "fast" }));
    install_preludes(
        vm.lua(),
        &[prelude(
            "kit",
            "kit = {}\nfunction kit.encode_var() return json.encode(var) end",
        )],
    )
    .expect("the prelude installs");
    let (ok, kind, message): (bool, String, String) = vm.eval(
        "local ok, err = pcall(kit.encode_var)\n\
         return ok, err.kind, tostring(err)",
    );
    assert!(!ok, "encoding the prelude's var view raises");
    assert_eq!(
        (kind.as_str(), message.as_str()),
        (
            "lua",
            "json.encode: value cannot be read (var is read-only); \
             copy the fields you need into a plain table"
        )
    );
    let section_text: String = vm.eval("return json.encode(var)");
    assert_eq!(
        section_text, r#"{"mode":"fast"}"#,
        "section code reads its own var"
    );
}

#[test]
fn section_code_encoding_a_sealed_global_gets_the_preludes_hidden_table() {
    let vm = section_vm();
    install_preludes(
        vm.lua(),
        &[prelude("kit", "cfg = { name = 'kit', sizes = { 1, 2 } }")],
    )
    .expect("the prelude installs");
    let (text, seal): (String, String) = vm.eval("return json.encode(cfg), getmetatable(cfg)");
    assert_eq!(text, r#"{"name":"kit","sizes":[1,2]}"#);
    assert_eq!(seal, "cfg is sealed", "the global is the sealed view");
}

#[test]
fn a_prelude_that_assigns_no_global_installs_nothing() {
    let vm = section_vm();
    let before = global_names(&vm);
    install_preludes(
        vm.lua(),
        &[prelude(
            "quiet",
            "local helper = 1\nlocal function unused() return helper end",
        )],
    )
    .expect("the prelude installs");
    assert_eq!(global_names(&vm), before);
}

#[test]
fn a_global_that_collides_with_an_engine_global_fails_naming_both_sides() {
    let vm = section_vm();
    let message = install_failure(&vm, &[prelude("kit", "store = {}")]);
    assert!(
        message.contains("Pack `kit`")
            && message.contains("`store`")
            && message.contains("Engine global"),
        "the collision names the Pack, the global, and the Engine global: {message}"
    );
}

#[test]
fn a_global_named_tools_store_or_models_is_refused_and_leaves_the_namespace_in_place() {
    for name in ["tools", "store", "models"] {
        let vm = section_vm();
        let before: mlua::Table = vm.lua().globals().raw_get(name).expect("a raw read");
        let message = install_failure(&vm, &[prelude("kit", &format!("{name} = {{}}"))]);
        assert!(
            message.contains(&format!(
                "Pack `kit`: its prelude defines the global `{name}`, \
                 which is reserved as an Engine global"
            )),
            "the collision names the Pack, the global, and the reservation: {message}"
        );
        let after: mlua::Table = vm.lua().globals().raw_get(name).expect("a raw read");
        assert_eq!(
            after, before,
            "the refused prelude left `{name}` bound as it was"
        );
    }
}

#[test]
fn ui_and_item_collide_on_a_vm_that_binds_neither() {
    for name in ["ui", "item"] {
        let vm = section_vm();
        let bound: bool = vm.eval(&format!("return {name} ~= nil"));
        assert!(!bound, "the fixture VM binds no `{name}`");
        let message = install_failure(&vm, &[prelude("kit", &format!("{name} = 1"))]);
        assert!(
            message.contains("Pack `kit`")
                && message.contains(&format!("`{name}`"))
                && message.contains("reserved"),
            "the collision names the Pack and the reserved global: {message}"
        );
    }
}

#[test]
fn argv_prose_and_messages_collide_though_the_g_metatable_serves_them() {
    let vm = section_vm();
    vm.install_lazy_prose(|_| Ok("the block's prose".to_owned()))
        .expect("the prose guard installs");
    for name in ["argv", "prose", "messages"] {
        let raw: Value = vm.lua().globals().raw_get(name).expect("a raw read");
        assert!(
            raw.is_nil(),
            "`{name}` is not a raw global on the fixture VM"
        );
        let served: bool = vm.eval(&format!("return {name} ~= nil"));
        assert!(served, "the `_G` metatable serves `{name}` to author code");
        let message = install_failure(&vm, &[prelude("kit", &format!("{name} = 1"))]);
        assert!(
            message.contains("Pack `kit`")
                && message.contains(&format!("`{name}`"))
                && message.contains("reserved"),
            "the collision names the Pack and the reserved global: {message}"
        );
    }
}

#[test]
fn a_prelude_sets_and_reads_metatables_as_the_base_functions_do() {
    let vm = section_vm();
    install_preludes(
        vm.lua(),
        &[prelude(
            "meta",
            "kit = {}\n\
             local shape = { __index = function(_, key) return key .. '!' end }\n\
             function kit.make() return setmetatable({}, shape) end\n\
             function kit.shaped(t) return getmetatable(t) == shape end\n\
             function kit.lock()\n\
               local t = setmetatable({}, { __metatable = 'locked' })\n\
               local ok, err = pcall(setmetatable, t, {})\n\
               return getmetatable(t), ok, tostring(err)\n\
             end",
        )],
    )
    .expect("the prelude installs");
    let (field, shaped): (String, bool) =
        vm.eval("local t = kit.make()\nreturn t.x, kit.shaped(t)");
    assert_eq!((field.as_str(), shaped), ("x!", true));
    let (label, ok, message): (String, bool, String) = vm.eval("return kit.lock()");
    assert_eq!(label, "locked");
    assert!(!ok, "a protected metatable refuses replacement");
    assert_eq!(message, "cannot change a protected metatable");
}

#[test]
fn two_preludes_defining_one_global_fail_naming_both_packs() {
    let vm = section_vm();
    let message = install_failure(
        &vm,
        &[prelude("one", "shared = 1"), prelude("two", "shared = 2")],
    );
    assert!(
        message.contains("Pack `two`")
            && message.contains("`shared`")
            && message.contains("Pack `one`"),
        "the collision names the global and both Packs: {message}"
    );
}

#[test]
fn a_global_whose_name_is_not_a_utf8_string_fails_naming_the_key() {
    for (source, key) in [
        ("_ENV[1] = true", "a key of type integer"),
        (
            "_ENV['\\xff'] = true",
            "a string key that is not valid UTF-8",
        ),
    ] {
        let vm = section_vm();
        let message = install_failure(&vm, &[prelude("kit", source)]);
        assert!(
            message.contains("Pack `kit`")
                && message.contains(key)
                && message.contains("must be a UTF-8 string"),
            "the refusal names the Pack and the key: {message}"
        );
    }
}

#[test]
fn a_prelude_that_raises_while_loading_fails_naming_its_pack() {
    let vm = section_vm();
    let message = install_failure(&vm, &[prelude("boom", "local x = 1\nerror('boom')")]);
    assert!(
        message.starts_with("Pack `boom`: its prelude failed to load: ")
            && message.contains("boom.")
            && message.contains(
                "A prelude only defines functions; it must not call tools while loading."
            ),
        "the failure names the Pack and gives the rule: {message}"
    );
    assert!(
        traceback(&message).contains("pack:boom:2:"),
        "the traceback names the prelude chunk and line: {message}"
    );
}

#[test]
fn a_prelude_that_calls_a_tool_while_loading_fails_naming_its_pack() {
    let vm = section_vm();
    let message = install_failure(&vm, &[prelude("eager", "tools.call('eager/run')")]);
    assert!(
        message.starts_with("Pack `eager`: its prelude failed to load: ")
            && message.contains("yield")
            && message.contains("it must not call tools while loading."),
        "the failure names the Pack and gives the rule: {message}"
    );
    assert!(
        traceback(&message).contains("pack:eager:1:"),
        "the traceback names the prelude chunk and line: {message}"
    );
}
