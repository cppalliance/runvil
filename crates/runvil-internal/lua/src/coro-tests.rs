//! Tests that the shim chunk names stay pointed at the shim files, and
//! that the one shim install puts `models.loop` and `models.compact` on
//! `models`.

use super::{
    FANOUT_CHUNK_NAME, FANOUT_SOURCE, SHIM_CHUNK_NAME, SHIM_SOURCE, TASKS_CHUNK_NAME, TASKS_SOURCE,
};
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
