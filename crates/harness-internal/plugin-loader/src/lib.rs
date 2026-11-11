//! `harness-plugin-loader` - inserts Plugins into a Host's Pack registry,
//! from dynamic libraries in a Plugin directory or linked into the Host.
//! Each Plugin loads as it is inserted, and each Pack it offers goes into
//! the registry with a factory that makes one instance of the Pack for
//! each run and performs that instance's tool calls, all through the
//! Plugin's entry point.
//!
//! ## Invariants
//!
//! - May depend on: `runvil-plugin` among workspace crates, and
//!   `runvil-plugin-sys`, the C contract at version 0.4, from its own
//!   repository.
//! - Every entry point gets the same calls, whether it came from a library
//!   or was linked in. The only difference is that a library is opened
//!   when it is inserted and closed after its Plugin is dropped.
//! - Dropping a `Plugin` sends `RV_PLUGIN_UNLOAD` once, with the instance
//!   pointer its `RV_PLUGIN_LOAD` set, then closes its library, if it has
//!   one. Each Pack in the registry and each Pack instance of a run holds
//!   the `Plugin`, so the unload comes after every `RV_PACK_DELETE`.
//! - An entry point whose `RV_PLUGIN_LOAD` fails gets no
//!   `RV_PLUGIN_UNLOAD`, and its library is closed at once; a failing load
//!   must leave nothing running. None of its Packs go into the registry.
//! - Dropping a Pack instance sends `RV_PACK_DELETE` once. A tool call
//!   borrows its instance, so the delete comes after every call of the
//!   instance has ended. An `RV_PACK_NEW` that fails gets no
//!   `RV_PACK_DELETE`.
//! - A tool call sends one `RV_TOOL_CALL` each time its future is polled,
//!   and a step that returns `RV_PENDING` wakes the future's own task. A
//!   call whose last step returned `RV_PENDING` gets one `RV_TOOL_CANCEL`
//!   when it fails or its future is dropped, and no other call does.
//! - The functions in a call's arguments struct answer only that call's
//!   `ctx`, on its thread, while it runs.
//! - A step's file functions read through the `Access` of the step's tool
//!   call and nothing else, so the run's mode and policy apply to every
//!   read. What a file function hands back is freed by the step's next
//!   file function, or when the step returns.
//! - Each `RV_PLUGIN_LOAD` gets its own `ctx`, a loader-owned allocation
//!   freed when a rejected load returns or after its Plugin's
//!   `RV_PLUGIN_UNLOAD` returns. The `ctx` of every other call is freed
//!   when the call returns.
//! - Nothing the loader keeps points into a library except its entry
//!   point and the pointers the Plugin set for the loader to pass back,
//!   which the loader never reads through; every text is copied.
//! - What a Plugin hands the loader is checked as `testing` in
//!   `runvil-plugin-sdk` checks it: the same order, the same bounds, and
//!   the same words for each misuse.
#![expect(
    unsafe_code,
    reason = "opens dynamic libraries and calls C entry points"
)]

mod call;
#[cfg(test)]
mod fakes;
mod file;
mod host;
mod load;
mod pack;
mod plugin;
mod session;

pub use plugin::{Plugin, PluginReport, PluginSource, insert_plugin, insert_plugin_dir};
