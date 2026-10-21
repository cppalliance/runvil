//! The `RvHost` table every call hands a Plugin, the functions in it, and
//! the two verb senders. A thread-local session ties the functions to the
//! one `RV_PLUGIN_LOAD` call they may answer.

use std::cell::Cell;
use std::ffi::c_void;
use std::mem::size_of;
use std::{ptr, slice};

use runvil_plugin::PackId;
use runvil_plugin::abi::{
    PluginEntry, RV_API_MAJOR, RV_API_MINOR, RV_ERR_MISUSE, RV_ERR_REFUSED, RV_OK, RV_PLUGIN_LOAD,
    RV_PLUGIN_UNLOAD, RvCall, RvHost, RvPluginInfo, RvStatus, RvStr,
};

use crate::load::LoadError;

#[cfg(test)]
#[path = "host-tests.rs"]
mod tests;

/// The `RvHost` table every call hands the Plugin.
pub(crate) static HOST: RvHost = RvHost {
    size: size_field::<RvHost>(),
    api_major: RV_API_MAJOR,
    api_minor: RV_API_MINOR,
    reserved: 0,
    refuse,
    describe_plugin,
};

thread_local! {
    /// The RV_PLUGIN_LOAD call running on this thread, as a `Session` that
    /// is idle when none is. A `Cell`, so no borrow is ever held and no
    /// borrow check can fail: `answer` takes the session out while it
    /// works, and `send_load` and `send_unload` swap it.
    static SESSION: Cell<Session> = const { Cell::new(Session::idle()) };
}

/// What one RV_PLUGIN_LOAD has reported so far, or an idle session when no
/// RV_PLUGIN_LOAD call is running.
struct Session {
    /// The address of the open call, compared and never dereferenced; it
    /// is `None` while the session is idle.
    call: Option<*const RvCall>,
    described: Option<Described>,
    refusal: Option<String>,
    misuse: Option<&'static str>,
}

impl Session {
    /// A session with no call open.
    const fn idle() -> Session {
        Session {
            call: None,
            described: None,
            refusal: None,
            misuse: None,
        }
    }

    /// A session open on `call`, with nothing reported yet.
    fn open(call: *const RvCall) -> Session {
        Session {
            call: Some(call),
            ..Session::idle()
        }
    }
}

impl Default for Session {
    fn default() -> Session {
        Session::idle()
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

/// Answers an `RvHost` function. `f` runs only when this thread has an
/// RV_PLUGIN_LOAD call open and `call` is its address; otherwise the
/// answer is RV_ERR_MISUSE, and an open session on this thread records
/// the misuse. A thread whose session slot is already destroyed has no
/// session, so it answers RV_ERR_MISUSE too. Nothing here dereferences
/// `call`.
///
/// The session is out of its slot while `f` runs, so an `RvHost` function
/// that `f` somehow reached would find an idle session and answer
/// RV_ERR_MISUSE.
fn answer(call: *const RvCall, f: impl FnOnce(&mut Session) -> RvStatus) -> RvStatus {
    SESSION
        .try_with(|cell| {
            let mut session = cell.take();
            let status = match session.call {
                Some(open) if ptr::eq(open, call) => f(&mut session),
                Some(_) => {
                    session.misuse.get_or_insert(
                        "an RvHost function was handed a call other than the open RV_PLUGIN_LOAD call",
                    );
                    RV_ERR_MISUSE
                }
                None => RV_ERR_MISUSE,
            };
            cell.set(session);
            status
        })
        .unwrap_or(RV_ERR_MISUSE)
}

/// Records the Plugin's refusal, validated; returns RV_ERR_REFUSED, or
/// RV_ERR_MISUSE for a call it doesn't answer or a second refusal.
unsafe extern "C" fn refuse(call: *const RvCall, reason: RvStr) -> RvStatus {
    answer(call, |session| {
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
unsafe extern "C" fn describe_plugin(call: *const RvCall, info: *const RvPluginInfo) -> RvStatus {
    answer(call, |session| {
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

/// Sends RV_PLUGIN_LOAD through `entry` with a session open on this thread, then
/// closes it. On success, returns the description and the instance
/// pointer the Plugin set.
///
/// # Safety
/// `entry` must follow the C contract in `runvil_plugin::abi`, and must
/// not unwind.
pub(crate) unsafe fn send_load(entry: PluginEntry) -> Result<(Described, *mut c_void), LoadError> {
    let mut call = RvCall {
        size: size_field::<RvCall>(),
        verb: RV_PLUGIN_LOAD,
        host: &raw const HOST,
        plugin: ptr::null_mut(),
        pack_run: ptr::null_mut(),
        args: ptr::null(),
    };
    let call_ptr = &raw mut call;
    // A load inside a load, on one thread, keeps the outer session.
    let outer = SESSION.with(|cell| cell.replace(Session::open(call_ptr.cast_const())));
    // SAFETY: the caller guarantees `entry` follows the contract, and
    // `call_ptr` points at a live `RvCall` that nothing else uses.
    let status = unsafe { entry(call_ptr) };
    // Everything that touches the slot while `entry` runs (`answer`,
    // `send_load`, and `send_unload`) puts back what it replaced, so this
    // swap returns the session opened above.
    let session = SESSION.with(|cell| cell.replace(outer));
    if let Some(reason) = session.refusal {
        return Err(LoadError::Refused(reason));
    }
    if let Some(reason) = session.misuse {
        return Err(LoadError::Misuse(reason));
    }
    if status != RV_OK {
        return Err(LoadError::Status(status));
    }
    match session.described {
        Some(described) => Ok((described, call.plugin)),
        None => Err(LoadError::Undescribed),
    }
}

/// Sends RV_PLUGIN_UNLOAD through `entry` with `plugin`. No session is open, so
/// any `RvHost` function the Plugin calls answers RV_ERR_MISUSE. The
/// status is ignored, because a Plugin that is going away has no one to
/// tell.
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
    // destructor that runs after this one's slot is destroyed. Such a thread
    // has no session to hide, and `answer` treats its destroyed slot as "no
    // session", so the unload still goes out.
    let outer = SESSION.try_with(Cell::take).unwrap_or_default();
    // SAFETY: the caller guarantees `entry` follows the contract and
    // `plugin` is the live instance, and `call` is a live `RvCall`.
    let _status = unsafe { entry(&raw mut call) };
    let _restored = SESSION.try_with(|cell| cell.set(outer));
}
