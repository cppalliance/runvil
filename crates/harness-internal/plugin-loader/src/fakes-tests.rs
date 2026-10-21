//! Fake entry points for the unit tests: plain `extern "C" fn`s written
//! against `runvil_plugin::abi`, and the helpers that call the `RvHost`
//! functions the way a Plugin does. Each test defines its own fake with
//! `fake!`, so its statics belong to it alone.

use std::ffi::c_void;
use std::ptr;
use std::sync::Arc;

use runvil_plugin::PackRegistry;
pub(crate) use runvil_plugin::abi::{
    PluginEntry, RV_ERR_MISUSE, RV_ERR_REFUSED, RV_ERR_UNSUPPORTED, RV_OK, RV_PLUGIN_LOAD,
    RV_PLUGIN_UNLOAD, RvCall, RvHost, RvPluginInfo, RvStatus, RvStr, RvVerb,
};

use crate::host::HOST;
use crate::plugin::{Plugin, PluginFailure, insert_plugin};

/// A status no verb returns, for a fake that fails with its own code.
pub(crate) const ODD_STATUS: RvStatus = 77;

/// Defines `mod $name` holding a fake entry point and its statics. Its
/// `RV_PLUGIN_LOAD` runs `$load`, which sees the call as `$call` and
/// returns the load's status. Its `RV_PLUGIN_UNLOAD` counts itself in
/// `UNLOADS`, stores the instance pointer it was sent in `UNLOADED`,
/// records whether the call carried the contract's fields in
/// `UNLOAD_WELL_FORMED`, and stores the status of a `describe_plugin` it
/// then tries in `UNLOAD_STATUS`. `LAST_CALL` is the address of the call
/// it got last. The second form adds the items a test needs the load body
/// to share with it, such as a `pub(super) static`.
macro_rules! fake {
    ($name:ident, |$call:ident| $load:expr) => {
        $crate::fakes::fake!($name, {}, |$call| $load);
    };
    ($name:ident, { $($items:item)* }, |$call:ident| $load:expr) => {
        mod $name {
            use std::ffi::c_void;
            use std::ptr;
            use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};

            use $crate::fakes::*;

            /// How many RV_PLUGIN_UNLOAD calls this fake got.
            pub(super) static UNLOADS: AtomicUsize = AtomicUsize::new(0);
            /// The instance pointer its last RV_PLUGIN_UNLOAD carried.
            pub(super) static UNLOADED: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
            /// Whether its last RV_PLUGIN_UNLOAD carried the contract's fields.
            pub(super) static UNLOAD_WELL_FORMED: AtomicBool = AtomicBool::new(false);
            /// The status a `describe_plugin` during RV_PLUGIN_UNLOAD got,
            /// plus one, so zero means it never tried.
            pub(super) static UNLOAD_STATUS: AtomicUsize = AtomicUsize::new(0);
            /// The address of the last call the fake was sent, which a
            /// test reads to hand an `RvHost` function a stale call.
            pub(super) static LAST_CALL: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

            $($items)*

            pub(super) extern "C" fn entry($call: *mut RvCall) -> RvStatus {
                LAST_CALL.store($call.cast(), Ordering::SeqCst);
                match verb($call) {
                    RV_PLUGIN_LOAD => $load,
                    RV_PLUGIN_UNLOAD => {
                        UNLOADS.fetch_add(1, Ordering::SeqCst);
                        UNLOADED.store(plugin($call), Ordering::SeqCst);
                        UNLOAD_WELL_FORMED
                            .store(contract_holds($call, RV_PLUGIN_UNLOAD), Ordering::SeqCst);
                        let status = describe($call, "acme-url", "1.0.0");
                        UNLOAD_STATUS.store(status as usize + 1, Ordering::SeqCst);
                        RV_OK
                    }
                    _ => RV_ERR_UNSUPPORTED,
                }
            }
        }
    };
}
pub(crate) use fake;

/// Inserts the linked Plugin `entry` into a fresh Pack registry.
pub(crate) fn insert(entry: PluginEntry) -> Result<Arc<Plugin>, PluginFailure> {
    insert_plugin(&mut PackRegistry::new(), entry)
}

/// Inserts `entry`, which must load.
pub(crate) fn loaded(entry: PluginEntry) -> Arc<Plugin> {
    insert(entry).expect("the Plugin loads")
}

/// Inserts `entry`, which must fail to load.
pub(crate) fn failure(entry: PluginEntry) -> PluginFailure {
    insert(entry).expect_err("the Plugin fails to load")
}

/// Whether `call` carries the fields the contract gives a call that sends
/// `verb` in version 0.1, and its table says the contract's version. A
/// load call carries no instance yet; an unload call carries one.
pub(crate) fn contract_holds(call: *mut RvCall, verb: RvVerb) -> bool {
    // SAFETY: a fake runs only inside an entry point call, where `call` is
    // the loader's live `RvCall` and its table is the loader's static.
    let (call, table) = unsafe { (&*call, &*(*call).host) };
    let is_load = verb == RV_PLUGIN_LOAD;
    call.size == u32::try_from(size_of::<RvCall>()).unwrap_or(0)
        && call.verb == verb
        && ptr::eq(call.host, &raw const HOST)
        && call.plugin.is_null() == is_load
        && call.pack_run.is_null()
        && call.args.is_null()
        && table.size == u32::try_from(size_of::<RvHost>()).unwrap_or(0)
        && table.api_major == 0
        && table.api_minor == 1
        && table.reserved == 0
}

/// The verb a call carries.
pub(crate) fn verb(call: *mut RvCall) -> RvVerb {
    // SAFETY: as in `contract_holds`.
    unsafe { (*call).verb }
}

/// The instance pointer a call carries.
pub(crate) fn plugin(call: *mut RvCall) -> *mut c_void {
    // SAFETY: as in `contract_holds`.
    unsafe { (*call).plugin }
}

/// Sets the instance pointer, as RV_PLUGIN_LOAD may.
pub(crate) fn set_plugin(call: *mut RvCall, plugin: *mut c_void) {
    // SAFETY: as in `contract_holds`, and nothing else reads the call
    // meanwhile.
    unsafe { (*call).plugin = plugin };
}

/// An address that is never null, which a fake hands out as its instance.
pub(crate) fn instance(tag: usize) -> *mut c_void {
    ptr::without_provenance_mut(tag)
}

/// The `RvHost` table a call carries.
pub(crate) fn host(call: *mut RvCall) -> &'static RvHost {
    // SAFETY: as in `contract_holds`.
    unsafe { &*(*call).host }
}

/// A string for the C side, borrowed from `text`.
pub(crate) fn rv_str(text: &str) -> RvStr {
    RvStr {
        ptr: text.as_ptr().cast(),
        len: text.len(),
    }
}

/// A string for the C side, borrowed from `bytes`.
pub(crate) fn rv_str_bytes(bytes: &[u8]) -> RvStr {
    RvStr {
        ptr: bytes.as_ptr().cast(),
        len: bytes.len(),
    }
}

/// A well-formed `RvPluginInfo`.
pub(crate) fn info(name: RvStr, version: RvStr) -> RvPluginInfo {
    RvPluginInfo {
        size: u32::try_from(size_of::<RvPluginInfo>()).expect("the info's size fits a u32"),
        reserved: 0,
        name,
        version,
    }
}

/// Calls the call's `describe_plugin` with `info`.
pub(crate) fn describe_info(call: *mut RvCall, info: &RvPluginInfo) -> RvStatus {
    describe_as(call, call, info)
}

/// Calls the `describe_plugin` of `table_from`'s table, handing it `as_call`.
pub(crate) fn describe_as(
    table_from: *mut RvCall,
    as_call: *const RvCall,
    info: &RvPluginInfo,
) -> RvStatus {
    // SAFETY: `info` is a live `RvPluginInfo` whose strings outlive the
    // call, and the function never dereferences `as_call`.
    unsafe { (host(table_from).describe_plugin)(as_call, info) }
}

/// Describes the Plugin with `name` and `version`.
pub(crate) fn describe(call: *mut RvCall, name: &str, version: &str) -> RvStatus {
    describe_info(call, &info(rv_str(name), rv_str(version)))
}

/// Calls the `refuse` of `table_from`'s table, handing it `as_call`.
pub(crate) fn refuse_as(
    table_from: *mut RvCall,
    as_call: *const RvCall,
    reason: RvStr,
) -> RvStatus {
    // SAFETY: `reason` outlives the call, and the function never
    // dereferences `as_call`.
    unsafe { (host(table_from).refuse)(as_call, reason) }
}

/// Calls the call's `refuse` with `reason`.
pub(crate) fn refuse_str(call: *mut RvCall, reason: RvStr) -> RvStatus {
    refuse_as(call, call, reason)
}

/// Refuses the load with `reason`.
pub(crate) fn refuse(call: *mut RvCall, reason: &str) -> RvStatus {
    refuse_str(call, rv_str(reason))
}
