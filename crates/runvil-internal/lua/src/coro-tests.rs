//! Tests that the shim chunk names stay pointed at the shim files, and
//! that the one shim install puts `models.loop` and `models.compact` on
//! `models`.

use std::sync::{Arc, Mutex};

use mlua::{Lua, Value};
use runvil_model_client::model::{ModelBinding, ModelId, ModelInvocation};

use super::{
    FANOUT_CHUNK_NAME, FANOUT_SOURCE, SHIM_CHUNK_NAME, SHIM_SOURCE, TASKS_CHUNK_NAME, TASKS_SOURCE,
    selection_captures,
};
use crate::models::LuaModelHandle;
use crate::test_vm::TestVm;
use crate::tests::assert_chunk_name_resolves;

#[test]
fn the_shim_chunk_name_resolves_to_the_shim_file() {
    assert_chunk_name_resolves("SHIM_CHUNK_NAME", SHIM_CHUNK_NAME, SHIM_SOURCE);
}

#[test]
fn the_tasks_chunk_name_resolves_to_the_tasks_file() {
    assert_chunk_name_resolves("TASKS_CHUNK_NAME", TASKS_CHUNK_NAME, TASKS_SOURCE);
}

#[test]
fn the_fanout_chunk_name_resolves_to_the_fanout_file() {
    assert_chunk_name_resolves("FANOUT_CHUNK_NAME", FANOUT_CHUNK_NAME, FANOUT_SOURCE);
}

#[test]
fn the_yield_shim_install_puts_models_loop_and_models_compact_on_models() {
    let vm = TestVm::new().shims().build();
    let (looped, compacted): (String, String) =
        vm.eval("return type(models.loop), type(models.compact)");
    assert_eq!(
        (looped.as_str(), compacted.as_str()),
        ("function", "function"),
        "the coroutine shim install alone leaves both shims on `models`"
    );
}

/// A binding labelled `label`, over the model `model`.
fn binding(label: &str, model: &str) -> ModelBinding {
    ModelBinding::new(
        label,
        "A general model for tests",
        ModelId::gateway(model).expect("a static model id is valid"),
        ModelInvocation::new(),
        std::num::NonZeroU32::new(8192).expect("8192 is non-zero"),
    )
}

/// The label of the handle `value` holds, when it holds one.
fn handle_label(value: &Value) -> Option<String> {
    match value {
        Value::UserData(userdata) => userdata
            .borrow::<LuaModelHandle>()
            .ok()
            .map(|handle| handle.binding().alias().to_owned()),
        _ => None,
    }
}

#[test]
fn get_selection_returns_the_current_handle_or_nil() {
    let lua = Lua::new();
    let selection = Arc::new(Mutex::new(None));
    let (get, _) = selection_captures(&lua, &selection).expect("the captures build");

    let empty: Value = get.call(()).expect("a read of an empty selection succeeds");
    assert!(matches!(empty, Value::Nil), "no model reads as nil");

    *selection.lock().expect("the selection is not poisoned") = Some(binding("strong", "m2"));
    let current: Value = get.call(()).expect("a read of a held selection succeeds");
    assert_eq!(handle_label(&current).as_deref(), Some("strong"));
}

#[test]
fn set_selection_stores_a_handle_clears_on_nil_and_refuses_anything_else() {
    let lua = Lua::new();
    let selection = Arc::new(Mutex::new(Some(binding("default", "m1"))));
    let (_, set) = selection_captures(&lua, &selection).expect("the captures build");
    let label = || {
        selection
            .lock()
            .expect("the selection is not poisoned")
            .as_ref()
            .map(|held| held.alias().to_owned())
    };

    let handle = lua
        .create_userdata(LuaModelHandle::from_binding(binding("fast", "m3")))
        .expect("a handle builds");
    set.call::<()>(handle).expect("a handle is stored");
    assert_eq!(label().as_deref(), Some("fast"));

    set.call::<()>(Value::Nil)
        .expect("nil clears the selection");
    assert_eq!(label(), None);

    let error = set
        .call::<()>(5)
        .expect_err("a number is neither a handle nor nil");
    assert!(
        error
            .to_string()
            .contains("set_selection takes a model handle or nil, got number"),
        "the refusal names the Lua type: {error}"
    );
    assert_eq!(label(), None, "a refused write leaves the selection alone");
}
