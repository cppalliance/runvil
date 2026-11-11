//! The public surface: inserting the library Plugins in a directory or a
//! linked Plugin, and the report, the Plugin, and the failure that come
//! back.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use runvil_plugin::{Error, ErrorFact, PackInfo, PackRegistry};
use runvil_plugin_sys::PluginEntry;

use crate::host::{Loading, send_load};
use crate::load::{Instance, Loaded, make_load_error, open_entry, plugin_files};
use crate::pack::EntryFactory;
use crate::session::InsertedPack;

#[cfg(test)]
#[path = "plugin-tests.rs"]
mod tests;

/// Opens every dynamic library in `directory`, in file-name order, loads
/// each one's Plugin by sending it RV_PLUGIN_LOAD, and inserts the Packs
/// that Plugin offers into `packs`. A file that can't be opened, has no
/// `runvil_plugin`, fails to load, or repeats a Plugin name this call
/// already loaded is recorded in the report as a failure, and one Plugin's
/// failure never stops the others. A repeat did load, so it is unloaded
/// and dropped, and none of its Packs go in. A Pack that `packs` refuses,
/// such as one whose name it already holds, is recorded as a failure too,
/// and the Plugin's other Packs still go in. A dependency library beside a
/// Plugin is opened too, and reported as missing `runvil_plugin`. Names
/// are unique within one call, and nothing remembers them across calls.
///
/// Each Pack that goes in keeps its Plugin loaded for as long as `packs`,
/// or what a Host builds from it, holds the Pack.
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
pub fn insert_plugin_dir(packs: &mut PackRegistry, directory: &Path) -> io::Result<PluginReport> {
    let mut report = PluginReport {
        plugins: Vec::new(),
        failures: Vec::new(),
    };
    for path in plugin_files(directory)? {
        match load_library(&path) {
            Ok((plugin, _))
                if report
                    .plugins
                    .iter()
                    .any(|loaded| loaded.name == plugin.name) =>
            {
                let mut repeat = make_load_error(
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
            Ok((plugin, inserted)) => {
                let plugin = Arc::new(plugin);
                let refused = insert_packs(packs, &plugin, inserted);
                report
                    .failures
                    .extend(refused.into_iter().map(|error| with_path(error, &path)));
                report.plugins.push(plugin);
            }
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
fn load_library(path: &Path) -> Result<(Plugin, Vec<InsertedPack>), Error> {
    let (library, entry) = open_entry(path)?;
    // SAFETY: `insert_plugin_dir`'s documentation requires every
    // `runvil_plugin` to follow the C contract and not unwind, and
    // `library` is the library `entry` came from.
    unsafe { load(PluginSource::Library(path.to_owned()), entry, Some(library)) }
}

/// Sends `entry` RV_PLUGIN_LOAD and, when that succeeds, makes the Plugin
/// that owns its instance and `library`, beside the Packs it inserted.
/// When it fails, `library` drops at once, and no RV_PLUGIN_UNLOAD is
/// sent.
///
/// # Safety
/// `entry` must follow the C contract in `runvil-plugin-sys` and must not
/// unwind, and `library`, when there is one, must be the library `entry`
/// came from.
unsafe fn load(
    source: PluginSource,
    entry: PluginEntry,
    library: Option<libloading::Library>,
) -> Result<(Plugin, Vec<InsertedPack>), Error> {
    // SAFETY: the caller guarantees `entry` follows the C contract and does
    // not unwind.
    let Loading {
        described,
        packs,
        instance,
        ctx,
    } = unsafe { send_load(entry) }?;
    let plugin = Plugin {
        name: described.name,
        version: described.version,
        source,
        loaded: Loaded::new(Instance::new(entry, instance, ctx), library),
    };
    Ok((plugin, packs))
}

/// Inserts each Pack `plugin` offered into `packs`, each with a factory
/// that keeps the Plugin loaded, and returns the failure of each one
/// `packs` refused: a `PluginLoadFailed` error with the `Reason` fact
/// `pack`, the Pack's name as its `Name` fact, and the refusal as its
/// cause.
///
/// A `PackInfo` borrows its text for `'static`, and a Plugin's text lives
/// only as long as the Plugin, so each Pack's name and description are
/// leaked: a few bytes for each Pack, once for each insert.
fn insert_packs(
    packs: &mut PackRegistry,
    plugin: &Arc<Plugin>,
    inserted: Vec<InsertedPack>,
) -> Vec<Error> {
    let mut refused = Vec::new();
    for InsertedPack { name, description } in inserted {
        let info = PackInfo::new(String::leak(name.clone())).description(String::leak(description));
        let factory = EntryFactory::new(Arc::clone(plugin), name.clone());
        if let Err(refusal) = packs.insert(info, factory) {
            let mut error = make_load_error(
                "pack",
                format!(
                    "the Pack {name} of the Plugin {} was not inserted: {refusal}",
                    plugin.name
                ),
            )
            .with_cause(&refusal);
            error.facts.insert(ErrorFact::Name, name.into());
            refused.push(error);
        }
    }
    refused
}

/// Loads a Plugin linked into the Host by sending `entry` RV_PLUGIN_LOAD,
/// inserts the Packs it offers into `packs`, and returns it. `entry` must
/// follow the C contract in `runvil-plugin-sys`, as a library's must.
///
/// Each Pack that goes in keeps the Plugin loaded for as long as `packs`,
/// or what a Host builds from it, holds the Pack.
///
/// This safe function runs foreign code: it calls `entry` with no way to
/// check it, so a Host passes only an entry point it trusts, and `entry`
/// must not unwind.
///
/// # Errors
/// Returns the `PluginLoadFailed` failure, with no `Path` fact, when the
/// Plugin refuses, misuses the load functions, or does not describe
/// itself, and the failure of the first Pack that `packs` refuses. The
/// Packs that went in before that one stay in `packs`.
pub fn insert_plugin(packs: &mut PackRegistry, entry: PluginEntry) -> Result<Arc<Plugin>, Error> {
    // SAFETY: this function's documentation requires `entry` to follow the
    // C contract and not unwind, and a linked Plugin has no library.
    let (plugin, inserted) = unsafe { load(PluginSource::Linked, entry, None) }?;
    let plugin = Arc::new(plugin);
    match insert_packs(packs, &plugin, inserted).into_iter().next() {
        Some(refused) => Err(refused),
        None => Ok(plugin),
    }
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
    /// the failure's kind as its `Reason` fact, and, for a repeated Plugin
    /// name or a Pack the registry refused, that name as its `Name` fact.
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
/// its instance and, for a library Plugin, its library. Each Pack it
/// inserted and each Pack instance made from one holds an `Arc` of it.
/// When the last `Arc` drops, it sends RV_PLUGIN_UNLOAD, then closes the
/// library.
pub struct Plugin {
    /// At most 64 bytes, one name segment.
    name: String,
    /// Non-empty, at most 64 bytes, no control characters.
    version: String,
    source: PluginSource,
    /// Its drop sends RV_PLUGIN_UNLOAD and then closes the library.
    loaded: Loaded,
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

    /// The live instance every later call names.
    pub(crate) fn instance(&self) -> &Instance {
        self.loaded.instance()
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
