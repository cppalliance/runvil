//! The std Plugin's library, which Workshop loads from the Plugin
//! directory of its installation.
//!
//! Nothing here builds the library. The Workshop server depends on
//! `workshop-plugin-std`, so Cargo writes the library to `deps` in the
//! profile's directory whenever it builds Workshop, and each mode takes
//! it from there. A build copies it into `plugins` beside Workshop in the
//! target directory. A macOS or Linux installer has Tauri copy it into
//! Workshop's bundle, so the updater, which replaces only that bundle,
//! replaces the library with it. The Windows setup takes it from `deps`
//! itself, in `crates/workshop/desktop/installer.nsi`.

use std::fs;
use std::path::{Path, PathBuf};

use crate::installer::System;

// The names Workshop's Plugin directory lookup searches
// (`crates/workshop/gateway-discovery/src/peer.rs`); a test reads that file
// and requires the same three.
/// The Plugin directory's name.
pub(crate) const PLUGIN_DIR_NAME: &str = "plugins";
/// The directory that holds the Plugin directory inside the macOS
/// Workshop bundle, from the bundle's `Contents`.
pub(crate) const MACOS_PLUGIN_PARENT: &str = "Resources";
/// The directory that holds the Plugin directory inside the Linux
/// AppImage, from the root of the AppDir.
pub(crate) const APPIMAGE_PLUGIN_PARENT: &str = "usr/lib/runvil";

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

/// Where Workshop's bundle for `system` keeps the library named `name`,
/// both with `/` between their segments: the tree Tauri leaves under the
/// bundle's own directory, and the library's path inside that tree. The
/// tree is `Contents` in the macOS bundle and the AppDir on Linux. `None`
/// for Windows, which has no bundle.
fn bundled(system: System, name: &str) -> Option<(&'static str, String)> {
    let (tree, parent) = match system {
        System::Windows => return None,
        System::MacOs => ("Runvil.app/Contents", MACOS_PLUGIN_PARENT),
        System::Linux => ("Runvil.AppDir", APPIMAGE_PLUGIN_PARENT),
    };
    Some((tree, format!("{parent}/{PLUGIN_DIR_NAME}/{name}")))
}

/// The Tauri configuration, as JSON text for `--config`, that copies
/// `library` into Workshop's bundle for `system`. `Ok(None)` for
/// Windows, whose setup installs the library itself.
///
/// The bundler copies these files after Cargo has built Workshop, so the
/// library exists by then. Tauri's `bundle.resources` would not do: the
/// desktop crate's build script checks that each resource exists, and it
/// runs before Cargo has built the library.
pub(crate) fn bundle_config(system: System, library: &Path) -> Result<Option<String>, String> {
    let name = library
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("{} names no file", library.display()))?;
    let Some((_, inside)) = bundled(system, name) else {
        return Ok(None);
    };
    let source = library
        .to_str()
        .ok_or_else(|| format!("{} is not UTF-8", library.display()))?;
    // Tauri takes a macOS path from `Contents`, and an AppImage path from
    // the root of the AppDir, written as an absolute path.
    let files = if system == System::MacOs {
        serde_json::json!({ "macOS": { "files": { inside: source } } })
    } else {
        serde_json::json!({ "linux": { "appimage": { "files": { format!("/{inside}"): source } } } })
    };
    Ok(Some(serde_json::json!({ "bundle": files }).to_string()))
}

/// Checks that the bundle Tauri built for `system` holds the library
/// named `name`. `bundle` is the bundle's own directory under
/// `release/bundle`.
///
/// # Errors
/// Returns the path that is missing. A Tauri CLI that ignored the
/// configuration of [`bundle_config`] would otherwise ship a Workshop
/// with no file tools.
pub(crate) fn check_bundled(system: System, bundle: &Path, name: &str) -> Result<(), String> {
    let Some((tree, inside)) = bundled(system, name) else {
        return Ok(());
    };
    let library = tree
        .split('/')
        .chain(inside.split('/'))
        .fold(bundle.to_path_buf(), |path, segment| path.join(segment));
    if library.is_file() {
        return Ok(());
    }
    Err(format!(
        "the Workshop bundle lacks the std Plugin's library: {} is missing after the Tauri build",
        library.display()
    ))
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
