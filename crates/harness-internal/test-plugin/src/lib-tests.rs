//! Tests for the fixture's private copy of the C contract and its entry
//! point. The layout and number tests pin the same values as
//! `runvil-plugin-sys`, because this crate cannot import it. The verb
//! tests call `runvil_plugin` directly with a hand-built `RvHost` table
//! and arguments structs, whose functions record what they were given.

use std::cell::{Cell, RefCell};
use std::ptr;
use std::slice;

#[cfg(target_pointer_width = "64")]
use std::mem::offset_of;

use super::*;

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
    /// The name and description of every Pack this test's thread inserted.
    static INSERTED: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
    /// The status `insert_pack` answers with.
    static INSERT_STATUS: Cell<RvStatus> = const { Cell::new(RV_OK) };
    /// The name, wire name, and parameters of every tool this test's
    /// thread added.
    static ADDED: RefCell<Vec<(String, String, String)>> = const { RefCell::new(Vec::new()) };
    /// How many `add_tool` calls answer `RV_OK` before one answers
    /// `RV_ERR_MISUSE`.
    static ADDS_ALLOWED: Cell<usize> = const { Cell::new(usize::MAX) };
    /// The kind and text of every result this test's thread finished with.
    static FINISHED: RefCell<Vec<(RvResultKind, String)>> = const { RefCell::new(Vec::new()) };
    /// How many times the Plugin called `refuse`, which it never should.
    static REFUSALS: Cell<usize> = const { Cell::new(0) };
}

/// Copies the bytes of `text` into a `String`.
fn copy(text: RvStr) -> String {
    if text.len == 0 {
        return String::new();
    }
    // SAFETY: the entry point under test hands over a string that lives
    // for the whole call and holds `len` bytes.
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

/// The load arguments' `insert_pack`: records the Pack and answers with
/// `INSERT_STATUS`.
unsafe extern "C" fn insert_pack(ctx: *mut c_void, info: *const RvPackInfo) -> RvStatus {
    assert_eq!(ctx, test_ctx(), "insert_pack is handed the load's own ctx");
    // SAFETY: the entry point under test passes a live `RvPackInfo`.
    let info = unsafe { &*info };
    assert_eq!(info.size, size_of_u32::<RvPackInfo>());
    assert_eq!(info.reserved, 0);
    INSERTED.with_borrow_mut(|all| all.push((copy(info.name), copy(info.description))));
    INSERT_STATUS.get()
}

/// The `refuse` of every arguments struct: counts itself, because this
/// Plugin never refuses.
unsafe extern "C" fn refuse(_ctx: *mut c_void, _reason: RvStr) -> RvStatus {
    REFUSALS.set(REFUSALS.get() + 1);
    RV_ERR_REFUSED
}

/// The Pack arguments' `add_tool`: records the tool, and answers `RV_OK`
/// until `ADDS_ALLOWED` runs out.
unsafe extern "C" fn add_tool(ctx: *mut c_void, info: *const RvToolInfo) -> RvStatus {
    assert_eq!(ctx, test_ctx(), "add_tool is handed the call's own ctx");
    // SAFETY: the entry point under test passes a live `RvToolInfo`.
    let info = unsafe { &*info };
    assert_eq!(info.size, size_of_u32::<RvToolInfo>());
    assert_eq!(info.reserved, 0);
    assert!(info.description.len > 0, "each tool has a description");
    if ADDS_ALLOWED.get() == 0 {
        return RV_ERR_MISUSE;
    }
    ADDS_ALLOWED.set(ADDS_ALLOWED.get() - 1);
    ADDED.with_borrow_mut(|all| {
        all.push((copy(info.name), copy(info.wire_name), copy(info.parameters)));
    });
    RV_OK
}

/// The step arguments' `finish`: records the result.
unsafe extern "C" fn finish(ctx: *mut c_void, result: *const RvToolResult) -> RvStatus {
    assert_eq!(ctx, test_ctx(), "finish is handed the step's own ctx");
    // SAFETY: the entry point under test passes a live `RvToolResult`.
    let result = unsafe { &*result };
    assert_eq!(result.size, size_of_u32::<RvToolResult>());
    FINISHED.with_borrow_mut(|all| all.push((result.kind, copy(result.text))));
    RV_OK
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
    host_with(size_of_u32::<RvHost>(), 0, 3)
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
        insert_pack,
    }
}

/// Load arguments the way the loader builds them.
fn args() -> RvLoadArgs {
    args_with(size_of_u32::<RvLoadArgs>())
}

/// Pack arguments that report `size` and name the Pack `pack`.
fn pack_args(size: u32, pack: &str) -> RvPackNewArgs {
    RvPackNewArgs {
        size,
        reserved: 0,
        ctx: test_ctx(),
        pack: rv_str(pack),
        refuse,
        add_tool,
    }
}

/// Step arguments that report `size`, for the tool `tool` with
/// `arguments`.
fn step_args(size: u32, tool: &str, arguments: &str) -> RvToolCallArgs {
    RvToolCallArgs {
        size,
        reserved: 0,
        ctx: test_ctx(),
        tool: rv_str(tool),
        arguments: rv_str(arguments),
        finish,
    }
}

/// A call that sends `verb` and carries `plugin`, with `host` as its table
/// and `args` as its arguments.
fn call_for<T>(verb: RvVerb, host: &RvHost, args: *const T, plugin: *mut c_void) -> RvCall {
    RvCall {
        size: size_of_u32::<RvCall>(),
        verb,
        host: ptr::from_ref(host),
        plugin,
        pack_run: ptr::null_mut(),
        args: args.cast(),
        tool_call: ptr::null_mut(),
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
    call.pack_run = ptr::null_mut();
    call.tool_call = ptr::null_mut();
    send(call)
}

/// Sends `RV_PACK_NEW` for the fixture's Pack on the loaded `call`, which
/// then carries the Pack instance.
fn new_pack(call: &mut RvCall) -> RvStatus {
    let args = pack_args(size_of_u32::<RvPackNewArgs>(), PACK);
    call.verb = RV_PACK_NEW;
    call.args = (&raw const args).cast();
    send(call)
}

/// Sends `RV_PACK_DELETE` for the Pack instance `call` carries.
fn delete_pack(call: &mut RvCall) -> RvStatus {
    call.verb = RV_PACK_DELETE;
    call.args = ptr::null();
    let status = send(call);
    call.pack_run = ptr::null_mut();
    status
}

/// Sends one `RV_TOOL_CALL` step of `tool` with `arguments` on the Pack
/// instance `call` carries.
fn step(call: &mut RvCall, tool: &str, arguments: &str) -> RvStatus {
    let args = step_args(size_of_u32::<RvToolCallArgs>(), tool, arguments);
    call.verb = RV_TOOL_CALL;
    call.args = (&raw const args).cast();
    send(call)
}

/// Sends `RV_TOOL_CANCEL` for the call state `call` carries.
fn cancel(call: &mut RvCall) -> RvStatus {
    call.verb = RV_TOOL_CANCEL;
    call.args = ptr::null();
    let status = send(call);
    call.tool_call = ptr::null_mut();
    status
}

/// How many times the Plugin called `describe_plugin`.
fn describes() -> usize {
    DESCRIBED.with_borrow(Vec::len)
}

/// How many instances this test's thread has made and not yet freed.
fn live_instances() -> isize {
    LIVE_INSTANCES.get()
}

/// The results this test's thread finished with, in order.
fn finished() -> Vec<(RvResultKind, String)> {
    FINISHED.with_borrow(Clone::clone)
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
fn the_infos_and_the_result_keep_the_layout_the_contract_pins() {
    assert_eq!(size_of::<RvPluginInfo>(), 40);
    assert_eq!(offset_of!(RvPluginInfo, size), 0);
    assert_eq!(offset_of!(RvPluginInfo, reserved), 4);
    assert_eq!(offset_of!(RvPluginInfo, name), 8);
    assert_eq!(offset_of!(RvPluginInfo, version), 24);

    assert_eq!(size_of::<RvPackInfo>(), 40);
    assert_eq!(offset_of!(RvPackInfo, size), 0);
    assert_eq!(offset_of!(RvPackInfo, reserved), 4);
    assert_eq!(offset_of!(RvPackInfo, name), 8);
    assert_eq!(offset_of!(RvPackInfo, description), 24);

    assert_eq!(size_of::<RvToolInfo>(), 72);
    assert_eq!(offset_of!(RvToolInfo, size), 0);
    assert_eq!(offset_of!(RvToolInfo, reserved), 4);
    assert_eq!(offset_of!(RvToolInfo, name), 8);
    assert_eq!(offset_of!(RvToolInfo, wire_name), 24);
    assert_eq!(offset_of!(RvToolInfo, description), 40);
    assert_eq!(offset_of!(RvToolInfo, parameters), 56);

    assert_eq!(size_of::<RvToolResult>(), 24);
    assert_eq!(offset_of!(RvToolResult, size), 0);
    assert_eq!(offset_of!(RvToolResult, kind), 4);
    assert_eq!(offset_of!(RvToolResult, text), 8);
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
fn the_arguments_structs_keep_the_layout_the_contract_pins() {
    assert_eq!(size_of::<RvLoadArgs>(), 40);
    assert_eq!(offset_of!(RvLoadArgs, size), 0);
    assert_eq!(offset_of!(RvLoadArgs, reserved), 4);
    assert_eq!(offset_of!(RvLoadArgs, ctx), 8);
    assert_eq!(offset_of!(RvLoadArgs, refuse), 16);
    assert_eq!(offset_of!(RvLoadArgs, describe_plugin), 24);
    assert_eq!(offset_of!(RvLoadArgs, insert_pack), 32);

    assert_eq!(size_of::<RvPackNewArgs>(), 48);
    assert_eq!(offset_of!(RvPackNewArgs, size), 0);
    assert_eq!(offset_of!(RvPackNewArgs, reserved), 4);
    assert_eq!(offset_of!(RvPackNewArgs, ctx), 8);
    assert_eq!(offset_of!(RvPackNewArgs, pack), 16);
    assert_eq!(offset_of!(RvPackNewArgs, refuse), 32);
    assert_eq!(offset_of!(RvPackNewArgs, add_tool), 40);

    assert_eq!(size_of::<RvToolCallArgs>(), 56);
    assert_eq!(offset_of!(RvToolCallArgs, size), 0);
    assert_eq!(offset_of!(RvToolCallArgs, reserved), 4);
    assert_eq!(offset_of!(RvToolCallArgs, ctx), 8);
    assert_eq!(offset_of!(RvToolCallArgs, tool), 16);
    assert_eq!(offset_of!(RvToolCallArgs, arguments), 32);
    assert_eq!(offset_of!(RvToolCallArgs, finish), 48);
}

#[test]
#[cfg(target_pointer_width = "64")]
fn the_call_keeps_the_layout_the_contract_pins() {
    assert_eq!(size_of::<RvCall>(), 48);
    assert_eq!(offset_of!(RvCall, size), 0);
    assert_eq!(offset_of!(RvCall, verb), 4);
    assert_eq!(offset_of!(RvCall, host), 8);
    assert_eq!(offset_of!(RvCall, plugin), 16);
    assert_eq!(offset_of!(RvCall, pack_run), 24);
    assert_eq!(offset_of!(RvCall, args), 32);
    assert_eq!(offset_of!(RvCall, tool_call), 40);
}

#[test]
fn the_status_codes_and_the_result_kinds_keep_their_numbers() {
    assert_eq!(RV_OK, 0);
    assert_eq!(RV_ERR_MISUSE, 4);
    assert_eq!(RV_ERR_REFUSED, 5);
    assert_eq!(RV_ERR_UNSUPPORTED, 6);
    assert_eq!(RV_PENDING, 7);
    assert_eq!(RV_RESULT_TRUSTED, 1);
    assert_eq!(RV_RESULT_OTHER, 5);
}

#[test]
fn the_verbs_and_the_version_keep_their_numbers() {
    assert_eq!(RV_PLUGIN_LOAD, 1);
    assert_eq!(RV_PLUGIN_UNLOAD, 2);
    assert_eq!(RV_PACK_NEW, 3);
    assert_eq!(RV_PACK_DELETE, 4);
    assert_eq!(RV_TOOL_CALL, 5);
    assert_eq!(RV_TOOL_CANCEL, 6);
    assert_eq!(RV_API_MAJOR, 0);
    assert_eq!(RV_API_MINOR, 3);
}

#[test]
fn a_load_describes_test_plugin_once_inserts_its_pack_and_sets_the_instance() {
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
    INSERTED.with_borrow(|all| {
        assert_eq!(
            *all,
            [(
                "fixture/test".to_owned(),
                "The loader's test fixture".to_owned()
            )],
            "the load inserts its one Pack exactly once"
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
    for verb in [0, 7, 8, u32::MAX] {
        let (status, call) = send_verb(verb, &host(), ptr::null());
        assert_eq!(status, RV_ERR_UNSUPPORTED, "verb {verb}");
        assert!(call.plugin.is_null(), "verb {verb} sets no instance");
    }
    assert_eq!(describes(), 0, "no other verb names the Plugin");
}

#[test]
fn a_host_reporting_version_0_2_is_unsupported_and_leaves_no_instance() {
    let (status, call) = send_load_with(&host_with(size_of_u32::<RvHost>(), 0, 2));
    assert_eq!(status, RV_ERR_UNSUPPORTED);
    assert!(call.plugin.is_null());
    assert_eq!(describes(), 0, "a 0.2 Host is never called back");
    assert_eq!(live_instances(), 0);
}

#[test]
fn a_table_that_is_too_short_or_not_version_0_3_is_unsupported_and_leaves_no_instance() {
    let full = size_of_u32::<RvHost>();
    let short = [0, 4, 8, 12, full - 1].map(|size| (size, 0, 3));
    let versions = [(1, 3), (2, 3), (u32::MAX, 3), (0, 0), (0, 4), (0, u32::MAX)]
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
    let host = host_with(size_of_u32::<RvHost>() + 32, 0, 3);
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
    for size in [0, 4, 8, 16, 24, 32, full - 1] {
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
fn a_host_that_is_not_0_3_is_rejected_before_null_args_are_noticed() {
    let (status, _call) = send_verb(
        RV_PLUGIN_LOAD,
        &host_with(size_of_u32::<RvHost>(), 0, 2),
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

#[test]
fn an_insert_pack_that_fails_fails_the_load_with_its_status_and_no_instance() {
    INSERT_STATUS.set(RV_ERR_MISUSE);
    let (status, call) = send_load_with(&host());
    assert_eq!(
        status, RV_ERR_MISUSE,
        "the load returns insert_pack's status"
    );
    assert!(call.plugin.is_null());
    assert_eq!(
        live_instances(),
        0,
        "a failing load frees the instance it made"
    );
}

#[test]
fn a_pack_instance_adds_its_three_tools_and_is_deleted_once() {
    let host = host();
    let mut call = loaded(&host);
    assert_eq!(new_pack(&mut call), RV_OK);
    assert!(!call.pack_run.is_null(), "a successful call sets pack_run");
    ADDED.with_borrow(|all| {
        let tools: Vec<(&str, &str, &str)> = all
            .iter()
            .map(|(name, wire, parameters)| (name.as_str(), wire.as_str(), parameters.as_str()))
            .collect();
        assert_eq!(
            tools,
            [
                ("hello", "", NO_PARAMETERS),
                ("steps", "", NO_PARAMETERS),
                ("never", "", NO_PARAMETERS),
            ]
        );
    });
    assert_eq!(delete_pack(&mut call), RV_OK);
    assert_eq!(REFUSALS.get(), 0, "the Plugin never refuses");
    assert_eq!(unload(&mut call), RV_OK);
}

#[test]
fn a_pack_instance_whose_tool_is_rejected_is_freed_and_never_handed_over() {
    let host = host();
    let mut call = loaded(&host);
    ADDS_ALLOWED.set(1);
    assert_eq!(new_pack(&mut call), RV_ERR_MISUSE);
    assert!(call.pack_run.is_null());
    assert_eq!(unload(&mut call), RV_OK);
}

#[test]
fn a_pack_verb_with_the_wrong_arguments_or_no_instance_makes_nothing() {
    let host = host();
    let mut call = loaded(&host);
    call.verb = RV_PACK_NEW;
    call.args = ptr::null();
    assert_eq!(
        send(&mut call),
        RV_ERR_MISUSE,
        "RV_PACK_NEW needs its arguments"
    );

    let short = pack_args(size_of_u32::<RvPackNewArgs>() - 1, PACK);
    call.args = (&raw const short).cast();
    assert_eq!(send(&mut call), RV_ERR_UNSUPPORTED, "short arguments");

    let other = pack_args(size_of_u32::<RvPackNewArgs>(), "fixture/other");
    call.args = (&raw const other).cast();
    assert_eq!(send(&mut call), RV_ERR_MISUSE, "a Pack it never inserted");

    assert!(call.pack_run.is_null());
    assert_eq!(delete_pack(&mut call), RV_ERR_MISUSE, "nothing to delete");
    assert_eq!(unload(&mut call), RV_OK);
}

#[test]
fn the_hello_tool_finishes_in_one_step_with_its_arguments() {
    let host = host();
    let mut call = loaded(&host);
    assert_eq!(new_pack(&mut call), RV_OK);
    assert_eq!(step(&mut call, "hello", r#"{"a":1}"#), RV_OK);
    assert_eq!(
        finished(),
        [(RV_RESULT_TRUSTED, r#"hello {"a":1}"#.to_owned())]
    );
    assert!(
        call.tool_call.is_null(),
        "a call that ended leaves no state"
    );
    assert_eq!(delete_pack(&mut call), RV_OK);
    assert_eq!(unload(&mut call), RV_OK);
}

#[test]
fn the_steps_tool_is_pending_twice_and_finishes_in_its_third_step() {
    let host = host();
    let mut call = loaded(&host);
    assert_eq!(new_pack(&mut call), RV_OK);
    assert_eq!(step(&mut call, "steps", "{}"), RV_PENDING);
    let state = call.tool_call;
    assert!(!state.is_null(), "a pending call keeps its state");
    assert_eq!(step(&mut call, "steps", "{}"), RV_PENDING);
    assert_eq!(call.tool_call, state, "the same state comes back");
    assert_eq!(finished(), [], "a pending step finishes nothing");
    assert_eq!(step(&mut call, "steps", "{}"), RV_OK);
    assert_eq!(finished(), [(RV_RESULT_TRUSTED, "3 steps".to_owned())]);
    assert!(call.tool_call.is_null(), "the last step frees the state");
    assert_eq!(delete_pack(&mut call), RV_OK);
    assert_eq!(unload(&mut call), RV_OK);
}

#[test]
fn the_never_tool_stays_pending_until_a_cancel_frees_its_state() {
    let host = host();
    let mut call = loaded(&host);
    assert_eq!(new_pack(&mut call), RV_OK);
    for _ in 0..5 {
        assert_eq!(step(&mut call, "never", "{}"), RV_PENDING);
    }
    assert!(!call.tool_call.is_null());
    assert_eq!(cancel(&mut call), RV_OK);
    assert_eq!(
        cancel(&mut call),
        RV_OK,
        "a cancel with no state frees nothing"
    );
    assert_eq!(finished(), []);
    assert_eq!(delete_pack(&mut call), RV_OK);
    assert_eq!(unload(&mut call), RV_OK);
}

#[test]
fn a_tool_the_pack_does_not_have_fails_the_call() {
    let host = host();
    let mut call = loaded(&host);
    assert_eq!(new_pack(&mut call), RV_OK);
    assert_eq!(step(&mut call, "absent", "{}"), RV_OK);
    assert_eq!(
        finished(),
        [(
            RV_RESULT_OTHER,
            "test-plugin has no tool named absent".to_owned()
        )]
    );
    assert_eq!(delete_pack(&mut call), RV_OK);
    assert_eq!(unload(&mut call), RV_OK);
}

#[test]
fn a_step_with_the_wrong_arguments_ends_the_call_and_frees_its_state() {
    let host = host();
    let mut call = loaded(&host);
    assert_eq!(new_pack(&mut call), RV_OK);
    assert_eq!(step(&mut call, "never", "{}"), RV_PENDING);
    call.verb = RV_TOOL_CALL;
    call.args = ptr::null();
    assert_eq!(send(&mut call), RV_ERR_MISUSE, "a step needs its arguments");
    assert!(
        call.tool_call.is_null(),
        "a step that ends the call leaves no state, whatever its status"
    );

    assert_eq!(step(&mut call, "never", "{}"), RV_PENDING);
    let short = step_args(size_of_u32::<RvToolCallArgs>() - 1, "never", "{}");
    call.args = (&raw const short).cast();
    assert_eq!(send(&mut call), RV_ERR_UNSUPPORTED, "short arguments");
    assert!(call.tool_call.is_null());
    assert_eq!(finished(), []);
    assert_eq!(delete_pack(&mut call), RV_OK);
    assert_eq!(unload(&mut call), RV_OK);
}
