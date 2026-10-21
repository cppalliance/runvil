//! `harness-plugin-loader` - inserts Plugins into a Host's Pack registry,
//! from dynamic libraries in a Plugin directory or linked into the Host.
//! Each Plugin loads as it is inserted.
//!
//! ## Invariants
//!
//! - May depend on: `runvil-plugin` among workspace crates.
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
//!   `RvHost` functions answer only the `RV_PLUGIN_LOAD` call they were
//!   handed, on its thread.
//! - Nothing the loader keeps points into a library except its entry point
//!   and the Plugin's instance pointer; the name and version are copied.
#![expect(
    unsafe_code,
    reason = "opens dynamic libraries and calls C entry points"
)]

#[cfg(test)]
#[path = "fakes-tests.rs"]
mod fakes;
mod host;
mod load;
mod plugin;

pub use plugin::{
    Plugin, PluginFailure, PluginReport, PluginSource, insert_plugin, insert_plugin_dir,
};
