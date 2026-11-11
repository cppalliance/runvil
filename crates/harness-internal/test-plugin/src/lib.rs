//! `harness-test-plugin` - a library Plugin for the loader's end-to-end
//! tests: a dynamic library that exports `runvil_plugin` and follows the C
//! contract. It inserts one Pack, `fixture/test`, whose three tools show a
//! call that ends in one step, one that takes three, and one that never
//! ends.
//!
//! ## Invariants
//!
//! - No `[dependencies]` at all, so a library Plugin needs nothing from
//!   Runvil. Only the end-to-end tests in `tests/it` name other crates, as
//!   `[dev-dependencies]`. The C types below are a private copy of the
//!   contract's, not an import.
//! - Only `runvil_plugin` and the five counters `harness_test_plugin_loads`,
//!   `harness_test_plugin_unloads`, `harness_test_plugin_live`,
//!   `harness_test_plugin_packs`, and `harness_test_plugin_calls` are
//!   public. The mirrored C types stay private, and unit tests pin them to
//!   the same layout numbers as `runvil-plugin-sys`.
//! - The counters are process-global, so each copy of the library that is
//!   opened counts on its own, from zero.
//! - The entry point never unwinds: it has no `unwrap`, `expect`, or
//!   indexing that can panic.
//! - `RV_PLUGIN_LOAD` loads only when the `RvHost` table reports exactly
//!   version 0.3, and checks that before it reads `args`.
//! - `RV_PLUGIN_LOAD` describes the Plugin exactly once, as `test-plugin`
//!   at `0.1.0`, inserts its one Pack, and sets the instance pointer only
//!   when both succeed. A failing load frees what it made, and so does a
//!   failing `RV_PACK_NEW`.
//! - A tool call's state is freed by the step that ends the call or by
//!   `RV_TOOL_CANCEL`, never by both.
#![expect(unsafe_code, reason = "exports a C entry point")]

use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicU32, Ordering};

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
/// The tool call needs another step.
const RV_PENDING: RvStatus = 7;

/// The operation an `RvCall` asks for.
type RvVerb = u32;
/// Make the Plugin instance, name the Plugin, and insert its Packs.
const RV_PLUGIN_LOAD: RvVerb = 1;
/// Free the Plugin instance.
const RV_PLUGIN_UNLOAD: RvVerb = 2;
/// Make one run's instance of a Pack and list its tools.
const RV_PACK_NEW: RvVerb = 3;
/// Free one run's instance of a Pack.
const RV_PACK_DELETE: RvVerb = 4;
/// Run one step of a tool call.
const RV_TOOL_CALL: RvVerb = 5;
/// Free the state of a tool call the Host has abandoned.
const RV_TOOL_CANCEL: RvVerb = 6;

/// How a tool call ended.
type RvResultKind = u32;
/// The call succeeded, and the Plugin vouches for its text.
const RV_RESULT_TRUSTED: RvResultKind = 1;
/// The call failed for a reason the other kinds do not name.
const RV_RESULT_OTHER: RvResultKind = 5;

/// The contract's major version, which the `RvHost` table must report.
const RV_API_MAJOR: u32 = 0;
/// The contract's minor version, which the `RvHost` table must report.
const RV_API_MINOR: u32 = 3;

/// The name this Plugin gives `describe_plugin`.
const NAME: &str = "test-plugin";
/// The version this Plugin gives `describe_plugin`.
const VERSION: &str = "0.1.0";
/// The name of the one Pack this Plugin inserts.
const PACK: &str = "fixture/test";
/// What the Pack says it offers.
const PACK_DESCRIPTION: &str = "The loader's test fixture";
/// The parameters of each tool: an object with no properties.
const NO_PARAMETERS: &str = r#"{"type":"object","properties":{}}"#;
/// The tool that ends in one step, with `hello ` and its arguments.
const HELLO: &str = "hello";
/// The tool that ends in its third step, with `3 steps`.
const STEPS: &str = "steps";
/// The tool that never ends.
const NEVER: &str = "never";

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

/// What a Plugin tells `insert_pack` about one of its Packs.
#[repr(C)]
#[derive(Debug)]
struct RvPackInfo {
    /// `size_of::<RvPackInfo>()`.
    size: u32,
    /// Zero.
    reserved: u32,
    /// The Pack's name, a vendor and a Pack id joined by `/`.
    name: RvStr,
    /// One line saying what the Pack offers.
    description: RvStr,
}

/// What a Pack instance tells `add_tool` about one of its tools.
#[repr(C)]
#[derive(Debug)]
struct RvToolInfo {
    /// `size_of::<RvToolInfo>()`.
    size: u32,
    /// Zero.
    reserved: u32,
    /// The tool's name within its Pack.
    name: RvStr,
    /// The name a model calls the tool by, or empty.
    wire_name: RvStr,
    /// What the tool does.
    description: RvStr,
    /// The JSON text of the tool's parameter schema.
    parameters: RvStr,
}

/// How a tool call ended, which a step tells `finish`.
#[repr(C)]
#[derive(Debug)]
struct RvToolResult {
    /// `size_of::<RvToolResult>()`.
    size: u32,
    /// One of the `RV_RESULT_` constants.
    kind: RvResultKind,
    /// The call's output, or a message saying what went wrong.
    text: RvStr,
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
    /// Inserts one Pack.
    insert_pack: unsafe extern "C" fn(ctx: *mut c_void, info: *const RvPackInfo) -> RvStatus,
}

/// The arguments of `RV_PACK_NEW`: the Pack to make an instance of, and
/// the functions the Plugin may call while it makes one.
#[repr(C)]
#[derive(Debug)]
struct RvPackNewArgs {
    /// `size_of::<RvPackNewArgs>()`.
    size: u32,
    /// Zero.
    reserved: u32,
    /// The loader's context for this call.
    ctx: *mut c_void,
    /// The name of the Pack, as the Plugin gave it to `insert_pack`.
    pack: RvStr,
    /// Declines to make the instance. This Plugin never calls it.
    refuse: unsafe extern "C" fn(ctx: *mut c_void, reason: RvStr) -> RvStatus,
    /// Adds one tool to the instance.
    add_tool: unsafe extern "C" fn(ctx: *mut c_void, info: *const RvToolInfo) -> RvStatus,
}

/// The arguments of `RV_TOOL_CALL`: the tool, its arguments, and the
/// function that ends the call.
#[repr(C)]
#[derive(Debug)]
struct RvToolCallArgs {
    /// `size_of::<RvToolCallArgs>()`.
    size: u32,
    /// Zero.
    reserved: u32,
    /// The loader's context for this step.
    ctx: *mut c_void,
    /// The tool's name, as the instance gave it to `add_tool`.
    tool: RvStr,
    /// The JSON text of the call's arguments.
    arguments: RvStr,
    /// Ends the call with its result.
    finish: unsafe extern "C" fn(ctx: *mut c_void, result: *const RvToolResult) -> RvStatus,
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
    /// The Plugin instance: NULL going into `RV_PLUGIN_LOAD`, which sets
    /// it.
    plugin: *mut c_void,
    /// One run's instance of a Pack: NULL going into `RV_PACK_NEW`, which
    /// sets it.
    pack_run: *mut c_void,
    /// The verb's own arguments, or NULL for a verb that has none.
    args: *const c_void,
    /// The state of one tool call: NULL going into its first step.
    tool_call: *mut c_void,
}

/// What `RV_PLUGIN_LOAD` makes and `RV_PLUGIN_UNLOAD` frees. It is not
/// zero-sized, so the allocation is real and a leak or a double free
/// shows up under Miri.
struct Instance {
    /// Ballast that gives the instance a size.
    _ballast: [u8; 16],
}

/// What `RV_PACK_NEW` makes and `RV_PACK_DELETE` frees.
struct PackRun {
    /// Ballast that gives the instance a size.
    _ballast: [u8; 16],
}

/// What a tool call that takes more than one step keeps between them.
struct CallState {
    /// How many steps the call has run.
    steps: u32,
}

#[cfg(test)]
thread_local! {
    /// How many `Instance` values this thread has made and not yet
    /// dropped. The unit tests read it to prove that a failing load and an
    /// unload each free the instance.
    static LIVE_INSTANCES: std::cell::Cell<isize> = const { std::cell::Cell::new(0) };
}

/// Successful `RV_PLUGIN_LOAD` calls since this copy of the library was
/// opened.
static LOADS: AtomicU32 = AtomicU32::new(0);
/// `RV_PLUGIN_UNLOAD` calls that freed an instance since this copy of the
/// library was opened.
static UNLOADS: AtomicU32 = AtomicU32::new(0);
/// `Instance` values made and not yet dropped, across every thread.
static LIVE: AtomicU32 = AtomicU32::new(0);
/// `PackRun` values made and not yet dropped, across every thread.
static PACKS: AtomicU32 = AtomicU32::new(0);
/// `CallState` values made and not yet dropped, across every thread.
static CALLS: AtomicU32 = AtomicU32::new(0);

impl Instance {
    /// Makes one instance and counts it as live.
    fn new() -> Instance {
        LIVE.fetch_add(1, Ordering::Relaxed);
        #[cfg(test)]
        LIVE_INSTANCES.set(LIVE_INSTANCES.get() + 1);
        Instance { _ballast: [0; 16] }
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        LIVE.fetch_sub(1, Ordering::Relaxed);
        #[cfg(test)]
        LIVE_INSTANCES.set(LIVE_INSTANCES.get() - 1);
    }
}

impl PackRun {
    /// Makes one Pack instance and counts it as live.
    fn new() -> PackRun {
        PACKS.fetch_add(1, Ordering::Relaxed);
        PackRun { _ballast: [0; 16] }
    }
}

impl Drop for PackRun {
    fn drop(&mut self) {
        PACKS.fetch_sub(1, Ordering::Relaxed);
    }
}

impl CallState {
    /// Makes the state of one call and counts it as live.
    fn new() -> CallState {
        CALLS.fetch_add(1, Ordering::Relaxed);
        CallState { steps: 0 }
    }
}

impl Drop for CallState {
    fn drop(&mut self) {
        CALLS.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Successful `RV_PLUGIN_LOAD` calls since this copy of the library was
/// opened.
#[unsafe(no_mangle)]
pub extern "C" fn harness_test_plugin_loads() -> u32 {
    LOADS.load(Ordering::Relaxed)
}

/// `RV_PLUGIN_UNLOAD` calls that freed an instance since this copy of the
/// library was opened.
#[unsafe(no_mangle)]
pub extern "C" fn harness_test_plugin_unloads() -> u32 {
    UNLOADS.load(Ordering::Relaxed)
}

/// Instances made and not yet dropped: incremented when an instance is
/// made, decremented in its `Drop`.
#[unsafe(no_mangle)]
pub extern "C" fn harness_test_plugin_live() -> u32 {
    LIVE.load(Ordering::Relaxed)
}

/// Pack instances made by `RV_PACK_NEW` and not yet freed by
/// `RV_PACK_DELETE`.
#[unsafe(no_mangle)]
pub extern "C" fn harness_test_plugin_packs() -> u32 {
    PACKS.load(Ordering::Relaxed)
}

/// Tool calls that hold state between steps: made by a call's first step,
/// freed by its last step or by `RV_TOOL_CANCEL`.
#[unsafe(no_mangle)]
pub extern "C" fn harness_test_plugin_calls() -> u32 {
    CALLS.load(Ordering::Relaxed)
}

/// A type's size as the contract's `u32`. The contract's structs are a few
/// dozen bytes, so the fallback is unreachable; it is `u32::MAX` so that a
/// wrong size fails a check instead of panicking.
fn size_of_u32<T>() -> u32 {
    u32::try_from(size_of::<T>()).unwrap_or(u32::MAX)
}

/// `text` as the contract's pointer and length.
fn rv_str(text: &str) -> RvStr {
    RvStr {
        ptr: text.as_ptr().cast(),
        len: text.len(),
    }
}

/// The text `s` holds, or the empty string when it is not UTF-8.
///
/// # Safety
/// When `s.len` is nonzero, `s.ptr` must be valid for reads of `s.len`
/// bytes for the lifetime `'a`.
unsafe fn text<'a>(s: RvStr) -> &'a str {
    if s.len == 0 || s.ptr.is_null() {
        return "";
    }
    // SAFETY: `s.ptr` is not NULL, and the caller guarantees it is valid
    // for `s.len` bytes.
    let bytes = unsafe { std::slice::from_raw_parts(s.ptr.cast::<u8>(), s.len) };
    std::str::from_utf8(bytes).unwrap_or_default()
}

/// The arguments struct of the verb `call` names: `RV_ERR_MISUSE` when the
/// call has none, and `RV_ERR_UNSUPPORTED` when it is shorter than `T`.
///
/// # Safety
/// `call` must point to a valid `RvCall` whose `args` is NULL or points to
/// one of the contract's arguments structs, each of which begins with its
/// `size`, as long as that `size` says, for the lifetime `'a`.
unsafe fn args<'a, T>(call: *mut RvCall) -> Result<&'a T, RvStatus> {
    // SAFETY: the caller guarantees `call` is valid.
    let args = unsafe { (*call).args }.cast::<T>();
    if args.is_null() {
        return Err(RV_ERR_MISUSE);
    }
    // SAFETY: a non-NULL `args` begins with its `size`, which is readable
    // on its own.
    if unsafe { args.cast::<u32>().read() } < size_of_u32::<T>() {
        return Err(RV_ERR_UNSUPPORTED);
    }
    // SAFETY: `size` says the loader wrote a whole `T`.
    Ok(unsafe { &*args })
}

/// The fixture's Plugin entry point, speaking contract 0.3.
///
/// # Safety
/// `call` must point to a valid `RvCall` made as the C contract requires:
/// its `host` points to a valid `RvHost`, its `args` is NULL or points to
/// the arguments struct of its verb, at least as long as its `size` says,
/// and its Plugin pointers are the ones this entry point set.
#[unsafe(no_mangle)]
#[expect(
    private_interfaces,
    reason = "the C types stay private; only the entry point is exported"
)]
pub unsafe extern "C" fn runvil_plugin(call: *mut RvCall) -> RvStatus {
    // SAFETY: the caller guarantees `call` points to a valid `RvCall`.
    let verb = unsafe { (*call).verb };
    // SAFETY: in each arm, as above, and the caller vouches for the
    // pointers the verb reads.
    unsafe {
        match verb {
            RV_PLUGIN_LOAD => load(call),
            RV_PLUGIN_UNLOAD => unload(call),
            RV_PACK_NEW => pack_new(call),
            RV_PACK_DELETE => pack_delete(call),
            RV_TOOL_CALL => tool_call(call),
            RV_TOOL_CANCEL => tool_cancel(call),
            _ => RV_ERR_UNSUPPORTED,
        }
    }
}

/// Handles `RV_PLUGIN_LOAD`: checks the `RvHost` version, then the load
/// arguments, makes the instance, describes the Plugin once and inserts
/// its Pack through the load's `ctx`, and only then hands the instance to
/// the loader.
///
/// # Safety
/// `call` points to a valid `RvCall` whose `host` and `args` are as the
/// entry point's own safety contract requires.
unsafe fn load(call: *mut RvCall) -> RvStatus {
    // SAFETY: the caller guarantees `call` is valid.
    let host = unsafe { (*call).host };
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
    // SAFETY: for this verb `args` is NULL or an `RvLoadArgs`.
    let args = match unsafe { args::<RvLoadArgs>(call) } {
        Ok(args) => args,
        Err(status) => return status,
    };

    let instance = Box::new(Instance::new());
    let info = RvPluginInfo {
        size: size_of_u32::<RvPluginInfo>(),
        reserved: 0,
        name: rv_str(NAME),
        version: rv_str(VERSION),
    };
    // SAFETY: `describe_plugin` and `ctx` are the load's own arguments,
    // and `info` and its strings outlive the call.
    let status = unsafe { (args.describe_plugin)(args.ctx, &raw const info) };
    if status != RV_OK {
        // `instance` drops here, so a failing load frees what it made.
        return status;
    }
    let pack = RvPackInfo {
        size: size_of_u32::<RvPackInfo>(),
        reserved: 0,
        name: rv_str(PACK),
        description: rv_str(PACK_DESCRIPTION),
    };
    // SAFETY: as above, for `insert_pack`.
    let status = unsafe { (args.insert_pack)(args.ctx, &raw const pack) };
    if status != RV_OK {
        return status;
    }
    // SAFETY: the caller guarantees `call` is valid, and nothing else
    // reads it meanwhile.
    unsafe { (*call).plugin = Box::into_raw(instance).cast() };
    LOADS.fetch_add(1, Ordering::Relaxed);
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
    UNLOADS.fetch_add(1, Ordering::Relaxed);
    RV_OK
}

/// Handles `RV_PACK_NEW`: makes the Pack instance, adds its three tools
/// through the call's `ctx`, and only then hands the instance to the
/// loader.
///
/// # Safety
/// `call` points to a valid `RvCall` whose `args` is NULL or an
/// `RvPackNewArgs` as long as its `size` says.
unsafe fn pack_new(call: *mut RvCall) -> RvStatus {
    // SAFETY: for this verb `args` is NULL or an `RvPackNewArgs`.
    let args = match unsafe { args::<RvPackNewArgs>(call) } {
        Ok(args) => args,
        Err(status) => return status,
    };
    // SAFETY: the loader keeps the Pack's name valid for the call.
    if unsafe { text(args.pack) } != PACK {
        return RV_ERR_MISUSE;
    }
    let pack_run = Box::new(PackRun::new());
    for (name, description) in [
        (HELLO, "Answers hello and its arguments in one step."),
        (STEPS, "Answers in its third step."),
        (NEVER, "Never answers."),
    ] {
        let info = RvToolInfo {
            size: size_of_u32::<RvToolInfo>(),
            reserved: 0,
            name: rv_str(name),
            wire_name: rv_str(""),
            description: rv_str(description),
            parameters: rv_str(NO_PARAMETERS),
        };
        // SAFETY: `add_tool` and `ctx` are the call's own arguments, and
        // `info` and its strings outlive the call.
        let status = unsafe { (args.add_tool)(args.ctx, &raw const info) };
        if status != RV_OK {
            // `pack_run` drops here, so a failing call frees what it made.
            return status;
        }
    }
    // SAFETY: the caller guarantees `call` is valid, and nothing else
    // reads it meanwhile.
    unsafe { (*call).pack_run = Box::into_raw(pack_run).cast() };
    RV_OK
}

/// Handles `RV_PACK_DELETE`: frees the instance `RV_PACK_NEW` set.
///
/// # Safety
/// `call` points to a valid `RvCall` whose `pack_run` is NULL or the
/// pointer this entry point's `RV_PACK_NEW` set, not yet deleted.
unsafe fn pack_delete(call: *mut RvCall) -> RvStatus {
    // SAFETY: the caller guarantees `call` is valid.
    let pack_run = unsafe { (*call).pack_run };
    if pack_run.is_null() {
        return RV_ERR_MISUSE;
    }
    // SAFETY: a non-NULL `pack_run` is the `Box<PackRun>` that
    // `RV_PACK_NEW` leaked, and the loader deletes each instance once.
    drop(unsafe { Box::from_raw(pack_run.cast::<PackRun>()) });
    RV_OK
}

/// Handles `RV_TOOL_CALL`: runs one step of the tool the arguments name.
/// The state of a call is taken out of `call->tool_call` first, so every
/// return other than `RV_PENDING` leaves none behind.
///
/// # Safety
/// `call` points to a valid `RvCall` whose `tool_call` is NULL or the
/// state the call's last step left, and whose `args` is NULL or an
/// `RvToolCallArgs` as long as its `size` says.
unsafe fn tool_call(call: *mut RvCall) -> RvStatus {
    // SAFETY: the caller guarantees `call` is valid, and a non-NULL
    // `tool_call` is the `Box<CallState>` the last step leaked.
    let state = unsafe {
        let state = (*call).tool_call.cast::<CallState>();
        (*call).tool_call = std::ptr::null_mut();
        (!state.is_null()).then(|| Box::from_raw(state))
    };
    // SAFETY: for this verb `args` is NULL or an `RvToolCallArgs`.
    let args = match unsafe { args::<RvToolCallArgs>(call) } {
        Ok(args) => args,
        Err(status) => return status,
    };
    // SAFETY: the loader keeps both strings valid for the step.
    let (tool, arguments) = unsafe { (text(args.tool), text(args.arguments)) };
    let mut state = state.unwrap_or_else(|| Box::new(CallState::new()));
    state.steps += 1;
    let (kind, answer) = match tool {
        HELLO => (RV_RESULT_TRUSTED, format!("hello {arguments}")),
        STEPS if state.steps == 3 => (RV_RESULT_TRUSTED, "3 steps".to_owned()),
        STEPS | NEVER => {
            // SAFETY: as above.
            unsafe { (*call).tool_call = Box::into_raw(state).cast() };
            return RV_PENDING;
        }
        other => (
            RV_RESULT_OTHER,
            format!("test-plugin has no tool named {other}"),
        ),
    };
    // The call ends here, so its state is freed before the step returns.
    drop(state);
    let result = RvToolResult {
        size: size_of_u32::<RvToolResult>(),
        kind,
        text: rv_str(&answer),
    };
    // SAFETY: `finish` and `ctx` are the step's own arguments, and
    // `result` and its text outlive the call.
    unsafe { (args.finish)(args.ctx, &raw const result) }
}

/// Handles `RV_TOOL_CANCEL`: frees the state of an abandoned call.
///
/// # Safety
/// `call` points to a valid `RvCall` whose `tool_call` is NULL or the
/// state the call's last step left.
unsafe fn tool_cancel(call: *mut RvCall) -> RvStatus {
    // SAFETY: the caller guarantees `call` is valid.
    let state = unsafe { (*call).tool_call }.cast::<CallState>();
    if !state.is_null() {
        // SAFETY: a non-NULL `tool_call` is the `Box<CallState>` the last
        // step leaked, and the loader cancels a call once.
        drop(unsafe { Box::from_raw(state) });
    }
    RV_OK
}
