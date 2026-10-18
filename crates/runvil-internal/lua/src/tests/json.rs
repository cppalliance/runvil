//! The `json` global: `json.decode` and `json.decode_lines` over JSON text,
//! `json.null`, the `json.array` list marker, and decoding the body of an
//! `untrusted` envelope.

use super::*;

mod decode;
mod envelope;
mod lines;
mod marked;

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
