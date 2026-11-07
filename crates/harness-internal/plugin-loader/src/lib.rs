//! `harness-plugin-loader` - inserts Plugins into a Host's Pack registry,
//! from dynamic libraries in a Plugin directory or linked into the Host.
//! Each Plugin loads as it is inserted.
//!
//! ## Invariants
//!
//! - May depend on: `runvil-plugin` among workspace crates, and
//!   `runvil-plugin-sys`, the C contract at version 0.2, from its own
//!   repository.
//! - Every entry point gets the same calls, whether it came from a library
//!   or was linked in. The only difference is that a library is opened
//!   when it is inserted and closed after its Plugin is dropped.
//! - Dropping a `Plugin` sends `RV_PLUGIN_UNLOAD` once, with the instance
//!   pointer its `RV_PLUGIN_LOAD` set, then closes its library, if it has
//!   one.
//! - An entry point whose `RV_PLUGIN_LOAD` fails gets no
//!   `RV_PLUGIN_UNLOAD`, and its library is closed at once; a failing load
//!   must leave nothing running.
//! - The loader sends only `RV_PLUGIN_LOAD` and `RV_PLUGIN_UNLOAD`, and the
//!   load functions in an `RV_PLUGIN_LOAD` call's `RvLoadArgs` answer only
//!   that call's `ctx`, on its thread, while it runs.
//! - Each `RV_PLUGIN_LOAD` gets its own `ctx`, a loader-owned allocation
//!   freed when a rejected load returns or after its Plugin's
//!   `RV_PLUGIN_UNLOAD` returns.
//! - Nothing the loader keeps points into a library except its entry point
//!   and the Plugin's instance pointer; the name and version are copied.
#![expect(
    unsafe_code,
    reason = "opens dynamic libraries and calls C entry points"
)]

#[cfg(test)]
mod fakes;
mod host;
mod load;
mod plugin;

pub use plugin::{Plugin, PluginReport, PluginSource, insert_plugin, insert_plugin_dir};
