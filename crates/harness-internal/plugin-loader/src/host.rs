//! The `RvHost` table every call hands a Plugin, the load functions each
//! RV_PLUGIN_LOAD's `RvLoadArgs` carries, and the two verb senders. Each
//! load's `ctx` is a heap allocation holding that load's session, and a
//! thread-local slot names the one `ctx` the load functions may answer.

use std::cell::Cell;
use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::{self, NonNull};
use std::slice;

use runvil_plugin::{Error, PackId};
use runvil_plugin_sys::{
    PluginEntry, RV_API_MAJOR, RV_API_MINOR, RV_ERR_MISUSE, RV_ERR_REFUSED, RV_OK, RV_PLUGIN_LOAD,
    RV_PLUGIN_UNLOAD, RvCall, RvHost, RvLoadArgs, RvPluginInfo, RvStatus, RvStr,
};

use crate::load::make_load_error;

#[cfg(test)]
#[path = "host-tests.rs"]
mod tests;

/// The `RvHost` table every call hands the Plugin.
pub(crate) static HOST: RvHost = RvHost {
    size: size_field::<RvHost>(),
    api_major: RV_API_MAJOR,
    api_minor: RV_API_MINOR,
    reserved: 0,
};

thread_local! {
    /// The session of the RV_PLUGIN_LOAD call running on this thread, which
    /// its `ctx` points at, or `None` when no load is running. A `Cell`, so
    /// no borrow is ever held and no borrow check can fail: `answer` takes
    /// it out while it works, and `send_load` and `send_unload` swap it.
    static OPEN: Cell<Option<NonNull<Session>>> = const { Cell::new(None) };
}

/// What one RV_PLUGIN_LOAD has reported so far.
#[derive(Default)]
struct Session {
    described: Option<Described>,
    refusal: Option<String>,
    misuse: Option<&'static str>,
}

/// The `ctx` of one RV_PLUGIN_LOAD call: a heap allocation holding its
/// session. It is freed when it drops: as the rejected load returns, or,
/// for a load that succeeds, after its RV_PLUGIN_UNLOAD returns. So while a
/// Plugin is loaded, its `ctx` is the address of no other load's.
pub(crate) struct LoadCtx {
    session: NonNull<Session>,
}

impl LoadCtx {
    /// Allocates a `ctx` with an empty session.
    fn new() -> LoadCtx {
        LoadCtx {
            session: NonNull::from(Box::leak(Box::<Session>::default())),
        }
    }

    /// The pointer the load's `RvLoadArgs` carries.
    fn as_ctx(&self) -> *mut c_void {
        self.session.as_ptr().cast()
    }

    /// Takes what the load reported. Called only after `send_load` closed
    /// the session, so no slot holds it and nothing else reaches it.
    fn take_session(&mut self) -> Session {
        // SAFETY: `session` is the live allocation `new` made, and once its
        // load returned no slot holds it, so this is its only access.
        std::mem::take(unsafe { self.session.as_mut() })
    }
}

impl Drop for LoadCtx {
    fn drop(&mut self) {
        // SAFETY: `session` came from `Box::leak` in `new`, and this drop
        // is its only free.
        drop(unsafe { Box::from_raw(self.session.as_ptr()) });
    }
}

/// The name and version a Plugin gave `describe_plugin`, copied.
pub(crate) struct Described {
    pub(crate) name: String,
    pub(crate) version: String,
}

/// One string field's bound, and what each way of breaking it is called.
struct Field {
    cap: usize,
    too_long: &'static str,
    null_with_length: &'static str,
    not_utf8: &'static str,
}

const NAME: Field = Field {
    cap: 64,
    too_long: "the name is longer than 64 bytes",
    null_with_length: "the name has a NULL pointer and a nonzero length",
    not_utf8: "the name is not UTF-8",
};

const VERSION: Field = Field {
    cap: 64,
    too_long: "the version is longer than 64 bytes",
    null_with_length: "the version has a NULL pointer and a nonzero length",
    not_utf8: "the version is not UTF-8",
};

const REASON: Field = Field {
    cap: 1024,
    too_long: "the refusal reason is longer than 1024 bytes",
    null_with_length: "the refusal reason has a NULL pointer and a nonzero length",
    not_utf8: "the refusal reason is not UTF-8",
};

impl Field {
    /// Copies the string `s` holds. The length is checked against the cap
    /// before any byte is read, and `s.ptr` is read only when `s.len` is
    /// nonzero, because `slice::from_raw_parts` with a NULL pointer is
    /// undefined behavior even at length 0.
    ///
    /// # Safety
    /// When `s.len` is nonzero, `s.ptr` must be valid for reads of `s.len`
    /// bytes for the duration of this call.
    unsafe fn read(&self, s: RvStr) -> Result<String, &'static str> {
        if s.len > self.cap {
            return Err(self.too_long);
        }
        if s.len == 0 {
            return Ok(String::new());
        }
        if s.ptr.is_null() {
            return Err(self.null_with_length);
        }
        // SAFETY: `s.ptr` is not NULL and `s.len` is at most the cap, and
        // the caller guarantees `s.ptr` is valid for `s.len` bytes.
        let bytes = unsafe { slice::from_raw_parts(s.ptr.cast::<u8>(), s.len) };
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| self.not_utf8)
    }
}

/// `size_of::<T>()` as the `u32` a contract struct's `size` field holds.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the assertion bounds the size to a u32"
)]
const fn size_field<T>() -> u32 {
    assert!(
        size_of::<T>() <= u32::MAX as usize,
        "a contract struct's size fits its u32 size field"
    );
    size_of::<T>() as u32
}

/// Answers a load function. `f` runs on the session only when this thread
/// has an RV_PLUGIN_LOAD call open and `ctx` is its `ctx`; otherwise the
/// answer is RV_ERR_MISUSE, and an open load on this thread records the
/// misuse. Nothing here reads through `ctx` itself, so a stale, freed, or
/// made-up `ctx` is only compared.
///
/// The session is out of its slot while `f` runs, so a load function that
/// `f` somehow reached would find no open load and answer RV_ERR_MISUSE.
fn answer(ctx: *mut c_void, f: impl FnOnce(&mut Session) -> RvStatus) -> RvStatus {
    OPEN.try_with(|cell| {
        let open = cell.take();
        let status = match open {
            Some(open) => {
                let is_open_ctx = ptr::eq(open.as_ptr().cast::<c_void>(), ctx);
                // SAFETY: only `send_load` fills the slot, with the live
                // session of the load it has open on this thread, and it
                // empties the slot before that session can be freed. The
                // slot is empty while this runs, so nothing else reaches
                // the session meanwhile.
                let session = unsafe { &mut *open.as_ptr() };
                if is_open_ctx {
                    f(session)
                } else {
                    session.misuse.get_or_insert(
                        "a load function was handed a ctx other than the open RV_PLUGIN_LOAD call's",
                    );
                    RV_ERR_MISUSE
                }
            }
            None => RV_ERR_MISUSE,
        };
        cell.set(open);
        status
    })
    .unwrap_or(RV_ERR_MISUSE)
}

/// Records the Plugin's refusal, validated; returns RV_ERR_REFUSED, or
/// RV_ERR_MISUSE for a `ctx` it doesn't answer or a second refusal.
pub(crate) unsafe extern "C" fn refuse(ctx: *mut c_void, reason: RvStr) -> RvStatus {
    answer(ctx, |session| {
        if session.refusal.is_some() {
            session.misuse.get_or_insert("refuse was called twice");
            return RV_ERR_MISUSE;
        }
        // SAFETY: the contract makes `reason` valid for its length during
        // this call.
        match unsafe { REASON.read(reason) } {
            Ok(reason) => {
                session.refusal = Some(reason);
                RV_ERR_REFUSED
            }
            Err(fault) => {
                session.misuse.get_or_insert(fault);
                RV_ERR_MISUSE
            }
        }
    })
}

/// Records the Plugin's description once, validated; returns RV_OK or
/// RV_ERR_MISUSE.
pub(crate) unsafe extern "C" fn describe_plugin(
    ctx: *mut c_void,
    info: *const RvPluginInfo,
) -> RvStatus {
    answer(ctx, |session| {
        // SAFETY: the contract makes `info` point at a readable
        // `RvPluginInfo` during this call.
        match unsafe { describe(session, info) } {
            Ok(()) => RV_OK,
            Err(fault) => {
                session.misuse.get_or_insert(fault);
                RV_ERR_MISUSE
            }
        }
    })
}

/// Validates `info` and copies the name and version into `session`.
///
/// # Safety
/// When `info` is not NULL, it must be readable as an `RvPluginInfo`
/// whose `size` field says how much of it the Plugin wrote, and each
/// string in it must be valid for its length, for the duration of this
/// call.
unsafe fn describe(session: &mut Session, info: *const RvPluginInfo) -> Result<(), &'static str> {
    if session.described.is_some() {
        return Err("describe_plugin was called twice");
    }
    if info.is_null() {
        return Err("describe_plugin was handed a NULL info");
    }
    if !info.is_aligned() {
        return Err("describe_plugin was handed a misaligned info");
    }
    // SAFETY: `info` is not NULL and is aligned, and `size` is the first
    // field of a struct the Plugin wrote, so it is readable on its own.
    let size = unsafe { (&raw const (*info).size).read() };
    if (size as usize) < size_of::<RvPluginInfo>() {
        return Err("the info's size is shorter than RvPluginInfo");
    }
    // SAFETY: `size` says the Plugin wrote a whole `RvPluginInfo`.
    let info = unsafe { &*info };
    if info.reserved != 0 {
        return Err("the info's reserved field is not zero");
    }
    // SAFETY: the caller makes each string valid for its length.
    let name = unsafe { NAME.read(info.name) }?;
    if PackId::parse(&name).is_err() {
        return Err("the name is not one Pack id segment, such as `acme-url`");
    }
    // SAFETY: as above.
    let version = unsafe { VERSION.read(info.version) }?;
    if version.is_empty() {
        return Err("the version is empty");
    }
    if version.chars().any(char::is_control) {
        return Err("the version has a control character");
    }
    session.described = Some(Described { name, version });
    Ok(())
}

/// Sends RV_PLUGIN_LOAD through `entry` with a fresh `ctx` open on this
/// thread, then closes it. On success, returns the description, the
/// instance pointer the Plugin set, and the `ctx`, which the caller keeps
/// until RV_PLUGIN_UNLOAD returns. On failure, the `ctx` is freed before
/// this returns.
///
/// # Safety
/// `entry` must follow the C contract in `runvil-plugin-sys`, and must not
/// unwind.
pub(crate) unsafe fn send_load(
    entry: PluginEntry,
) -> Result<(Described, *mut c_void, LoadCtx), Error> {
    let mut ctx = LoadCtx::new();
    let args = RvLoadArgs {
        size: size_field::<RvLoadArgs>(),
        reserved: 0,
        ctx: ctx.as_ctx(),
        refuse,
        describe_plugin,
    };
    let mut call = RvCall {
        size: size_field::<RvCall>(),
        verb: RV_PLUGIN_LOAD,
        host: &raw const HOST,
        plugin: ptr::null_mut(),
        pack_run: ptr::null_mut(),
        args: (&raw const args).cast(),
    };
    let call_ptr = &raw mut call;
    // A load inside a load, on one thread, keeps the outer session.
    let outer = OPEN.with(|cell| cell.replace(Some(ctx.session)));
    // SAFETY: the caller guarantees `entry` follows the contract, and
    // `call_ptr` points at a live `RvCall` that nothing else uses, whose
    // `args` is live for the call.
    let status = unsafe { entry(call_ptr) };
    // Everything that touches the slot while `entry` runs (`answer`,
    // `send_load`, and `send_unload`) puts back what it replaced, so this
    // swap closes the session opened above.
    OPEN.with(|cell| cell.set(outer));
    let session = ctx.take_session();
    if let Some(reason) = session.refusal {
        return Err(make_load_error(
            "refused",
            format!("the Plugin refused: {reason}"),
        ));
    }
    if let Some(reason) = session.misuse {
        return Err(make_load_error(
            "misuse",
            format!("the Plugin misused the load functions: {reason}"),
        ));
    }
    if status != RV_OK {
        return Err(make_load_error(
            "status",
            format!("RV_PLUGIN_LOAD returned status {status}"),
        ));
    }
    match session.described {
        Some(described) => Ok((described, call.plugin, ctx)),
        None => Err(make_load_error(
            "undescribed",
            "RV_PLUGIN_LOAD returned without describing the Plugin".to_owned(),
        )),
    }
}

/// Sends RV_PLUGIN_UNLOAD through `entry` with `plugin` and NULL `args`. No
/// load is open on this thread meanwhile, so any load function the Plugin
/// calls answers RV_ERR_MISUSE. The status is ignored, because a Plugin
/// that is going away has no one to tell.
///
/// # Safety
/// `entry` must follow the C contract and must not unwind, and `plugin`
/// must be the pointer its RV_PLUGIN_LOAD set, not yet unloaded.
pub(crate) unsafe fn send_unload(entry: PluginEntry, plugin: *mut c_void) {
    let mut call = RvCall {
        size: size_field::<RvCall>(),
        verb: RV_PLUGIN_UNLOAD,
        host: &raw const HOST,
        plugin,
        pack_run: ptr::null_mut(),
        args: ptr::null(),
    };
    // An unload inside a load, on one thread, must not see the load's
    // session. The last `Arc` of a Plugin may drop inside a thread-local
    // destructor; `try_with` keeps such an unload going out even on a
    // thread whose slot is gone, which `answer` treats as no open load.
    let outer = OPEN.try_with(Cell::take).unwrap_or_default();
    // SAFETY: the caller guarantees `entry` follows the contract and
    // `plugin` is the live instance, and `call` is a live `RvCall`.
    let _status = unsafe { entry(&raw mut call) };
    let _restored = OPEN.try_with(|cell| cell.set(outer));
}
