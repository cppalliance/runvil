//! Tests for the fixture's private copy of the C contract and its entry
//! point. The layout and number tests pin the same values as
//! `runvil-plugin-sys`, because this crate cannot import it. The verb
//! tests call `runvil_plugin` directly with a hand-built `RvHost` table
//! and `RvLoadArgs`, whose `describe_plugin` records what it was given.

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
    ctx: *mut c_void,
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

/// The load arguments' `describe_plugin`: records what it was given and
/// answers with `DESCRIBE_STATUS`.
unsafe extern "C" fn describe_plugin(ctx: *mut c_void, info: *const RvPluginInfo) -> RvStatus {
    // SAFETY: the entry point under test passes a live `RvPluginInfo`.
    let info = unsafe { &*info };
    let described = Described {
        ctx,
        size: info.size,
        reserved: info.reserved,
        name: copy(info.name),
        version: copy(info.version),
    };
    DESCRIBED.with_borrow_mut(|all| all.push(described));
    DESCRIBE_STATUS.get()
}

/// The load arguments' `refuse`: counts itself, because this Plugin never
/// refuses.
unsafe extern "C" fn refuse(_ctx: *mut c_void, _reason: RvStr) -> RvStatus {
    REFUSALS.set(REFUSALS.get() + 1);
    RV_ERR_REFUSED
}

/// A table that reports `size` and version `api_major`.`api_minor`.
fn host_with(size: u32, api_major: u32, api_minor: u32) -> RvHost {
    RvHost {
        size,
        api_major,
        api_minor,
        reserved: 0,
    }
}

/// A table the way the loader builds it.
fn host() -> RvHost {
    host_with(size_of_u32::<RvHost>(), 0, 2)
}

/// The `ctx` the tests hand the Plugin. The Plugin never reads through it,
/// so any address that is not NULL will do.
fn test_ctx() -> *mut c_void {
    ptr::without_provenance_mut(0xc7c7)
}

/// Load arguments that report `size`.
fn args_with(size: u32) -> RvLoadArgs {
    RvLoadArgs {
        size,
        reserved: 0,
        ctx: test_ctx(),
        refuse,
        describe_plugin,
    }
}

/// Load arguments the way the loader builds them.
fn args() -> RvLoadArgs {
    args_with(size_of_u32::<RvLoadArgs>())
}

/// A call that sends `verb` and carries `plugin`, with `host` as its table
/// and `args` as its arguments.
fn call_for(verb: RvVerb, host: &RvHost, args: *const RvLoadArgs, plugin: *mut c_void) -> RvCall {
    RvCall {
        size: size_of_u32::<RvCall>(),
        verb,
        host: ptr::from_ref(host),
        plugin,
        pack_run: ptr::null_mut(),
        args: args.cast(),
    }
}

/// Sends `call` to the entry point.
fn send(call: &mut RvCall) -> RvStatus {
    // SAFETY: `call` is a live `RvCall` whose table is live and as long as
    // its `size` says, and whose `args` is NULL or live and as long as its
    // `size` says.
    unsafe { runvil_plugin(call) }
}

/// Sends `verb` with no instance, the table `host`, and the arguments
/// `args`, and returns the status and the call.
fn send_verb(verb: RvVerb, host: &RvHost, args: *const RvLoadArgs) -> (RvStatus, RvCall) {
    let mut call = call_for(verb, host, args, ptr::null_mut());
    let status = send(&mut call);
    (status, call)
}

/// Sends `RV_PLUGIN_LOAD` with the table `host` and well-formed arguments.
fn send_load_with(host: &RvHost) -> (RvStatus, RvCall) {
    let args = args();
    send_verb(RV_PLUGIN_LOAD, host, &raw const args)
}

/// Loads an instance with a well-formed table, returning the call that
/// carries it. The call's `args` is dangling afterward, which an unload
/// never reads.
fn loaded(host: &RvHost) -> RvCall {
    let (status, call) = send_load_with(host);
    assert_eq!(status, RV_OK, "the load succeeds");
    call
}

/// Unloads the instance `call` carries.
fn unload(call: &mut RvCall) -> RvStatus {
    call.verb = RV_PLUGIN_UNLOAD;
    call.args = ptr::null();
    send(call)
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
fn the_plugin_info_keeps_the_layout_the_contract_pins() {
    assert_eq!(size_of::<RvPluginInfo>(), 40);
    assert_eq!(offset_of!(RvPluginInfo, size), 0);
    assert_eq!(offset_of!(RvPluginInfo, reserved), 4);
    assert_eq!(offset_of!(RvPluginInfo, name), 8);
    assert_eq!(offset_of!(RvPluginInfo, version), 24);
}

#[test]
#[cfg(target_pointer_width = "64")]
fn the_host_table_keeps_the_layout_the_contract_pins() {
    assert_eq!(size_of::<RvHost>(), 16);
    assert_eq!(offset_of!(RvHost, size), 0);
    assert_eq!(offset_of!(RvHost, api_major), 4);
    assert_eq!(offset_of!(RvHost, api_minor), 8);
    assert_eq!(offset_of!(RvHost, reserved), 12);
}

#[test]
#[cfg(target_pointer_width = "64")]
fn the_load_arguments_keep_the_layout_the_contract_pins() {
    assert_eq!(size_of::<RvLoadArgs>(), 32);
    assert_eq!(offset_of!(RvLoadArgs, size), 0);
    assert_eq!(offset_of!(RvLoadArgs, reserved), 4);
    assert_eq!(offset_of!(RvLoadArgs, ctx), 8);
    assert_eq!(offset_of!(RvLoadArgs, refuse), 16);
    assert_eq!(offset_of!(RvLoadArgs, describe_plugin), 24);
}

#[test]
#[cfg(target_pointer_width = "64")]
fn the_call_keeps_the_layout_the_contract_pins() {
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
    assert_eq!(RV_API_MINOR, 2);
}

#[test]
fn a_load_describes_test_plugin_once_through_its_ctx_and_sets_the_instance() {
    let host = host();
    let mut call = loaded(&host);
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
        assert_eq!(
            described.ctx,
            test_ctx(),
            "describe_plugin is handed the load's own ctx"
        );
    });
    assert_eq!(REFUSALS.get(), 0, "the Plugin never refuses");
    assert_eq!(live_instances(), 1, "a successful load keeps its instance");
    assert_eq!(unload(&mut call), RV_OK);
    assert_eq!(live_instances(), 0, "the unload frees that instance");
}

#[test]
fn an_unload_frees_the_instance_the_load_set_and_describes_nothing() {
    let host = host();
    let mut call = loaded(&host);
    assert_eq!(live_instances(), 1, "the load made one instance");
    let before = describes();
    assert_eq!(unload(&mut call), RV_OK);
    assert_eq!(
        live_instances(),
        0,
        "the unload drops the instance the load set, exactly once"
    );
    assert_eq!(
        describes(),
        before,
        "an unload names nobody: the load functions answer only a load"
    );
    assert_eq!(REFUSALS.get(), 0);
}

#[test]
fn an_unload_with_no_instance_is_misuse() {
    let (status, call) = send_verb(RV_PLUGIN_UNLOAD, &host(), ptr::null());
    assert_eq!(status, RV_ERR_MISUSE, "a NULL plugin has nothing to free");
    assert!(call.plugin.is_null());
}

#[test]
fn the_reserved_and_unknown_verbs_are_unsupported() {
    for verb in [0, RV_PACK_NEW, 4, 5, 6, 7, 8, u32::MAX] {
        let (status, call) = send_verb(verb, &host(), ptr::null());
        assert_eq!(status, RV_ERR_UNSUPPORTED, "verb {verb}");
        assert!(call.plugin.is_null(), "verb {verb} sets no instance");
    }
    assert_eq!(describes(), 0, "no other verb names the Plugin");
}

#[test]
fn a_host_reporting_version_0_1_is_unsupported_and_leaves_no_instance() {
    let (status, call) = send_load_with(&host_with(size_of_u32::<RvHost>(), 0, 1));
    assert_eq!(status, RV_ERR_UNSUPPORTED);
    assert!(call.plugin.is_null());
    assert_eq!(describes(), 0, "a 0.1 Host is never called back");
    assert_eq!(live_instances(), 0);
}

#[test]
fn a_table_that_is_too_short_or_not_version_0_2_is_unsupported_and_leaves_no_instance() {
    let full = size_of_u32::<RvHost>();
    let short = [0, 4, 8, 12, full - 1].map(|size| (size, 0, 2));
    let versions = [(1, 2), (2, 2), (u32::MAX, 2), (0, 0), (0, 3), (0, u32::MAX)]
        .map(|(major, minor)| (full, major, minor));
    for (size, api_major, api_minor) in short.into_iter().chain(versions) {
        let (status, call) = send_load_with(&host_with(size, api_major, api_minor));
        assert_eq!(
            status, RV_ERR_UNSUPPORTED,
            "a table of size {size} and version {api_major}.{api_minor}"
        );
        assert!(
            call.plugin.is_null(),
            "a table of size {size} and version {api_major}.{api_minor} leaves call.plugin NULL"
        );
    }
    assert_eq!(describes(), 0, "an unsupported table is never called back");
    assert_eq!(live_instances(), 0);
}

#[test]
fn a_table_longer_than_the_one_the_fixture_knows_still_loads() {
    let host = host_with(size_of_u32::<RvHost>() + 32, 0, 2);
    let mut call = loaded(&host);
    assert!(!call.plugin.is_null());
    assert_eq!(unload(&mut call), RV_OK);
}

#[test]
fn a_load_with_null_args_is_misuse_and_leaves_no_instance() {
    let (status, call) = send_verb(RV_PLUGIN_LOAD, &host(), ptr::null());
    assert_eq!(status, RV_ERR_MISUSE, "a load needs its RvLoadArgs");
    assert!(call.plugin.is_null());
    assert_eq!(describes(), 0);
    assert_eq!(live_instances(), 0);
}

#[test]
fn short_load_arguments_are_unsupported_and_leave_no_instance() {
    let full = size_of_u32::<RvLoadArgs>();
    for size in [0, 4, 8, 16, 24, full - 1] {
        let args = args_with(size);
        let (status, call) = send_verb(RV_PLUGIN_LOAD, &host(), &raw const args);
        assert_eq!(status, RV_ERR_UNSUPPORTED, "load arguments of size {size}");
        assert!(
            call.plugin.is_null(),
            "load arguments of size {size} leave call.plugin NULL"
        );
    }
    assert_eq!(describes(), 0, "short load arguments are never called");
    assert_eq!(live_instances(), 0);
}

#[test]
fn a_host_that_is_not_0_2_is_rejected_before_null_args_are_noticed() {
    let (status, _call) = send_verb(
        RV_PLUGIN_LOAD,
        &host_with(size_of_u32::<RvHost>(), 0, 1),
        ptr::null(),
    );
    assert_eq!(
        status, RV_ERR_UNSUPPORTED,
        "the version is checked before args is read"
    );
}

#[test]
fn a_describe_plugin_that_fails_fails_the_load_with_its_status_and_no_instance() {
    for failing in [RV_ERR_MISUSE, 77] {
        DESCRIBE_STATUS.set(failing);
        let (status, call) = send_load_with(&host());
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
