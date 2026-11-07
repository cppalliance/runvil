//! `harness-test-plugin` - a library Plugin for the loader's end-to-end
//! tests: a dynamic library that exports `runvil_plugin` and follows the C
//! contract.
//!
//! ## Invariants
//!
//! - No `[dependencies]` at all, so a library Plugin needs nothing from
//!   Runvil. Only the end-to-end tests in `tests/it` name other crates,
//!   `harness-plugin-loader` and `runvil-plugin`, as `[dev-dependencies]`.
//!   The C types below are a private copy of the contract's, not an
//!   import.
//! - Only `runvil_plugin` is public. The mirrored C types stay private, and
//!   unit tests pin them to the same layout numbers as
//!   `runvil-plugin-sys`.
//! - The entry point never unwinds: it has no `unwrap`, `expect`, or
//!   indexing that can panic.
//! - `RV_PLUGIN_LOAD` loads only when the `RvHost` table reports exactly
//!   version 0.2, and checks that before it reads `args`.
//! - `RV_PLUGIN_LOAD` describes the Plugin exactly once, as `test-plugin`
//!   at `0.1.0`, and sets the instance pointer only when that succeeds. A
//!   failing load frees what it made.
#![expect(unsafe_code, reason = "exports a C entry point")]

use std::ffi::{c_char, c_void};

#[cfg(test)]
#[path = "lib-tests.rs"]
mod tests;

/// What every function in the contract returns: zero for success.
type RvStatus = u32;
/// Success.
const RV_OK: RvStatus = 0;
/// The caller broke the contract.
const RV_ERR_MISUSE: RvStatus = 4;
/// The Plugin declined to load. This Plugin never refuses.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "pinned by the unit tests; never returned")
)]
const RV_ERR_REFUSED: RvStatus = 5;
/// The Plugin does not know the verb it was sent.
const RV_ERR_UNSUPPORTED: RvStatus = 6;

/// The operation an `RvCall` asks for.
type RvVerb = u32;
/// Make the Plugin instance and name the Plugin.
const RV_PLUGIN_LOAD: RvVerb = 1;
/// Free the Plugin instance.
const RV_PLUGIN_UNLOAD: RvVerb = 2;

/// The contract's major version, which the `RvHost` table must report.
const RV_API_MAJOR: u32 = 0;
/// The contract's minor version, which the `RvHost` table must report.
const RV_API_MINOR: u32 = 2;

/// The name this Plugin gives `describe_plugin`.
const NAME: &str = "test-plugin";
/// The version this Plugin gives `describe_plugin`.
const VERSION: &str = "0.1.0";

/// A UTF-8 string that is not NUL-terminated.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct RvStr {
    /// The first byte.
    ptr: *const c_char,
    /// The length in bytes.
    len: usize,
}

/// What a Plugin tells `describe_plugin` about itself.
#[repr(C)]
#[derive(Debug)]
struct RvPluginInfo {
    /// `size_of::<RvPluginInfo>()`.
    size: u32,
    /// Zero.
    reserved: u32,
    /// The Plugin's name.
    name: RvStr,
    /// The Plugin's version.
    version: RvStr,
}

/// What the `RvHost` table reports on every call: the contract version.
#[repr(C)]
#[derive(Debug)]
struct RvHost {
    /// `size_of::<RvHost>()`.
    size: u32,
    /// The contract's major version.
    api_major: u32,
    /// The contract's minor version.
    api_minor: u32,
    /// Zero.
    reserved: u32,
}

/// The arguments of `RV_PLUGIN_LOAD`: the functions the Plugin may call
/// while it loads, each taking `ctx` first.
#[repr(C)]
#[derive(Debug)]
struct RvLoadArgs {
    /// `size_of::<RvLoadArgs>()`.
    size: u32,
    /// Zero.
    reserved: u32,
    /// The loader's context for this load, which the Plugin passes back and
    /// never reads through.
    ctx: *mut c_void,
    /// Declines the load. This Plugin never calls it.
    refuse: unsafe extern "C" fn(ctx: *mut c_void, reason: RvStr) -> RvStatus,
    /// Names the Plugin, exactly once.
    describe_plugin: unsafe extern "C" fn(ctx: *mut c_void, info: *const RvPluginInfo) -> RvStatus,
}

/// One call into the Plugin's entry point.
#[repr(C)]
#[derive(Debug)]
struct RvCall {
    /// `size_of::<RvCall>()`.
    size: u32,
    /// The operation.
    verb: RvVerb,
    /// What the `RvHost` table reports on every call.
    host: *const RvHost,
    /// The Plugin instance: NULL going into `RV_PLUGIN_LOAD`, which may
    /// set it.
    plugin: *mut c_void,
    /// The per-run Pack instance; NULL until the Pack verbs exist.
    pack_run: *mut c_void,
    /// The verb's own arguments: an `RvLoadArgs` for `RV_PLUGIN_LOAD`, and
    /// NULL for `RV_PLUGIN_UNLOAD`.
    args: *const c_void,
}

/// What `RV_PLUGIN_LOAD` makes and `RV_PLUGIN_UNLOAD` frees. It is not
/// zero-sized, so the allocation is real and a leak or a double free
/// shows up under Miri.
struct Instance {
    /// Ballast that gives the instance a size.
    _ballast: [u8; 16],
}

#[cfg(test)]
thread_local! {
    /// How many `Instance` values this thread has made and not yet
    /// dropped. The unit tests read it to prove that a failing load and an
    /// unload each free the instance.
    static LIVE_INSTANCES: std::cell::Cell<isize> = const { std::cell::Cell::new(0) };
}

impl Instance {
    /// Makes one instance; under test, counts it as live.
    fn new() -> Instance {
        #[cfg(test)]
        LIVE_INSTANCES.set(LIVE_INSTANCES.get() + 1);
        Instance { _ballast: [0; 16] }
    }
}

#[cfg(test)]
impl Drop for Instance {
    fn drop(&mut self) {
        LIVE_INSTANCES.set(LIVE_INSTANCES.get() - 1);
    }
}

/// A type's size as the contract's `u32`. The contract's structs are a few
/// dozen bytes, so the fallback is unreachable; it is `u32::MAX` so that a
/// wrong size fails a check instead of panicking.
fn size_of_u32<T>() -> u32 {
    u32::try_from(size_of::<T>()).unwrap_or(u32::MAX)
}

/// The fixture's Plugin entry point, speaking contract 0.2.
///
/// # Safety
/// `call` must point to a valid `RvCall` whose `host` points to a valid
/// `RvHost`, and whose `args`, for `RV_PLUGIN_LOAD`, is NULL or points to
/// an `RvLoadArgs` at least as long as its `size` says, as the C contract
/// requires.
#[unsafe(no_mangle)]
#[expect(
    private_interfaces,
    reason = "the C types stay private; only the entry point is exported"
)]
pub unsafe extern "C" fn runvil_plugin(call: *mut RvCall) -> RvStatus {
    // SAFETY: the caller guarantees `call` points to a valid `RvCall`.
    let verb = unsafe { (*call).verb };
    match verb {
        RV_PLUGIN_LOAD => {
            // SAFETY: as above, and the caller guarantees the `RvHost`.
            unsafe { load(call) }
        }
        RV_PLUGIN_UNLOAD => {
            // SAFETY: as above.
            unsafe { unload(call) }
        }
        _ => RV_ERR_UNSUPPORTED,
    }
}

/// Handles `RV_PLUGIN_LOAD`: checks the `RvHost` version, then the load
/// arguments, makes the instance, describes the Plugin once through its
/// `ctx`, and only then hands the instance to the loader.
///
/// # Safety
/// `call` points to a valid `RvCall` whose `host` and `args` are as the
/// entry point's own safety contract requires.
unsafe fn load(call: *mut RvCall) -> RvStatus {
    // SAFETY: the caller guarantees `call` is valid.
    let (host, args) = unsafe { ((*call).host, (*call).args.cast::<RvLoadArgs>()) };
    // The version check comes first and reads only `size` until the table
    // is known to be long enough, so a short table is never read past its
    // end. It also runs before `args` is read and before anything is
    // allocated, because another version's `args` has another shape.
    // SAFETY: the caller guarantees `host` points to a valid `RvHost`.
    if unsafe { (*host).size } < size_of_u32::<RvHost>() {
        return RV_ERR_UNSUPPORTED;
    }
    // SAFETY: as above, and the table is long enough to hold the version.
    let version = unsafe { ((*host).api_major, (*host).api_minor) };
    if version != (RV_API_MAJOR, RV_API_MINOR) {
        return RV_ERR_UNSUPPORTED;
    }
    if args.is_null() {
        return RV_ERR_MISUSE;
    }
    // SAFETY: the caller guarantees a non-NULL `args` points to an
    // `RvLoadArgs`, whose first field, `size`, is readable on its own.
    if unsafe { (*args).size } < size_of_u32::<RvLoadArgs>() {
        return RV_ERR_UNSUPPORTED;
    }
    // SAFETY: `size` says the loader wrote a whole `RvLoadArgs`.
    let args = unsafe { &*args };

    let instance = Box::new(Instance::new());
    let info = RvPluginInfo {
        size: size_of_u32::<RvPluginInfo>(),
        reserved: 0,
        name: RvStr {
            ptr: NAME.as_ptr().cast(),
            len: NAME.len(),
        },
        version: RvStr {
            ptr: VERSION.as_ptr().cast(),
            len: VERSION.len(),
        },
    };
    // SAFETY: `describe_plugin` and `ctx` are the load's own arguments,
    // and `info` and its strings outlive the call.
    let status = unsafe { (args.describe_plugin)(args.ctx, &raw const info) };
    if status != RV_OK {
        // `instance` drops here, so a failing load frees what it made.
        return status;
    }
    // SAFETY: the caller guarantees `call` is valid, and nothing else
    // reads it meanwhile.
    unsafe { (*call).plugin = Box::into_raw(instance).cast() };
    RV_OK
}

/// Handles `RV_PLUGIN_UNLOAD`: frees the instance `RV_PLUGIN_LOAD` set.
///
/// # Safety
/// `call` points to a valid `RvCall` whose `plugin` is NULL or the pointer
/// this entry point's `RV_PLUGIN_LOAD` set, not yet unloaded.
unsafe fn unload(call: *mut RvCall) -> RvStatus {
    // SAFETY: the caller guarantees `call` is valid.
    let plugin = unsafe { (*call).plugin };
    if plugin.is_null() {
        return RV_ERR_MISUSE;
    }
    // SAFETY: a non-NULL `plugin` is the `Box<Instance>` that
    // `RV_PLUGIN_LOAD` leaked, and the loader sends `RV_PLUGIN_UNLOAD`
    // once per load, so this is its only free.
    drop(unsafe { Box::from_raw(plugin.cast::<Instance>()) });
    RV_OK
}
