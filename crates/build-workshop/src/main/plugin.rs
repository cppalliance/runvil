//! The std Plugin's library, which Workshop loads from the Plugin
//! directory of its installation.
//!
//! Nothing here builds the library. The Workshop server depends on
//! `workshop-plugin-std`, so Cargo writes the library to `deps` in the
//! profile's directory whenever it builds Workshop, and each mode copies
//! it from there: a build into `plugins` beside Workshop in the target
//! directory, and a macOS or Linux installer into its payload. The
//! Windows setup takes it from `deps` itself, in
//! `crates/workshop/desktop/installer.nsi`.

use std::fs;
use std::path::{Path, PathBuf};

// The name Workshop's Plugin directory lookup searches
// (`crates/workshop/gateway-discovery/src/peer.rs`); a test reads that file
// and requires the same name.
/// The Plugin directory's name.
pub(crate) const PLUGIN_DIR_NAME: &str = "plugins";

/// The file name Cargo gives the std Plugin's library for `target`.
pub(crate) fn library_name(target: &str) -> &'static str {
    let has = |part: &str| target.split('-').any(|word| word == part);
    if has("windows") {
        "plugin_std.dll"
    } else if has("apple") {
        "libplugin_std.dylib"
    } else {
        "libplugin_std.so"
    }
}

/// Where Cargo writes the library when it builds Workshop for `target`
/// into `profile_dir`.
pub(crate) fn built_library(profile_dir: &Path, target: &str) -> PathBuf {
    profile_dir.join("deps").join(library_name(target))
}

/// Copies `library` into the Plugin directory in `beside`, which it makes
/// when it is missing.
pub(crate) fn place(library: &Path, beside: &Path) -> Result<(), String> {
    let name = library
        .file_name()
        .ok_or_else(|| format!("{} names no file", library.display()))?;
    if !library.is_file() {
        return Err(format!(
            "{} is missing; a Workshop build writes it, because the server depends on \
             workshop-plugin-std",
            library.display()
        ));
    }
    let directory = beside.join(PLUGIN_DIR_NAME);
    fs::create_dir_all(&directory)
        .map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
    let placed = directory.join(name);
    // A Workshop from an earlier run may still have the library loaded,
    // and writing into a loaded library's file crashes that process on
    // Linux. A rename replaces the name and leaves that file alone.
    let fresh = placed.with_extension("new");
    let _ = fs::remove_file(&fresh);
    fs::copy(library, &fresh).map_err(|error| {
        format!(
            "cannot copy {} to {}: {error}",
            library.display(),
            fresh.display()
        )
    })?;
    fs::rename(&fresh, &placed).map_err(|error| {
        format!(
            "cannot move {} to {}: {error}",
            fresh.display(),
            placed.display()
        )
    })
}
