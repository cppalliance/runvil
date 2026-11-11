//! The call the loader has open on a thread: the [`Session`] its `ctx`
//! points at, the thread-local slot that names it, and the checks every
//! function a Plugin calls back begins with.

use std::cell::Cell;
use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::{self, NonNull};
use std::slice;

use runvil_plugin::{Error, ToolOutput};
use runvil_plugin_sys::{RV_ERR_MISUSE, RV_ERR_REFUSED, RvStatus, RvStr};
use serde_json::Value;

#[cfg(test)]
#[path = "session-tests.rs"]
mod tests;

thread_local! {
    /// The session of the call open on this thread, which its `ctx` points
    /// at, or `None` while no call is open. A `Cell`, so no borrow is ever
    /// held and no borrow check can fail: `answer` takes it out while it
    /// works, and `open` and `closed` swap it.
    static OPEN: Cell<Option<NonNull<Session>>> = const { Cell::new(None) };
}

/// The verb of an open call, which decides the functions that answer it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Verb {
    /// `RV_PLUGIN_LOAD`.
    Load,
    /// `RV_PACK_NEW`.
    PackNew,
    /// `RV_TOOL_CALL`.
    ToolCall,
}

/// What one call into a Plugin has reported so far; the call's `ctx`
/// points at it.
pub(crate) struct Session {
    /// The call's verb.
    verb: Verb,
    /// The first way the Plugin broke the contract during the call.
    pub(crate) misuse: Option<&'static str>,
    /// The reason the Plugin declined with.
    pub(crate) refusal: Option<String>,
    /// The name and version a load described the Plugin with.
    pub(crate) described: Option<Described>,
    /// The Packs a load inserted, in order.
    pub(crate) packs: Vec<InsertedPack>,
    /// The tools an `RV_PACK_NEW` added, in order.
    pub(crate) tools: Vec<AddedTool>,
    /// The result a step finished its call with.
    pub(crate) result: Option<Result<ToolOutput, Error>>,
}

/// The name and version a Plugin gave `describe_plugin`, copied.
pub(crate) struct Described {
    pub(crate) name: String,
    pub(crate) version: String,
}

/// A Pack a Plugin gave `insert_pack`, copied.
pub(crate) struct InsertedPack {
    /// A vendor and a Pack id joined by `/`.
    pub(crate) name: String,
    pub(crate) description: String,
}

/// A tool a Pack instance gave `add_tool`, copied.
pub(crate) struct AddedTool {
    /// One name segment.
    pub(crate) name: String,
    /// `None` for the wire name the Engine derives.
    pub(crate) wire_name: Option<String>,
    pub(crate) description: String,
    /// The parsed JSON Schema, an object.
    pub(crate) parameters: Value,
}

/// Sends one call of `verb` with a fresh session open on this thread.
/// `send` gets the session's `ctx` and returns the Plugin's status. The
/// session is closed before this returns that status and what the Plugin
/// reported, so no slot holds the session and nothing else reaches it.
pub(crate) fn open(
    verb: Verb,
    send: impl FnOnce(*mut c_void) -> RvStatus,
) -> (RvStatus, Box<Session>) {
    let session = NonNull::from(Box::leak(Box::new(Session {
        verb,
        misuse: None,
        refusal: None,
        described: None,
        packs: Vec::new(),
        tools: Vec::new(),
        result: None,
    })));
    // A call inside a call, on one thread, keeps the outer session.
    let outer = OPEN.with(|cell| cell.replace(Some(session)));
    let status = send(session.as_ptr().cast());
    // Everything that touches the slot while `send` runs (`answer`, `open`,
    // and `closed`) puts back what it replaced, so this swap closes the
    // session opened above.
    OPEN.with(|cell| cell.set(outer));
    // SAFETY: `session` came from `Box::leak` above, and with the call
    // closed no slot holds it, so this is its only owner.
    (status, unsafe { Box::from_raw(session.as_ptr()) })
}

/// Sends a call that has no arguments, with no session open on this
/// thread, so a function the Plugin calls meanwhile answers
/// RV_ERR_MISUSE.
pub(crate) fn closed(send: impl FnOnce() -> RvStatus) -> RvStatus {
    // A call inside a call, on one thread, must not see the outer session.
    // The last `Arc` of a Plugin may drop inside a thread-local destructor;
    // `try_with` keeps such a call going out even on a thread whose slot is
    // gone, which `answer` treats as no open call.
    let outer = OPEN.try_with(Cell::take).unwrap_or_default();
    let status = send();
    let _restored = OPEN.try_with(|cell| cell.set(outer));
    status
}

/// The functions of one verb's arguments struct, and what a breach of the
/// `ctx` rules by one of them is called.
pub(crate) struct Family {
    verb: Verb,
    wrong_ctx: &'static str,
    wrong_call: &'static str,
}

/// The functions in an `RvLoadArgs`.
pub(crate) const LOAD: Family = Family {
    verb: Verb::Load,
    wrong_ctx: "a load function was handed a ctx other than the open RV_PLUGIN_LOAD call's",
    wrong_call: "a load function was called outside RV_PLUGIN_LOAD",
};

/// The functions in an `RvPackNewArgs`.
pub(crate) const PACK_NEW: Family = Family {
    verb: Verb::PackNew,
    wrong_ctx: "a Pack function was handed a ctx other than the open RV_PACK_NEW call's",
    wrong_call: "a Pack function was called outside RV_PACK_NEW",
};

/// The function in an `RvToolCallArgs`.
pub(crate) const TOOL_CALL: Family = Family {
    verb: Verb::ToolCall,
    wrong_ctx: "finish was handed a ctx other than the open RV_TOOL_CALL step's",
    wrong_call: "finish was called outside RV_TOOL_CALL",
};

/// Answers a function of `family`. `f` runs on the session only when this
/// thread has a call of the family's verb open and `ctx` is its `ctx`;
/// otherwise the answer is RV_ERR_MISUSE. A fault `f` returns is answered
/// the same way, and an open call records the first misuse. Nothing here
/// reads through `ctx` itself, so a stale, freed, or made-up `ctx` is only
/// compared.
///
/// The session is out of its slot while `f` runs, so a function that `f`
/// somehow reached would find no open call and answer RV_ERR_MISUSE.
pub(crate) fn answer(
    ctx: *mut c_void,
    family: &Family,
    f: impl FnOnce(&mut Session) -> Result<RvStatus, &'static str>,
) -> RvStatus {
    OPEN.try_with(|cell| {
        let Some(open) = cell.take() else {
            return RV_ERR_MISUSE;
        };
        // SAFETY: only `open` fills the slot, with the live session of the
        // call it has open on this thread, and it empties the slot before
        // that session can be freed. The slot is empty while this runs, so
        // nothing else reaches the session meanwhile.
        let session = unsafe { &mut *open.as_ptr() };
        let outcome = if !ptr::eq(open.as_ptr().cast::<c_void>(), ctx) {
            Err(family.wrong_ctx)
        } else if session.verb != family.verb {
            Err(family.wrong_call)
        } else {
            f(session)
        };
        let status = outcome.unwrap_or_else(|fault| {
            session.misuse.get_or_insert(fault);
            RV_ERR_MISUSE
        });
        cell.set(Some(open));
        status
    })
    .unwrap_or(RV_ERR_MISUSE)
}

/// Records the Plugin's refusal of the open call of `family`, validated;
/// returns RV_ERR_REFUSED, or RV_ERR_MISUSE for a `ctx` it doesn't answer
/// or a second refusal.
pub(crate) fn refuse(ctx: *mut c_void, family: &Family, reason: RvStr) -> RvStatus {
    answer(ctx, family, |session| {
        if session.refusal.is_some() {
            return Err("refuse was called twice");
        }
        // SAFETY: the contract makes `reason` valid for its length during
        // this call.
        session.refusal = Some(unsafe { REASON.read(reason) }?);
        Ok(RV_ERR_REFUSED)
    })
}

/// One string's bound, and what each way of breaking it is called.
pub(crate) struct Field {
    cap: usize,
    too_long: &'static str,
    null_with_length: &'static str,
    not_utf8: &'static str,
}

/// The [`Field`] of the string `$what` names, bounded at `$cap` bytes.
macro_rules! field {
    ($what:literal, $cap:literal) => {
        $crate::session::Field::new(
            $cap,
            concat!($what, " is longer than ", $cap, " bytes"),
            concat!($what, " has a NULL pointer and a nonzero length"),
            concat!($what, " is not UTF-8"),
        )
    };
}
pub(crate) use field;

/// A refusal's reason.
const REASON: Field = field!("the refusal reason", 1024);

impl Field {
    /// A field bounded at `cap` bytes, with the name of each breach.
    pub(crate) const fn new(
        cap: usize,
        too_long: &'static str,
        null_with_length: &'static str,
        not_utf8: &'static str,
    ) -> Field {
        Field {
            cap,
            too_long,
            null_with_length,
            not_utf8,
        }
    }

    /// Copies the string `s` holds. The length is checked against the cap
    /// before any byte is read, and `s.ptr` is read only when `s.len` is
    /// nonzero, because `slice::from_raw_parts` with a NULL pointer is
    /// undefined behavior even at length 0.
    ///
    /// # Safety
    /// When `s.len` is nonzero, `s.ptr` must be valid for reads of `s.len`
    /// bytes for the duration of this call.
    pub(crate) unsafe fn read(&self, s: RvStr) -> Result<String, &'static str> {
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

/// One struct a Plugin hands a function, and what each way of handing it
/// over wrongly is called.
pub(crate) struct Shape {
    null: &'static str,
    misaligned: &'static str,
    short: &'static str,
}

/// The [`Shape`] of the struct `$struct`, which the function `$function`
/// takes and calls its `$what`.
macro_rules! shape {
    ($function:literal, $what:literal, $struct:literal) => {
        $crate::session::Shape::new(
            concat!($function, " was handed a NULL ", $what),
            concat!($function, " was handed a misaligned ", $what),
            concat!("the ", $what, "'s size is shorter than ", $struct),
        )
    };
}
pub(crate) use shape;

/// What a `reserved` field that holds anything but zero is called.
pub(crate) const RESERVED: &str = "the info's reserved field is not zero";

impl Shape {
    /// A shape with the name of each breach.
    pub(crate) const fn new(
        null: &'static str,
        misaligned: &'static str,
        short: &'static str,
    ) -> Shape {
        Shape {
            null,
            misaligned,
            short,
        }
    }

    /// The struct `at` points at, once it is known to be whole.
    ///
    /// # Safety
    /// When `at` is not NULL and is aligned, it must be readable as a
    /// struct that begins with a `u32` saying how much of it the Plugin
    /// wrote, and readable as a whole `T` when that says so, for the
    /// lifetime `'a`.
    pub(crate) unsafe fn read<'a, T>(&self, at: *const T) -> Result<&'a T, &'static str> {
        if at.is_null() {
            return Err(self.null);
        }
        if !at.is_aligned() {
            return Err(self.misaligned);
        }
        // SAFETY: `at` is not NULL and is aligned, and `size` is the first
        // field of a struct the Plugin wrote, so it is readable on its own.
        let size = unsafe { at.cast::<u32>().read() };
        if (size as usize) < size_of::<T>() {
            return Err(self.short);
        }
        // SAFETY: `size` says the Plugin wrote a whole `T`.
        Ok(unsafe { &*at })
    }
}

/// `size_of::<T>()` as the `u32` a contract struct's `size` field holds.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the assertion bounds the size to a u32"
)]
pub(crate) const fn size_field<T>() -> u32 {
    assert!(
        size_of::<T>() <= u32::MAX as usize,
        "a contract struct's size fits its u32 size field"
    );
    size_of::<T>() as u32
}

/// `s` as the contract's pointer and length.
pub(crate) const fn rv_str(s: &str) -> RvStr {
    RvStr {
        ptr: s.as_ptr().cast(),
        len: s.len(),
    }
}
