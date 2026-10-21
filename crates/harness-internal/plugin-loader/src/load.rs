//! Owning a loaded Plugin and naming why one did not load.

use std::ffi::c_void;

use runvil_plugin::abi::{PluginEntry, RvStatus};

use crate::host::send_unload;

/// A live Plugin instance: the entry point and the pointer RV_PLUGIN_LOAD set.
/// Dropping it sends RV_PLUGIN_UNLOAD once.
pub(crate) struct Instance {
    entry: PluginEntry,
    plugin: *mut c_void,
}

impl Instance {
    /// Takes ownership of the instance `plugin` that a successful
    /// RV_PLUGIN_LOAD through `entry` set.
    pub(crate) fn new(entry: PluginEntry, plugin: *mut c_void) -> Instance {
        Instance { entry, plugin }
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        // SAFETY: an `Instance` is made only from a successful
        // RV_PLUGIN_LOAD, so `entry` follows the contract and `plugin` is
        // the pointer it set, and this drop is the only unload.
        unsafe { send_unload(self.entry, self.plugin) };
    }
}

// SAFETY: the C contract lets RV_PLUGIN_UNLOAD come on any thread, and the
// loader never reads `plugin`.
unsafe impl Send for Instance {}
// SAFETY: `Instance` has no method that takes `&self`, so sharing it
// between threads exposes nothing.
unsafe impl Sync for Instance {}

/// What a loaded Plugin owns. Fields drop in declaration order, so the
/// instance sends RV_PLUGIN_UNLOAD before anything declared after it is
/// released.
pub(crate) struct Loaded {
    _instance: Instance,
}

impl Loaded {
    /// Owns `instance`.
    pub(crate) fn new(instance: Instance) -> Loaded {
        Loaded {
            _instance: instance,
        }
    }
}

/// Why one Plugin did not load; its message becomes the failure's reason.
#[derive(Debug, thiserror::Error)]
pub(crate) enum LoadError {
    #[error("the Plugin refused: {0}")]
    Refused(String),
    #[error("the Plugin misused the RvHost table: {0}")]
    Misuse(&'static str),
    #[error("RV_PLUGIN_LOAD returned status {0}")]
    Status(RvStatus),
    #[error("RV_PLUGIN_LOAD returned without describing the Plugin")]
    Undescribed,
}
