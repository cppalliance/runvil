//! Tests for the VM's own locks as Lua callbacks take them: a poisoned
//! lock raises the internal failure, which no author `pcall` mistakes for
//! a Lua error.

use std::sync::{Arc, Mutex};

use crate::test_vm::{TestVm, offer_set};

/// Poisons `lock` by panicking on another thread while holding it.
fn poison<T: Send + 'static>(lock: &Arc<Mutex<T>>) {
    let held = Arc::clone(lock);
    let _ = std::thread::spawn(move || {
        let _guard = held.lock();
        panic!("poison the lock");
    })
    .join();
}

#[test]
fn a_poisoned_jump_slot_raises_the_internal_failure_inside_lua() {
    let vm = TestVm::new().shims().build();
    poison(&vm.jump_slot);
    let kind: String = vm.eval(
        "local ok, err = pcall(jump, '## Next')\n\
         assert(not ok, 'jump must raise')\n\
         return tostring(err.kind)",
    );
    assert_eq!(kind, "internal");
}

#[test]
fn a_poisoned_offers_lock_raises_the_internal_failure_inside_lua() {
    let vm = TestVm::new().tools(offer_set()).shims().build();
    poison(vm.offers());
    let kind: String = vm.eval(
        "local ok, err = pcall(tools.offer, 'web/search')\n\
         assert(not ok, 'the offer must raise')\n\
         return tostring(err.kind)",
    );
    assert_eq!(kind, "internal");
}
