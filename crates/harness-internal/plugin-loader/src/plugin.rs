//! The public surface: inserting the library Plugins in a directory or a
//! linked Plugin, and the report, the Plugin, and the failure that come
//! back.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use runvil_plugin::{Error, ErrorFact, PackRegistry};
use runvil_plugin_sys::PluginEntry;

use crate::host::send_load;
use crate::load::{Instance, Loaded, load_error, open_entry, plugin_files};

#[cfg(test)]
#[path = "plugin-tests.rs"]
mod tests;

/// Opens every dynamic library in `directory`, in file-name order, and
/// loads each one's Plugin by sending it RV_PLUGIN_LOAD. Its Packs will go
/// into `packs`; in this MVP no Plugin inserts any. A file that can't be
/// opened, has no `runvil_plugin`, fails to load, or repeats a Plugin name
/// this call already loaded is recorded in the report as a failure, and one
/// Plugin's failure never stops the others. A repeat did load, so it is
/// unloaded and dropped. A dependency library beside a Plugin is opened
/// too, and reported as missing `runvil_plugin`. Names are unique within
/// one call, and nothing remembers them across calls.
///
/// This safe function runs foreign code: opening a library runs its
/// initialization code, and each `runvil_plugin` it calls has no way to be
/// checked, because Rust cannot check the C contract. So a Host points it
/// only at a directory of libraries it trusts, and each `runvil_plugin`
/// must follow the contract in `runvil-plugin-sys` and must not unwind.
///
/// # Errors
/// Returns the I/O error when `directory`, or the metadata of an entry in
/// it with the platform's library extension, cannot be read. Other entries
/// are never examined beyond their names.
#[expect(
    unused_variables,
    reason = "Plugins insert no Packs until insert_pack exists"
)]
pub fn insert_plugin_dir(packs: &mut PackRegistry, directory: &Path) -> io::Result<PluginReport> {
    let mut report = PluginReport {
        plugins: Vec::new(),
        failures: Vec::new(),
    };
    for path in plugin_files(directory)? {
        match load_library(&path) {
            Ok(plugin)
                if report
                    .plugins
                    .iter()
                    .any(|loaded| loaded.name == plugin.name) =>
            {
                let mut repeat = load_error(
                    "duplicate",
                    format!(
                        "a Plugin named {} was already loaded from this directory",
                        plugin.name
                    ),
                );
                repeat
                    .facts
                    .insert(ErrorFact::Name, plugin.name.as_str().into());
                report.failures.push(with_path(repeat, &path));
                // It did load, so this drop sends RV_PLUGIN_UNLOAD and then
                // closes its library, like any other Plugin's.
                drop(plugin);
            }
            Ok(plugin) => report.plugins.push(Arc::new(plugin)),
            Err(error) => report.failures.push(with_path(error, &path)),
        }
    }
    Ok(report)
}

/// `error` with the library's `path` as its `Path` fact.
fn with_path(mut error: Error, path: &Path) -> Error {
    error
        .facts
        .insert(ErrorFact::Path, path.display().to_string().into());
    error
}

/// Opens the library at `path` and loads its Plugin. A failing load closes
/// the library before this returns.
fn load_library(path: &Path) -> Result<Plugin, Error> {
    let (library, entry) = open_entry(path)?;
    // SAFETY: `insert_plugin_dir`'s documentation requires every
    // `runvil_plugin` to follow the C contract and not unwind, and
    // `library` is the library `entry` came from.
    unsafe { load(PluginSource::Library(path.to_owned()), entry, Some(library)) }
}

/// Sends `entry` RV_PLUGIN_LOAD and, when that succeeds, makes the Plugin
/// that owns its instance and `library`. When it fails, `library` drops
/// at once, and no RV_PLUGIN_UNLOAD is sent.
///
/// # Safety
/// `entry` must follow the C contract in `runvil-plugin-sys` and must not
/// unwind, and `library`, when there is one, must be the library `entry`
/// came from.
unsafe fn load(
    source: PluginSource,
    entry: PluginEntry,
    library: Option<libloading::Library>,
) -> Result<Plugin, Error> {
    // SAFETY: the caller guarantees `entry` follows the C contract and does
    // not unwind.
    let (described, instance, ctx) = unsafe { send_load(entry) }?;
    Ok(Plugin {
        name: described.name,
        version: described.version,
        source,
        _loaded: Loaded::new(Instance::new(entry, instance, ctx), library),
    })
}

/// Loads a Plugin linked into the Host by sending `entry` RV_PLUGIN_LOAD,
/// and returns it. Its Packs will go into `packs`; in this MVP no Plugin
/// inserts any. `entry` must follow the C contract in
/// `runvil-plugin-sys`, as a library's must.
///
/// This safe function runs foreign code: it calls `entry` with no way to
/// check it, so a Host passes only an entry point it trusts, and `entry`
/// must not unwind.
///
/// # Errors
/// Returns the `PluginLoadFailed` failure, with no `Path` fact, when the
/// Plugin refuses, misuses the load functions, or does not describe
/// itself.
#[expect(
    unused_variables,
    reason = "Plugins insert no Packs until insert_pack exists"
)]
pub fn insert_plugin(packs: &mut PackRegistry, entry: PluginEntry) -> Result<Arc<Plugin>, Error> {
    // SAFETY: this function's documentation requires `entry` to follow the
    // C contract and not unwind, and a linked Plugin has no library.
    unsafe { load(PluginSource::Linked, entry, None) }.map(Arc::new)
}

/// What one `insert_plugin_dir` call loaded and what failed. It holds one
/// `Arc` of each Plugin it loaded; dropping it releases only that hold.
#[derive(Debug)]
pub struct PluginReport {
    plugins: Vec<Arc<Plugin>>,
    failures: Vec<Error>,
}

impl PluginReport {
    /// Every Plugin this call loaded, in load order.
    #[must_use]
    pub fn plugins(&self) -> &[Arc<Plugin>] {
        &self.plugins
    }

    /// Every failure, in the order it happened: a `PluginLoadFailed` error
    /// whose message says why, with the library's path as its `Path` fact,
    /// the failure's kind as its `Reason` fact, and, for a repeat, the
    /// repeated Plugin name as its `Name` fact.
    #[must_use]
    pub fn failures(&self) -> &[Error] {
        &self.failures
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
/// its instance and, for a library Plugin, its library. When the last `Arc`
/// drops, it sends RV_PLUGIN_UNLOAD, then closes the library.
pub struct Plugin {
    /// At most 64 bytes, one segment, by the same rules as a Pack id.
    name: String,
    /// Non-empty, at most 64 bytes, no control characters.
    version: String,
    source: PluginSource,
    /// Held only so its drop sends RV_PLUGIN_UNLOAD and then closes the
    /// library.
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

/// Shows the name, version, and source, never the library or the instance.
impl fmt::Debug for Plugin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Plugin")
            .field("name", &self.name)
            .field("version", &self.version)
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}
