//! Listing and opening Plugin libraries, owning a loaded Plugin, and
//! naming why one did not load.

use std::env::consts::DLL_EXTENSION;
use std::ffi::{OsStr, c_void};
use std::io;
use std::path::{Path, PathBuf};

use runvil_plugin::abi::{PluginEntry, RvStatus};

use crate::host::send_unload;

#[cfg(test)]
#[path = "load-tests.rs"]
mod tests;

/// The top-level regular files in `directory` whose extension is
/// `std::env::consts::DLL_EXTENSION`, sorted by file name, each made
/// absolute with `std::path::absolute`. Other files and subdirectories are
/// ignored, and the extension is compared exactly. A symbolic link to a
/// regular file counts as that file, and a link that leads nowhere is
/// ignored.
///
/// # Errors
/// Returns the I/O error when `directory`, or the metadata of an entry in
/// it, cannot be read. An entry that is missing when its metadata is read
/// is not an error.
pub(crate) fn plugin_files(directory: &Path) -> io::Result<Vec<PathBuf>> {
    let directory = std::path::absolute(directory)?;
    let mut names = Vec::new();
    for entry in std::fs::read_dir(&directory)? {
        let entry = entry?;
        let path = entry.path();
        // `metadata` follows a link.
        let is_file = is_regular_file(std::fs::metadata(&path))?;
        if is_file && path.extension() == Some(OsStr::new(DLL_EXTENSION)) {
            names.push(entry.file_name());
        }
    }
    names.sort();
    Ok(names.into_iter().map(|name| directory.join(name)).collect())
}

/// Whether `metadata`, read for an entry the directory listing returned,
/// is a regular file. A `NotFound` error means the entry is not one: a
/// link that leads nowhere, or an entry removed since the listing. Any
/// other error, such as a permission error, is returned, so the entry
/// does not silently vanish from the Plugin directory's report.
fn is_regular_file(metadata: io::Result<std::fs::Metadata>) -> io::Result<bool> {
    match metadata {
        Ok(metadata) => Ok(metadata.is_file()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// The one symbol a library Plugin exports.
const ENTRY_POINT: &[u8] = b"runvil_plugin\0";

/// Opens the library at the absolute `path`. On Windows its own
/// dependencies resolve only from its directory and System32, with
/// `LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32`.
#[cfg(target_os = "windows")]
fn open_library(path: &Path) -> io::Result<libloading::Library> {
    use libloading::os::windows::{LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32};

    let flags = LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32;
    // SAFETY: the path is absolute, as LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR
    // requires. Opening a library runs its initialization code, which the
    // contract in `insert_plugin_dir`'s documentation covers, and its
    // dependencies resolve only beside it or from System32.
    let library = unsafe { libloading::os::windows::Library::load_with_flags(path, flags) }
        .map_err(open_error)?;
    Ok(library.into())
}

/// Opens the library at `path`.
#[cfg(not(target_os = "windows"))]
fn open_library(path: &Path) -> io::Result<libloading::Library> {
    // SAFETY: opening a library runs its initialization code, which the
    // contract in `insert_plugin_dir`'s documentation covers.
    unsafe { libloading::Library::new(path) }.map_err(open_error)
}

/// The `io::Error` for a library that did not open: `error`'s text and,
/// when it has a cause, the cause's text after it. On Windows
/// `libloading` reports only "LoadLibraryExW failed" and keeps the system's
/// reason, such as "The specified module could not be found. (os error
/// 126)", as the error's `source`, which a plain `Display` of the error
/// would lose.
fn open_error(error: libloading::Error) -> io::Error {
    match std::error::Error::source(&error) {
        Some(cause) => io::Error::other(format!("{error}: {cause}")),
        None => io::Error::other(error),
    }
}

/// Opens the library at `path` and resolves `ENTRY_POINT`. The entry point
/// is valid only while the returned library stays open. When the symbol is
/// missing, the library closes before this returns.
pub(crate) fn open_entry(path: &Path) -> Result<(libloading::Library, PluginEntry), LoadError> {
    let library = open_library(path).map_err(LoadError::Open)?;
    let entry = {
        // SAFETY: the C contract gives `runvil_plugin` the shape of
        // `PluginEntry`, and `ENTRY_POINT` ends in NUL.
        let symbol =
            unsafe { library.get::<PluginEntry>(ENTRY_POINT) }.map_err(LoadError::MissingEntry)?;
        *symbol
    };
    Ok((library, entry))
}

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

/// What a loaded Plugin owns: its instance and, for a library Plugin, its
/// library. Fields drop in declaration order, so the instance sends
/// RV_PLUGIN_UNLOAD before the library closes, with no hand-written `Drop`
/// for this struct. A linked Plugin has no library.
pub(crate) struct Loaded {
    _instance: Instance,
    _library: Option<libloading::Library>,
}

impl Loaded {
    /// Owns `instance` and the `library` its entry point came from, if
    /// any.
    pub(crate) fn new(instance: Instance, library: Option<libloading::Library>) -> Loaded {
        Loaded {
            _instance: instance,
            _library: library,
        }
    }
}

/// Why one Plugin did not load; its message becomes the failure's reason.
#[derive(Debug, thiserror::Error)]
pub(crate) enum LoadError {
    #[error("the library could not be opened: {0}")]
    Open(io::Error),
    #[error("runvil_plugin is missing: {0}")]
    MissingEntry(libloading::Error),
    #[error("the Plugin refused: {0}")]
    Refused(String),
    #[error("the Plugin misused the RvHost table: {0}")]
    Misuse(&'static str),
    #[error("RV_PLUGIN_LOAD returned status {0}")]
    Status(RvStatus),
    #[error("RV_PLUGIN_LOAD returned without describing the Plugin")]
    Undescribed,
    #[error("a Plugin named {0} was already loaded from this directory")]
    Duplicate(String),
}
