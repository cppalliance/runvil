//! The `json` global: `json.decode` and `json.decode_lines` over JSON text,
//! `json.encode` and `json.encode_lines` over Lua values, `json.null`, the
//! `json.array` list marker, and decoding the body of an `untrusted`
//! envelope.

use mlua::{LightUserData, MetaMethod, UserData, UserDataMethods};

use super::*;

mod debug;
mod decode;
mod encode;
mod envelope;
mod errors;
mod lines;
mod marked;
mod views;

/// A test VM with the coroutine shims, whose `pcall` hands a caught Engine
/// error back as an error table.
#[track_caller]
fn json_vm() -> BuiltVm {
    TestVm::new().shims().build()
}

/// Runs `call` under `pcall` on `vm` and returns the caught error's kind and
/// message, failing the test when the call returns.
#[track_caller]
fn caught(vm: &BuiltVm, call: &str) -> (String, String) {
    let (raised, kind, message): (bool, String, String) = vm.eval(&format!(
        "local ok, err = pcall(function() return {call} end)\n\
         if ok then return false, '', '' end\n\
         return true, tostring(err.kind), tostring(err)"
    ));
    assert!(raised, "required {call} to raise, actual a return");
    (kind, message)
}

/// The `(kind, message)` pair of a caught `lua` error.
fn lua_error(message: &str) -> (String, String) {
    ("lua".to_owned(), message.to_owned())
}

/// Evaluates the Lua expression `call` on `vm` and returns the string it
/// gives.
#[track_caller]
fn encoded(vm: &BuiltVm, call: &str) -> String {
    vm.eval(&format!("return {call}"))
}

/// A userdata whose `__tostring` gives `probe`, which debug mode never
/// calls.
struct Described;

impl UserData for Described {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::ToString, |_, _, ()| Ok("probe"));
    }
}

/// Binds a coroutine as `co`, a non-null light userdata as `light`, and a
/// userdata defining `__tostring` as `described`, none of which section
/// code can build for itself.
fn bind_foreign_values(vm: &BuiltVm) {
    let lua = vm.lua();
    let globals = lua.globals();
    let thread = lua
        .create_thread(
            lua.create_function(|_, ()| Ok(()))
                .expect("the thread body builds"),
        )
        .expect("the thread builds");
    globals
        .raw_set("co", thread)
        .expect("the global co installs");
    globals
        .raw_set(
            "light",
            Value::LightUserData(LightUserData(std::ptr::without_provenance_mut(1))),
        )
        .expect("the global light installs");
    globals
        .raw_set(
            "described",
            lua.create_userdata(Described).expect("the userdata builds"),
        )
        .expect("the global described installs");
}

/// The path `value` followed by `count` steps of `[1]`.
fn nested_path(count: usize) -> String {
    format!("value{}", "[1]".repeat(count))
}
