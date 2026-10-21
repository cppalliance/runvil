//! The public surface: inserting a linked Plugin, and the Plugin and the
//! failure that come back.

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use runvil_plugin::PackRegistry;
use runvil_plugin::abi::PluginEntry;

use crate::host::send_load;
use crate::load::{Instance, Loaded};

#[cfg(test)]
#[path = "plugin-tests.rs"]
mod tests;

/// Loads a Plugin linked into the Host by sending `entry` RV_PLUGIN_LOAD,
/// and returns it. Its Packs will go into `packs`; in this MVP no Plugin
/// inserts any. `entry` must follow the C contract in
/// `runvil_plugin::abi`, as a library's must.
///
/// This safe function runs foreign code: it calls `entry` with no way to
/// check it, so a Host passes only an entry point it trusts, and `entry`
/// must not unwind.
///
/// # Errors
/// Returns the failure, with the source `Linked`, when the Plugin refuses,
/// misuses the `RvHost` functions, or does not describe itself.
#[expect(
    unused_variables,
    reason = "Plugins insert no Packs until insert_pack exists"
)]
pub fn insert_plugin(
    packs: &mut PackRegistry,
    entry: PluginEntry,
) -> Result<Arc<Plugin>, PluginFailure> {
    // SAFETY: this function's documentation requires `entry` to follow the
    // C contract and not unwind.
    match unsafe { send_load(entry) } {
        Ok((described, plugin)) => Ok(Arc::new(Plugin {
            name: described.name,
            version: described.version,
            source: PluginSource::Linked,
            _loaded: Loaded::new(Instance::new(entry, plugin)),
        })),
        Err(error) => Err(PluginFailure {
            source: PluginSource::Linked,
            reason: error.to_string(),
        }),
    }
}

/// Where a Plugin's entry point came from.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PluginSource {
    /// A dynamic library at this absolute path, as `std::path::absolute`
    /// made it.
    Library(PathBuf),
    /// Compiled into the Host and inserted with `insert_plugin`.
    Linked,
}

/// One Plugin that loaded and named itself, shared through `Arc`. It owns
/// its instance. When the last `Arc` drops, it sends RV_PLUGIN_UNLOAD.
pub struct Plugin {
    /// At most 64 bytes, one segment, by the same rules as a Pack id.
    name: String,
    /// Non-empty, at most 64 bytes, no control characters.
    version: String,
    source: PluginSource,
    /// Held only so its drop sends RV_PLUGIN_UNLOAD.
    _loaded: Loaded,
}

impl Plugin {
    /// The name the Plugin gave `describe_plugin`.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The version the Plugin gave `describe_plugin`.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Where its entry point came from.
    #[must_use]
    pub fn source(&self) -> &PluginSource {
        &self.source
    }
}

/// Shows the name, version, and source, never the instance.
impl fmt::Debug for Plugin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Plugin")
            .field("name", &self.name)
            .field("version", &self.version)
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

/// One inserted entry point that did not open or load.
#[derive(Debug, Clone)]
pub struct PluginFailure {
    source: PluginSource,
    /// Readable, such as "RV_PLUGIN_LOAD returned without describing the
    /// Plugin".
    reason: String,
}

impl PluginFailure {
    /// Where the entry point that failed came from.
    #[must_use]
    pub fn source(&self) -> &PluginSource {
        &self.source
    }

    /// Why it failed, readable.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
}
