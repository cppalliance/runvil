//! The C contract every Plugin entry point follows, library or linked.
//!
//! A Plugin is one entry point, a [`PluginEntry`]. A library Plugin
//! exports it as `runvil_plugin`; a linked Plugin has the same shape under
//! a name of its own crate's choosing. These types are the Rust mirror of
//! the C declarations, so a linked Plugin crate, which may depend on
//! `runvil-plugin` alone, can name them. Declaring them is safe Rust; only
//! calling a [`PluginEntry`] or reading through its pointers needs
//! `unsafe`.
//!
//! # The contract
//!
//! - Every call passes an [`RvCall`] that names a verb. Loading returns the
//!   Plugin's instance pointer, and every later call passes it back.
//! - Strings are a pointer plus a length, UTF-8, not NUL-terminated.
//! - Version 0.1 makes no compatibility promise.
//! - Verb names follow `RV_<OBJECT>_<ACTION>`. Libraries open and close;
//!   Plugins load and unload.
//!
//! # Reserved verbs
//!
//! Verbs 3 to 7 are reserved for later versions, so their numbers never
//! change. Each one gets its own arguments struct behind `call->args`, so
//! no verb reads fields meant for another. No constants are declared for
//! them yet.
//!
//! - `RV_PACK_NEW` (3): make one Pack's per-run instance and set
//!   `call->pack_run`.
//! - `RV_PACK_DELETE` (4): free that per-run instance.
//! - `RV_TOOL_CALL` (5): start one tool call, which finishes later through
//!   an [`RvHost`] function.
//! - `RV_TOOL_CANCEL` (6): abandon a started call.
//! - `RV_PRELUDE_INSTALL` (7): install the Pack's native prelude, once per
//!   Lua VM.
//!
//! # Status codes
//!
//! - [`RV_OK`] (0) is success.
//! - Codes 1 to 3 are reserved for the Lua API's own errors: Lua raised an
//!   error, a fatal error such as the memory limit or a cancel, and a wrong
//!   type.
//! - [`RV_ERR_MISUSE`] (4) means the caller broke the contract.
//! - [`RV_ERR_REFUSED`] (5) means the Plugin declined to load.
//! - [`RV_ERR_UNSUPPORTED`] (6) means the Plugin doesn't know the verb.

use std::ffi::{c_char, c_void};

#[cfg(test)]
#[path = "abi-tests.rs"]
mod tests;

/// What every function in the contract returns: zero for success.
pub type RvStatus = u32;
/// Success.
pub const RV_OK: RvStatus = 0;
/// The caller broke the contract.
pub const RV_ERR_MISUSE: RvStatus = 4;
/// The Plugin declined to load; `refuse` returns it.
pub const RV_ERR_REFUSED: RvStatus = 5;
/// The Plugin does not know the verb it was sent.
pub const RV_ERR_UNSUPPORTED: RvStatus = 6;

/// The operation an [`RvCall`] asks for. A fixed-width integer, not a C
/// enum, so its size never depends on the compiler.
pub type RvVerb = u32;
/// Make the Plugin instance and name the Plugin.
pub const RV_PLUGIN_LOAD: RvVerb = 1;
/// Free the Plugin instance.
pub const RV_PLUGIN_UNLOAD: RvVerb = 2;

/// The contract's major version, which [`RvHost`] reports.
pub const RV_API_MAJOR: u32 = 0;
/// The contract's minor version, which [`RvHost`] reports.
pub const RV_API_MINOR: u32 = 1;

/// A UTF-8 string that is not NUL-terminated.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RvStr {
    /// The first byte.
    pub ptr: *const c_char,
    /// The length in bytes.
    pub len: usize,
}

/// What a Plugin tells `describe_plugin` about itself.
#[repr(C)]
#[derive(Debug)]
pub struct RvPluginInfo {
    /// `size_of::<RvPluginInfo>()`.
    pub size: u32,
    /// Zero.
    pub reserved: u32,
    /// At most 64 bytes, one segment, with the same characters as a Pack id.
    pub name: RvStr,
    /// Non-empty, at most 64 bytes, with no control characters.
    pub version: RvStr,
}

/// The functions a Plugin may call back during a call.
#[repr(C)]
#[derive(Debug)]
pub struct RvHost {
    /// `size_of::<RvHost>()`.
    pub size: u32,
    /// [`RV_API_MAJOR`].
    pub api_major: u32,
    /// [`RV_API_MINOR`].
    pub api_minor: u32,
    /// Zero.
    pub reserved: u32,
    /// Declines the load, at most once, with a UTF-8 reason of at most 1024
    /// bytes; returns [`RV_ERR_REFUSED`].
    pub refuse: unsafe extern "C" fn(*const RvCall, RvStr) -> RvStatus,
    /// Names the Plugin, exactly once, during [`RV_PLUGIN_LOAD`].
    pub describe_plugin: unsafe extern "C" fn(*const RvCall, *const RvPluginInfo) -> RvStatus,
}

/// One call into a Plugin's entry point.
#[repr(C)]
#[derive(Debug)]
pub struct RvCall {
    /// `size_of::<RvCall>()`.
    pub size: u32,
    /// The operation.
    pub verb: RvVerb,
    /// The functions the Plugin may call during this call.
    pub host: *const RvHost,
    /// The Plugin instance: NULL going into [`RV_PLUGIN_LOAD`], which may
    /// set it.
    pub plugin: *mut c_void,
    /// The per-run Pack instance; always NULL in version 0.1.
    pub pack_run: *mut c_void,
    /// The verb's own arguments; NULL for both version 0.1 verbs.
    pub args: *const c_void,
}

/// The shape of every Plugin entry point.
///
/// The entry point must not unwind: a Rust panic across `extern "C"`
/// aborts the process.
pub type PluginEntry = unsafe extern "C" fn(*mut RvCall) -> RvStatus;
