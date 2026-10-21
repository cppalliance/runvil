//! Tests for the fixture's private copy of the C contract and its entry
//! point. The layout and number tests pin the same values as
//! `runvil_plugin::abi`, because this crate cannot import it. The verb
//! tests call `runvil_plugin` directly with a hand-built `RvHost` table
//! whose `describe_plugin` records what it was given.

use std::cell::{Cell, RefCell};
use std::ptr;
use std::slice;

#[cfg(target_pointer_width = "64")]
use std::mem::offset_of;

use super::*;

/// Verb 3, which the contract reserves for a later version.
const RV_PACK_NEW: RvVerb = 3;

/// What one `describe_plugin` call was handed, copied.
struct Described {
    call: *const RvCall,
    size: u32,
    reserved: u32,
    name: String,
    version: String,
}

thread_local! {
    /// Every `describe_plugin` call this test's thread made.
    static DESCRIBED: RefCell<Vec<Described>> = const { RefCell::new(Vec::new()) };
    /// The status `describe_plugin` answers with.
    static DESCRIBE_STATUS: Cell<RvStatus> = const { Cell::new(RV_OK) };
    /// How many times the Plugin called `refuse`, which it never should.
    static REFUSALS: Cell<usize> = const { Cell::new(0) };
}

/// Copies the bytes of `text` into a `String`.
fn copy(text: RvStr) -> String {
    // SAFETY: the entry point under test hands over a string that lives
    // for the whole `describe_plugin` call and holds `len` bytes.
    let bytes = unsafe { slice::from_raw_parts(text.ptr.cast::<u8>(), text.len) };
    String::from_utf8_lossy(bytes).into_owned()
}

/// The table's `describe_plugin`: records what it was given and answers
/// with `DESCRIBE_STATUS`.
unsafe extern "C" fn describe_plugin(call: *const RvCall, info: *const RvPluginInfo) -> RvStatus {
    // SAFETY: the entry point under test passes a live `RvPluginInfo`.
    let info = unsafe { &*info };
    let described = Described {
        call,
        size: info.size,
        reserved: info.reserved,
        name: copy(info.name),
        version: copy(info.version),
    };
    DESCRIBED.with_borrow_mut(|all| all.push(described));
    DESCRIBE_STATUS.get()
}

/// The table's `refuse`: counts itself, because this Plugin never refuses.
unsafe extern "C" fn refuse(_call: *const RvCall, _reason: RvStr) -> RvStatus {
    REFUSALS.set(REFUSALS.get() + 1);
    RV_ERR_REFUSED
}

/// A table that reports `size` and `api_major`.
fn host_with(size: u32, api_major: u32) -> RvHost {
    RvHost {
        size,
        api_major,
        api_minor: 1,
        reserved: 0,
        refuse,
        describe_plugin,
    }
}

/// A table the way the loader builds it.
fn host() -> RvHost {
    host_with(size_of_u32::<RvHost>(), 0)
}

/// A call that sends `verb` and carries `plugin`, with `host` as its table.
fn call_for(verb: RvVerb, host: &RvHost, plugin: *mut c_void) -> RvCall {
    RvCall {
        size: size_of_u32::<RvCall>(),
        verb,
        host: ptr::from_ref(host),
        plugin,
        pack_run: ptr::null_mut(),
        args: ptr::null(),
    }
}

/// Sends `call` to the entry point.
fn send(call: &mut RvCall) -> RvStatus {
    // SAFETY: `call` is a live `RvCall` whose table is live and as long as
    // its `size` says.
    unsafe { runvil_plugin(call) }
}

/// Sends `verb` with no instance and a table that reports `size` and
/// `api_major`, and returns the status and the call.
fn send_verb(verb: RvVerb, size: u32, api_major: u32) -> (RvStatus, RvCall) {
    let host = host_with(size, api_major);
    let mut call = call_for(verb, &host, ptr::null_mut());
    let status = send(&mut call);
    (status, call)
}

/// Loads an instance with a well-formed table, returning the call that
/// carries it.
fn loaded(host: &RvHost) -> RvCall {
    let mut call = call_for(RV_PLUGIN_LOAD, host, ptr::null_mut());
    assert_eq!(send(&mut call), RV_OK, "the load succeeds");
    call
}

/// How many times the Plugin called `describe_plugin`.
fn describes() -> usize {
    DESCRIBED.with_borrow(Vec::len)
}

/// How many instances this test's thread has made and not yet freed.
fn live_instances() -> isize {
    LIVE_INSTANCES.get()
}

#[test]
#[cfg(target_pointer_width = "64")]
fn a_string_is_a_pointer_then_a_length() {
    assert_eq!(size_of::<RvStr>(), 16);
    assert_eq!(offset_of!(RvStr, ptr), 0);
    assert_eq!(offset_of!(RvStr, len), 8);
}

#[test]
#[cfg(target_pointer_width = "64")]
fn the_plugin_info_keeps_the_layout_the_abi_module_pins() {
    assert_eq!(size_of::<RvPluginInfo>(), 40);
    assert_eq!(offset_of!(RvPluginInfo, size), 0);
    assert_eq!(offset_of!(RvPluginInfo, reserved), 4);
    assert_eq!(offset_of!(RvPluginInfo, name), 8);
    assert_eq!(offset_of!(RvPluginInfo, version), 24);
}

#[test]
#[cfg(target_pointer_width = "64")]
fn the_host_table_keeps_the_layout_the_abi_module_pins() {
    assert_eq!(size_of::<RvHost>(), 32);
    assert_eq!(offset_of!(RvHost, size), 0);
    assert_eq!(offset_of!(RvHost, api_major), 4);
    assert_eq!(offset_of!(RvHost, api_minor), 8);
    assert_eq!(offset_of!(RvHost, reserved), 12);
    assert_eq!(offset_of!(RvHost, refuse), 16);
    assert_eq!(offset_of!(RvHost, describe_plugin), 24);
}

#[test]
#[cfg(target_pointer_width = "64")]
fn the_call_keeps_the_layout_the_abi_module_pins() {
    assert_eq!(size_of::<RvCall>(), 40);
    assert_eq!(offset_of!(RvCall, size), 0);
    assert_eq!(offset_of!(RvCall, verb), 4);
    assert_eq!(offset_of!(RvCall, host), 8);
    assert_eq!(offset_of!(RvCall, plugin), 16);
    assert_eq!(offset_of!(RvCall, pack_run), 24);
    assert_eq!(offset_of!(RvCall, args), 32);
}

#[test]
fn the_status_codes_keep_their_numbers() {
    assert_eq!(RV_OK, 0);
    assert_eq!(RV_ERR_MISUSE, 4);
    assert_eq!(RV_ERR_REFUSED, 5);
    assert_eq!(RV_ERR_UNSUPPORTED, 6);
}

#[test]
fn the_verbs_and_the_version_keep_their_numbers() {
    assert_eq!(RV_PLUGIN_LOAD, 1);
    assert_eq!(RV_PLUGIN_UNLOAD, 2);
    assert_eq!(RV_API_MAJOR, 0);
}

#[test]
fn a_load_describes_test_plugin_once_and_sets_the_instance() {
    let host = host();
    // Sent in place, not through `loaded`, which moves the call and so
    // changes the address `describe_plugin` is compared with below.
    let mut call = call_for(RV_PLUGIN_LOAD, &host, ptr::null_mut());
    assert_eq!(send(&mut call), RV_OK, "the load succeeds");
    assert!(
        !call.plugin.is_null(),
        "a successful load sets the instance pointer"
    );
    DESCRIBED.with_borrow(|all| {
        assert_eq!(all.len(), 1, "the load describes the Plugin exactly once");
        let described = &all[0];
        assert_eq!(described.name, "test-plugin");
        assert_eq!(described.version, "0.1.0");
        assert_eq!(described.size, size_of_u32::<RvPluginInfo>());
        assert_eq!(described.reserved, 0);
        assert!(
            ptr::eq(described.call, &raw const call),
            "describe_plugin is handed the load's own call"
        );
    });
    assert_eq!(REFUSALS.get(), 0, "the Plugin never refuses");
    assert_eq!(live_instances(), 1, "a successful load keeps its instance");
    call.verb = RV_PLUGIN_UNLOAD;
    assert_eq!(send(&mut call), RV_OK);
    assert_eq!(live_instances(), 0, "the unload frees that instance");
}

#[test]
fn an_unload_frees_the_instance_the_load_set_and_describes_nothing() {
    let host = host();
    let mut call = loaded(&host);
    assert_eq!(live_instances(), 1, "the load made one instance");
    let before = describes();
    call.verb = RV_PLUGIN_UNLOAD;
    assert_eq!(send(&mut call), RV_OK);
    assert_eq!(
        live_instances(),
        0,
        "the unload drops the instance the load set, exactly once"
    );
    assert_eq!(
        describes(),
        before,
        "an unload names nobody: the RvHost functions answer only a load"
    );
    assert_eq!(REFUSALS.get(), 0);
}

#[test]
fn an_unload_with_no_instance_is_misuse() {
    let (status, call) = send_verb(RV_PLUGIN_UNLOAD, size_of_u32::<RvHost>(), 0);
    assert_eq!(status, RV_ERR_MISUSE, "a NULL plugin has nothing to free");
    assert!(call.plugin.is_null());
}

#[test]
fn the_reserved_and_unknown_verbs_are_unsupported() {
    for verb in [0, RV_PACK_NEW, 4, 5, 6, 7, 8, u32::MAX] {
        let (status, call) = send_verb(verb, size_of_u32::<RvHost>(), 0);
        assert_eq!(status, RV_ERR_UNSUPPORTED, "verb {verb}");
        assert!(call.plugin.is_null(), "verb {verb} sets no instance");
    }
    assert_eq!(describes(), 0, "no other verb names the Plugin");
}

#[test]
fn a_table_that_is_too_short_or_not_major_zero_is_unsupported_and_leaves_no_instance() {
    let short = [0, 4, 8, 28, size_of_u32::<RvHost>() - 1];
    let majors = [1, 2, u32::MAX];
    let tables = short
        .iter()
        .map(|size| (*size, 0))
        .chain(majors.iter().map(|major| (size_of_u32::<RvHost>(), *major)));
    for (size, api_major) in tables {
        let (status, call) = send_verb(RV_PLUGIN_LOAD, size, api_major);
        assert_eq!(
            status, RV_ERR_UNSUPPORTED,
            "a table of size {size} and major {api_major}"
        );
        assert!(
            call.plugin.is_null(),
            "a table of size {size} and major {api_major} leaves call.plugin NULL"
        );
    }
    assert_eq!(describes(), 0, "an unsupported table is never called back");
}

#[test]
fn a_table_longer_than_the_one_the_fixture_knows_still_loads() {
    let host = host_with(size_of_u32::<RvHost>() + 32, 0);
    let mut call = loaded(&host);
    assert!(!call.plugin.is_null());
    call.verb = RV_PLUGIN_UNLOAD;
    assert_eq!(send(&mut call), RV_OK);
}

#[test]
fn a_describe_plugin_that_fails_fails_the_load_with_its_status_and_no_instance() {
    for failing in [RV_ERR_MISUSE, 77] {
        DESCRIBE_STATUS.set(failing);
        let (status, call) = send_verb(RV_PLUGIN_LOAD, size_of_u32::<RvHost>(), 0);
        assert_eq!(status, failing, "the load returns describe_plugin's status");
        assert!(
            call.plugin.is_null(),
            "a failing load hands the loader no instance, because it made one and freed it"
        );
        assert_eq!(
            live_instances(),
            0,
            "a failing load frees the instance it made with status {failing}"
        );
    }
    assert_eq!(describes(), 2, "each failing load still described once");
}
