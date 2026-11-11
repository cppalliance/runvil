//! Fake entry points for the unit tests: plain `extern "C" fn`s written
//! against `runvil-plugin-sys`, and the helpers that call the Host's
//! functions the way a Plugin does. Each test defines its own fake with
//! `fake!` or `pack_fake!`, so its statics belong to it alone.

use std::ffi::c_void;
use std::future::Future;
use std::pin::pin;
use std::ptr;
use std::slice;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use runvil_plugin::testing::TestCall;
use runvil_plugin::{Error, Pack, PackRegistry, ToolId, ToolOutput};
pub(crate) use runvil_plugin_sys::{
    PluginEntry, RV_ERR_MISUSE, RV_ERR_REFUSED, RV_ERR_UNSUPPORTED, RV_FILE_ERROR_IS_A_DIRECTORY,
    RV_FILE_ERROR_NOT_A_DIRECTORY, RV_FILE_ERROR_NOT_FOUND, RV_FILE_ERROR_OTHER,
    RV_FILE_TYPE_DIRECTORY, RV_FILE_TYPE_FILE, RV_OK, RV_PACK_DELETE, RV_PACK_NEW, RV_PENDING,
    RV_PLUGIN_LOAD, RV_PLUGIN_UNLOAD, RV_RESULT_BACKEND, RV_RESULT_INVALID_ARGUMENTS,
    RV_RESULT_OTHER, RV_RESULT_TRANSPORT, RV_RESULT_TRUSTED, RV_RESULT_UNTRUSTED, RV_TOOL_CALL,
    RV_TOOL_CANCEL, RvBytes, RvCall, RvDirEntries, RvFileError, RvHost, RvLoadArgs, RvPackInfo,
    RvPackNewArgs, RvPluginInfo, RvResultKind, RvStat, RvStatus, RvStr, RvToolCallArgs, RvToolInfo,
    RvToolResult, RvVerb,
};
use serde_json::Value;

use crate::host::{self, HOST};
use crate::plugin::{Plugin, insert_plugin};

/// A status no verb returns, for a fake that fails with its own code.
pub(crate) const ODD_STATUS: RvStatus = 77;

/// The name of the Pack a `pack_fake!` inserts.
pub(crate) const FAKE_PACK: &str = "acme/fake";

/// The parameters of a fake's tools: an object with no properties.
pub(crate) const NO_PARAMETERS: &str = r#"{"type":"object","properties":{}}"#;

/// Defines `mod $name` holding a fake entry point and its statics. Its
/// `RV_PLUGIN_LOAD` stores the `ctx` it was handed in `LOAD_CTX`, then runs
/// `$load`, which sees the call as `$call` and returns the load's status.
/// Its `RV_PLUGIN_UNLOAD` counts itself in `UNLOADS`, stores the instance
/// pointer it was sent in `UNLOADED`, records whether the call carried the
/// contract's fields in `UNLOAD_WELL_FORMED`, and stores the status of a
/// `describe_plugin` it then tries through `LOAD_CTX` in `UNLOAD_STATUS`.
/// The second form adds the items a test needs the load body to share with
/// it, such as a `pub(super) static`.
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
            /// The `ctx` its last RV_PLUGIN_LOAD was handed, which a test
            /// reads to call a load function after that load returned.
            pub(super) static LOAD_CTX: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

            $($items)*

            pub(super) extern "C" fn entry($call: *mut RvCall) -> RvStatus {
                match verb($call) {
                    RV_PLUGIN_LOAD => {
                        LOAD_CTX.store(ctx($call), Ordering::SeqCst);
                        $load
                    }
                    RV_PLUGIN_UNLOAD => {
                        UNLOADS.fetch_add(1, Ordering::SeqCst);
                        UNLOADED.store(plugin($call), Ordering::SeqCst);
                        UNLOAD_WELL_FORMED
                            .store(contract_holds($call, RV_PLUGIN_UNLOAD), Ordering::SeqCst);
                        let status =
                            describe_with(LOAD_CTX.load(Ordering::SeqCst), "acme-url", "1.0.0");
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

/// Defines `mod $name` holding a fake entry point whose Plugin, named
/// `fake`, inserts the one Pack [`FAKE_PACK`] and sets the instance
/// [`PLUGIN_TAG`]. Its `RV_PACK_NEW` runs `$new` and its `RV_TOOL_CALL`
/// runs `$step`, each seeing the call under the name it gives and
/// returning the call's status. It counts its `RV_PLUGIN_UNLOAD` calls in
/// `UNLOADS`, its `RV_PACK_DELETE` calls in `DELETES`, and its
/// `RV_TOOL_CANCEL` calls in `CANCELS`, keeps the pointer the last delete
/// and the last cancel carried in `DELETED` and `CANCELED`, and keeps in
/// `ORDER` the verbs of those three kinds, in the order they came. The
/// items in the braces are what a test needs the bodies to share with it.
macro_rules! pack_fake {
    (
        $name:ident,
        { $($items:item)* },
        new: |$new_call:ident| $new:expr,
        step: |$step_call:ident| $step:expr
    ) => {
        mod $name {
            use std::ffi::c_void;
            use std::ptr;
            use std::sync::Mutex;
            use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

            use $crate::fakes::*;

            /// How many RV_PLUGIN_UNLOAD calls this fake got.
            pub(super) static UNLOADS: AtomicUsize = AtomicUsize::new(0);
            /// How many RV_PACK_DELETE calls this fake got.
            pub(super) static DELETES: AtomicUsize = AtomicUsize::new(0);
            /// The `pack_run` its last RV_PACK_DELETE carried.
            pub(super) static DELETED: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
            /// How many RV_TOOL_CANCEL calls this fake got.
            pub(super) static CANCELS: AtomicUsize = AtomicUsize::new(0);
            /// The `tool_call` its last RV_TOOL_CANCEL carried.
            pub(super) static CANCELED: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
            /// The cancels, deletes, and unloads it got, in order.
            pub(super) static ORDER: Mutex<Vec<RvVerb>> = Mutex::new(Vec::new());

            $($items)*

            /// Notes that `verb` came.
            fn note(verb: RvVerb) {
                if let Ok(mut order) = ORDER.lock() {
                    order.push(verb);
                }
            }

            pub(super) extern "C" fn entry(call: *mut RvCall) -> RvStatus {
                match verb(call) {
                    RV_PLUGIN_LOAD => {
                        set_plugin(call, instance(PLUGIN_TAG));
                        describe(call, "fake", "1.0.0");
                        insert_pack(call, FAKE_PACK, "A fake Pack")
                    }
                    RV_PLUGIN_UNLOAD => {
                        UNLOADS.fetch_add(1, Ordering::SeqCst);
                        note(RV_PLUGIN_UNLOAD);
                        RV_OK
                    }
                    RV_PACK_NEW => {
                        let $new_call = call;
                        $new
                    }
                    RV_PACK_DELETE => {
                        DELETES.fetch_add(1, Ordering::SeqCst);
                        DELETED.store(pack_run(call), Ordering::SeqCst);
                        note(RV_PACK_DELETE);
                        RV_OK
                    }
                    RV_TOOL_CALL => {
                        let $step_call = call;
                        $step
                    }
                    RV_TOOL_CANCEL => {
                        CANCELS.fetch_add(1, Ordering::SeqCst);
                        CANCELED.store(tool_call(call), Ordering::SeqCst);
                        note(RV_TOOL_CANCEL);
                        RV_OK
                    }
                    _ => RV_ERR_UNSUPPORTED,
                }
            }
        }
    };
}
pub(crate) use pack_fake;

/// The instance a `pack_fake!` Plugin sets.
pub(crate) const PLUGIN_TAG: usize = 0x100;

/// The `pack_run` a well-behaved fake `RV_PACK_NEW` sets.
pub(crate) const PACK_TAG: usize = 0x200;

/// Inserts the linked Plugin `entry` into a fresh Pack registry.
pub(crate) fn insert(entry: PluginEntry) -> Result<Arc<Plugin>, Error> {
    insert_plugin(&mut PackRegistry::new(), entry)
}

/// Inserts `entry`, which must load.
pub(crate) fn loaded(entry: PluginEntry) -> Arc<Plugin> {
    insert(entry).expect("the Plugin loads")
}

/// Inserts `entry`, which must fail to load.
pub(crate) fn failure(entry: PluginEntry) -> Error {
    insert(entry).expect_err("the Plugin fails to load")
}

/// The `Reason` fact of a load failure.
pub(crate) fn reason(failure: &Error) -> Option<&str> {
    failure
        .facts
        .get(&runvil_plugin::ErrorFact::Reason)?
        .as_str()
}

/// Inserts the `pack_fake!` Plugin `entry` into a fresh registry, drops
/// the registry's hold on the Plugin's `Arc`, and asks the factory of
/// [`FAKE_PACK`] for one run's instance.
pub(crate) fn new_pack(entry: PluginEntry) -> Result<Arc<dyn Pack>, Error> {
    let mut packs = PackRegistry::new();
    drop(insert_plugin(&mut packs, entry).expect("the Plugin loads"));
    let (_info, id, factory) = packs
        .into_entries()
        .find(|(info, _, _)| info.fqn == FAKE_PACK)
        .expect("the Plugin inserted its Pack");
    factory.expect("the Pack is usable").new_pack(&id)
}

/// One run's instance of the Pack the `pack_fake!` Plugin `entry` inserts,
/// which must be made.
pub(crate) fn made(entry: PluginEntry) -> Arc<dyn Pack> {
    new_pack(entry).expect("the Pack instance is made")
}

/// A call of the fake Pack's tool `name`, for a test to lend its context.
pub(crate) fn test_call(name: &str) -> TestCall {
    TestCall::new(ToolId::parse(&format!("fake/{name}")).expect("a two-segment tool id parses"))
}

/// Drives `future` to its output, polling again after each `Pending`.
pub(crate) fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
            return output;
        }
    }
}

/// Runs one call of the tool `name` on `pack` with `args` to its end.
pub(crate) fn call_tool(pack: &dyn Pack, name: &str, args: Value) -> Result<ToolOutput, Error> {
    let call = test_call(name);
    block_on(pack.call(call.context(), args))
}

/// `size_of::<T>()` as a contract struct's `u32` size field.
pub(crate) fn size_u32<T>() -> u32 {
    u32::try_from(size_of::<T>()).unwrap_or(0)
}

/// Whether `call` carries the fields the contract gives a call that sends
/// `verb` in version 0.4, and its table says the contract's version. Each
/// verb carries the Plugin's pointers the contract names for it and no
/// others, and its own arguments struct, with a `ctx`, or no arguments.
pub(crate) fn contract_holds(call: *mut RvCall, verb: RvVerb) -> bool {
    // SAFETY: a fake runs only inside an entry point call, where `call` is
    // the loader's live `RvCall` and its table is the loader's static.
    let (call, table) = unsafe { (&*call, &*(*call).host) };
    let has_plugin = verb != RV_PLUGIN_LOAD;
    let has_pack = matches!(verb, RV_PACK_DELETE | RV_TOOL_CALL | RV_TOOL_CANCEL);
    // The first field of each arguments struct is its size, then a zero
    // `reserved`, then the `ctx`, as `RvLoadArgs` lays them out.
    let args_size = match verb {
        RV_PLUGIN_LOAD => Some(size_u32::<RvLoadArgs>()),
        RV_PACK_NEW => Some(size_u32::<RvPackNewArgs>()),
        RV_TOOL_CALL => Some(size_u32::<RvToolCallArgs>()),
        _ => None,
    };
    let args_hold = match args_size {
        Some(size) if !call.args.is_null() => {
            // SAFETY: each arguments struct begins with the three fields
            // of an `RvLoadArgs` this reads, and is live for the call.
            let args = unsafe { &*call.args.cast::<RvLoadArgs>() };
            args.size == size && args.reserved == 0 && !args.ctx.is_null()
        }
        Some(_) => false,
        None => call.args.is_null(),
    };
    call.size == size_u32::<RvCall>()
        && call.verb == verb
        && ptr::eq(call.host, &raw const HOST)
        && call.plugin.is_null() != has_plugin
        && (has_pack || call.pack_run.is_null())
        && (matches!(verb, RV_TOOL_CALL | RV_TOOL_CANCEL) || call.tool_call.is_null())
        && args_hold
        && table.size == size_u32::<RvHost>()
        && table.api_major == 0
        && table.api_minor == 4
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

/// The Pack instance pointer a call carries.
pub(crate) fn pack_run(call: *mut RvCall) -> *mut c_void {
    // SAFETY: as in `contract_holds`.
    unsafe { (*call).pack_run }
}

/// Sets the Pack instance pointer, as RV_PACK_NEW may.
pub(crate) fn set_pack_run(call: *mut RvCall, pack_run: *mut c_void) {
    // SAFETY: as in `set_plugin`.
    unsafe { (*call).pack_run = pack_run };
}

/// The tool call state a call carries.
pub(crate) fn tool_call(call: *mut RvCall) -> *mut c_void {
    // SAFETY: as in `contract_holds`.
    unsafe { (*call).tool_call }
}

/// Sets the tool call state, as a step may.
pub(crate) fn set_tool_call(call: *mut RvCall, state: *mut c_void) {
    // SAFETY: as in `set_plugin`.
    unsafe { (*call).tool_call = state };
}

/// An address that is never null, which a fake hands out as its instance.
pub(crate) fn instance(tag: usize) -> *mut c_void {
    ptr::without_provenance_mut(tag)
}

/// The `RvLoadArgs` a load call carries.
fn args<'call>(call: *mut RvCall) -> &'call RvLoadArgs {
    // SAFETY: as in `contract_holds`, and a load call's `args` is the
    // loader's `RvLoadArgs`, live for the whole call, which is as long as
    // a fake holds this reference.
    unsafe { &*(*call).args.cast::<RvLoadArgs>() }
}

/// The `RvPackNewArgs` an RV_PACK_NEW call carries.
pub(crate) fn pack_args<'call>(call: *mut RvCall) -> &'call RvPackNewArgs {
    // SAFETY: as in `args`, for the verb's own arguments struct.
    unsafe { &*(*call).args.cast::<RvPackNewArgs>() }
}

/// The `RvToolCallArgs` an RV_TOOL_CALL step carries.
pub(crate) fn step_args<'call>(call: *mut RvCall) -> &'call RvToolCallArgs {
    // SAFETY: as in `args`, for the verb's own arguments struct.
    unsafe { &*(*call).args.cast::<RvToolCallArgs>() }
}

/// The `ctx` a load call's `RvLoadArgs` carries.
pub(crate) fn ctx(call: *mut RvCall) -> *mut c_void {
    args(call).ctx
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

/// The text a string from the loader holds.
pub(crate) fn text(s: RvStr) -> String {
    if s.len == 0 {
        return String::new();
    }
    // SAFETY: the loader hands a fake strings that are valid for their
    // length during the call.
    let bytes = unsafe { slice::from_raw_parts(s.ptr.cast::<u8>(), s.len) };
    String::from_utf8_lossy(bytes).into_owned()
}

/// A well-formed `RvPluginInfo`.
pub(crate) fn info(name: RvStr, version: RvStr) -> RvPluginInfo {
    RvPluginInfo {
        size: size_u32::<RvPluginInfo>(),
        reserved: 0,
        name,
        version,
    }
}

/// Calls the call's `describe_plugin` with its own `ctx` and `info`.
pub(crate) fn describe_info(call: *mut RvCall, info: &RvPluginInfo) -> RvStatus {
    describe_as(call, ctx(call), info)
}

/// Calls the `describe_plugin` of `args_from`'s `RvLoadArgs`, handing it
/// `as_ctx`.
pub(crate) fn describe_as(
    args_from: *mut RvCall,
    as_ctx: *mut c_void,
    info: &RvPluginInfo,
) -> RvStatus {
    // SAFETY: `info` is a live `RvPluginInfo` whose strings outlive the
    // call, and the function reads through `as_ctx` only when it is the
    // `ctx` of the load open on this thread.
    unsafe { (args(args_from).describe_plugin)(as_ctx, info) }
}

/// Describes the Plugin with `name` and `version`.
pub(crate) fn describe(call: *mut RvCall, name: &str, version: &str) -> RvStatus {
    describe_info(call, &info(rv_str(name), rv_str(version)))
}

/// Calls the `refuse` of `args_from`'s `RvLoadArgs`, handing it `as_ctx`.
pub(crate) fn refuse_as(args_from: *mut RvCall, as_ctx: *mut c_void, reason: RvStr) -> RvStatus {
    // SAFETY: `reason` outlives the call, and the function reads through
    // `as_ctx` only when it is the `ctx` of the load open on this thread.
    unsafe { (args(args_from).refuse)(as_ctx, reason) }
}

/// Calls the call's `refuse` with its own `ctx` and `reason`.
pub(crate) fn refuse_str(call: *mut RvCall, reason: RvStr) -> RvStatus {
    refuse_as(call, ctx(call), reason)
}

/// Refuses the load with `reason`.
pub(crate) fn refuse(call: *mut RvCall, reason: &str) -> RvStatus {
    refuse_str(call, rv_str(reason))
}

/// Calls the loader's `describe_plugin` with `ctx`, as a Plugin that kept
/// the load functions and a `ctx` past its load would, from any thread.
pub(crate) fn describe_with(ctx: *mut c_void, name: &str, version: &str) -> RvStatus {
    let info = info(rv_str(name), rv_str(version));
    // SAFETY: `info` is live, and the function reads through `ctx` only
    // when it is the `ctx` of the load open on this thread.
    unsafe { host::describe_plugin(ctx, &raw const info) }
}

/// Calls the loader's `refuse` with `ctx`, as `describe_with` does.
pub(crate) fn refuse_with(ctx: *mut c_void, reason: &str) -> RvStatus {
    // SAFETY: as in `describe_with`.
    unsafe { host::refuse(ctx, rv_str(reason)) }
}

/// A well-formed `RvPackInfo`.
pub(crate) fn pack_info(name: RvStr, description: RvStr) -> RvPackInfo {
    RvPackInfo {
        size: size_u32::<RvPackInfo>(),
        reserved: 0,
        name,
        description,
    }
}

/// Calls the load call's `insert_pack` with its own `ctx` and `info`.
pub(crate) fn insert_pack_info(call: *mut RvCall, info: &RvPackInfo) -> RvStatus {
    // SAFETY: as in `describe_as`.
    unsafe { (args(call).insert_pack)(ctx(call), info) }
}

/// Inserts the Pack `name` with `description`.
pub(crate) fn insert_pack(call: *mut RvCall, name: &str, description: &str) -> RvStatus {
    insert_pack_info(call, &pack_info(rv_str(name), rv_str(description)))
}

/// A well-formed `RvToolInfo` for the tool `name`, with no wire name, a
/// one-line description, and [`NO_PARAMETERS`].
pub(crate) fn tool_info(name: &str) -> RvToolInfo {
    RvToolInfo {
        size: size_u32::<RvToolInfo>(),
        reserved: 0,
        name: rv_str(name),
        wire_name: rv_str(""),
        description: rv_str("A fake tool."),
        parameters: rv_str(NO_PARAMETERS),
    }
}

/// Calls the `add_tool` of an RV_PACK_NEW call, handing it `as_ctx`.
pub(crate) fn add_tool_as(call: *mut RvCall, as_ctx: *mut c_void, info: &RvToolInfo) -> RvStatus {
    // SAFETY: `info` is a live `RvToolInfo` whose strings outlive the
    // call, and the function reads through `as_ctx` only when it is the
    // `ctx` of the call open on this thread.
    unsafe { (pack_args(call).add_tool)(as_ctx, info) }
}

/// Calls the call's `add_tool` with its own `ctx` and `info`.
pub(crate) fn add_tool_info(call: *mut RvCall, info: &RvToolInfo) -> RvStatus {
    add_tool_as(call, pack_args(call).ctx, info)
}

/// Adds the tool `name` to the instance being made.
pub(crate) fn add_tool(call: *mut RvCall, name: &str) -> RvStatus {
    add_tool_info(call, &tool_info(name))
}

/// Declines the RV_PACK_NEW call with `reason`.
pub(crate) fn refuse_pack(call: *mut RvCall, reason: &str) -> RvStatus {
    let args = pack_args(call);
    // SAFETY: `reason` outlives the call, and `ctx` is the open call's.
    unsafe { (args.refuse)(args.ctx, rv_str(reason)) }
}

/// A well-formed `RvToolResult`.
pub(crate) fn result(kind: RvResultKind, text: RvStr) -> RvToolResult {
    RvToolResult {
        size: size_u32::<RvToolResult>(),
        kind,
        text,
    }
}

/// Calls the `finish` of an RV_TOOL_CALL step, handing it `as_ctx`.
pub(crate) fn finish_as(
    call: *mut RvCall,
    as_ctx: *mut c_void,
    result: *const RvToolResult,
) -> RvStatus {
    // SAFETY: `result` is NULL, misaligned on purpose, or a live
    // `RvToolResult` whose text outlives the call, and the function reads
    // through `as_ctx` only when it is the `ctx` of the step open on this
    // thread.
    unsafe { (step_args(call).finish)(as_ctx, result) }
}

/// Calls the step's `finish` with its own `ctx` and `result`.
pub(crate) fn finish_result(call: *mut RvCall, result: &RvToolResult) -> RvStatus {
    finish_as(call, step_args(call).ctx, result)
}

/// Finishes the call with `kind` and `text`.
pub(crate) fn finish(call: *mut RvCall, kind: RvResultKind, text: &str) -> RvStatus {
    finish_result(call, &result(kind, rv_str(text)))
}

/// An `RvStat` whose every field is 9, so a test sees whether a file
/// function wrote it.
pub(crate) fn unset_stat() -> RvStat {
    RvStat {
        file_type: 9,
        reserved: 9,
        len: 9,
    }
}

/// An `RvFileError` of kind 9 that says `untouched`, so a test sees
/// whether a file function wrote it.
pub(crate) fn unset_error() -> RvFileError {
    RvFileError {
        kind: 9,
        reserved: 9,
        text: rv_str("untouched"),
    }
}

/// An `RvBytes` with no pointer and a length of 9, so a test sees whether
/// a file function wrote it.
pub(crate) fn unset_bytes() -> RvBytes {
    RvBytes {
        ptr: ptr::null(),
        len: 9,
    }
}

/// An `RvDirEntries` with no pointer and a length of 9, so a test sees
/// whether a file function wrote it.
pub(crate) fn unset_entries() -> RvDirEntries {
    RvDirEntries {
        ptr: ptr::null(),
        len: 9,
    }
}

/// Calls the step's `file_stat` with `as_ctx`, as a Plugin would or as it
/// must not.
pub(crate) fn stat_as(
    call: *mut RvCall,
    as_ctx: *mut c_void,
    path: RvStr,
    stat: *mut RvStat,
    error: *mut RvFileError,
) -> RvStatus {
    // SAFETY: the function refuses a `ctx` that is not the open step's, a
    // path it cannot read, and a pointer that is NULL or misaligned,
    // before it reads or writes through any of them. Every other pointer
    // a test hands over is a live struct of its own.
    unsafe { (step_args(call).file_stat)(as_ctx, path, stat, error) }
}

/// Calls the step's `file_list` with `as_ctx`, as `stat_as` calls
/// `file_stat`.
pub(crate) fn list_as(
    call: *mut RvCall,
    as_ctx: *mut c_void,
    path: RvStr,
    entries: *mut RvDirEntries,
    error: *mut RvFileError,
) -> RvStatus {
    // SAFETY: as in `stat_as`.
    unsafe { (step_args(call).file_list)(as_ctx, path, entries, error) }
}

/// Calls the step's `file_read` with `as_ctx`, as `stat_as` calls
/// `file_stat`.
pub(crate) fn read_as(
    call: *mut RvCall,
    as_ctx: *mut c_void,
    path: RvStr,
    range: (u64, u64),
    bytes: *mut RvBytes,
    error: *mut RvFileError,
) -> RvStatus {
    // SAFETY: as in `stat_as`.
    unsafe { (step_args(call).file_read)(as_ctx, path, range.0, range.1, bytes, error) }
}

/// What a file function of a step answered: what it handed back, or the
/// kind and the text of its failure.
pub(crate) type Answer<T> = Result<T, (u32, String)>;

/// What the step's `file_stat` says of `path`: the file type and the
/// length, or the kind and the text of the failure.
pub(crate) fn stat_of(call: *mut RvCall, path: &str) -> Answer<(u32, u64)> {
    let (mut stat, mut error) = (unset_stat(), unset_error());
    let ctx = step_args(call).ctx;
    match stat_as(call, ctx, rv_str(path), &raw mut stat, &raw mut error) {
        RV_OK => Ok((stat.file_type, stat.len)),
        _ => Err((error.kind, text(error.text))),
    }
}

/// What the step's `file_list` says of `path`: each entry's name, file
/// type, and length, or the kind and the text of the failure.
pub(crate) fn list_of(call: *mut RvCall, path: &str) -> Answer<Vec<(String, u32, u64)>> {
    let (mut entries, mut error) = (unset_entries(), unset_error());
    let ctx = step_args(call).ctx;
    match list_as(call, ctx, rv_str(path), &raw mut entries, &raw mut error) {
        RV_OK if entries.len == 0 => Ok(Vec::new()),
        RV_OK => {
            // SAFETY: the loader keeps the entries and their names valid
            // until the step's next file function.
            let listed = unsafe { slice::from_raw_parts(entries.ptr, entries.len) };
            Ok(listed
                .iter()
                .map(|entry| (text(entry.name), entry.stat.file_type, entry.stat.len))
                .collect())
        }
        _ => Err((error.kind, text(error.text))),
    }
}

/// What the step's `file_read` hands back for `path` and the range
/// `offset` and `len`, or the kind and the text of the failure.
pub(crate) fn read_of(call: *mut RvCall, path: &str, range: (u64, u64)) -> Answer<Vec<u8>> {
    let (mut bytes, mut error) = (unset_bytes(), unset_error());
    let ctx = step_args(call).ctx;
    match read_as(
        call,
        ctx,
        rv_str(path),
        range,
        &raw mut bytes,
        &raw mut error,
    ) {
        RV_OK if bytes.len == 0 => Ok(Vec::new()),
        // SAFETY: the loader keeps the bytes valid until the step's next
        // file function.
        RV_OK => Ok(unsafe { slice::from_raw_parts(bytes.ptr, bytes.len) }.to_vec()),
        _ => Err((error.kind, text(error.text))),
    }
}

/// A call of the fake Pack's tool `name` over an in-memory filesystem
/// that holds each of `files`, a path beside its text.
pub(crate) fn seeded_call(name: &str, files: &[(&str, &str)]) -> TestCall {
    let call = test_call(name);
    for (path, text) in files {
        call.context()
            .access()
            .write(path, text.as_bytes())
            .expect("a seeded file is written");
    }
    call
}
