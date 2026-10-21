//! `harness-plugin-loader` - inserts Plugins into a Host's Pack registry,
//! from entry points linked into the Host. Each Plugin loads as it is
//! inserted.
//!
//! ## Invariants
//!
//! - May depend on: `runvil-plugin` among workspace crates.
//! - Dropping a `Plugin` sends `RV_PLUGIN_UNLOAD` once, with the instance
//!   pointer its `RV_PLUGIN_LOAD` set.
//! - An entry point whose `RV_PLUGIN_LOAD` fails gets no
//!   `RV_PLUGIN_UNLOAD`; a failing load must leave nothing running.
//! - The loader sends only `RV_PLUGIN_LOAD` and `RV_PLUGIN_UNLOAD`, and the
//!   `RvHost` functions answer only the `RV_PLUGIN_LOAD` call they were
//!   handed, on its thread.
//! - Nothing the loader keeps points into a Plugin except its entry point
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

pub use plugin::{Plugin, PluginFailure, PluginSource, insert_plugin};
