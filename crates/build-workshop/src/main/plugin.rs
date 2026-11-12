//! The libraries of the Plugins Workshop ships, the std Plugin's and the
//! bashkit Plugin's, which Workshop loads from the Plugin directory of
//! its installation.
//!
//! Nothing here builds a library. The Workshop server depends on
//! `workshop-plugin-std` and `workshop-plugin-bashkit`, so Cargo writes
//! each library to `deps` in the profile's directory whenever it builds
//! Workshop, and each mode takes them from there. A build copies them
//! into `plugins` beside Workshop in the target directory. A macOS or
//! Linux installer has Tauri copy them into Workshop's bundle, so the
//! updater, which replaces only that bundle, replaces the libraries with
//! it. The Windows setup takes them from `deps` itself, in
//! `crates/workshop/desktop/installer.nsi`.

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

/// The library of each Plugin Workshop ships, as its `workshop-plugin-*`
/// crate names it: the std Plugin's, which holds the file tools, and the
/// bashkit Plugin's, which holds the shell. `installer.nsi` and the two
/// scripts in `.github/installers` name the same two.
pub(crate) const LIBRARIES: [&str; 2] = ["plugin_std", "plugin_bashkit"];

/// The file name Cargo gives each shipped library for `target`, in the
/// order of [`LIBRARIES`].
pub(crate) fn library_names(target: &str) -> Vec<String> {
    let has = |part: &str| target.split('-').any(|word| word == part);
    LIBRARIES
        .iter()
        .map(|library| {
            if has("windows") {
                format!("{library}.dll")
            } else if has("apple") {
                format!("lib{library}.dylib")
            } else {
                format!("lib{library}.so")
            }
        })
        .collect()
}

/// Where Cargo writes each shipped library when it builds Workshop for
/// `target` into `profile_dir`.
pub(crate) fn built_libraries(profile_dir: &Path, target: &str) -> Vec<PathBuf> {
    library_names(target)
        .into_iter()
        .map(|name| profile_dir.join("deps").join(name))
        .collect()
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

/// The Tauri configuration, as JSON text for `--config`, that copies each
/// of `libraries` into Workshop's bundle for `system`. `Ok(None)` for
/// Windows, whose setup installs the libraries itself.
///
/// The bundler copies these files after Cargo has built Workshop, so the
/// libraries exist by then. Tauri's `bundle.resources` would not do: the
/// desktop crate's build script checks that each resource exists, and it
/// runs before Cargo has built the libraries.
pub(crate) fn bundle_config(
    system: System,
    libraries: &[PathBuf],
) -> Result<Option<String>, String> {
    let mut files = serde_json::Map::new();
    for library in libraries {
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
        // Tauri takes a macOS path from `Contents`, and an AppImage path
        // from the root of the AppDir, written as an absolute path.
        let inside = if system == System::MacOs {
            inside
        } else {
            format!("/{inside}")
        };
        files.insert(inside, source.into());
    }
    let files = if system == System::MacOs {
        serde_json::json!({ "macOS": { "files": files } })
    } else {
        serde_json::json!({ "linux": { "appimage": { "files": files } } })
    };
    Ok(Some(serde_json::json!({ "bundle": files }).to_string()))
}

/// Checks that the bundle Tauri built for `system` holds each library of
/// `names`. `bundle` is the bundle's own directory under `release/bundle`.
///
/// # Errors
/// Returns the first path that is missing. A Tauri CLI that ignored the
/// configuration of [`bundle_config`] would otherwise ship a Workshop
/// with no file tools or no shell.
pub(crate) fn check_bundled(system: System, bundle: &Path, names: &[String]) -> Result<(), String> {
    for name in names {
        let Some((tree, inside)) = bundled(system, name) else {
            return Ok(());
        };
        let library = tree
            .split('/')
            .chain(inside.split('/'))
            .fold(bundle.to_path_buf(), |path, segment| path.join(segment));
        if !library.is_file() {
            return Err(format!(
                "the Workshop bundle lacks a Plugin's library: {} is missing after the Tauri build",
                library.display()
            ));
        }
    }
    Ok(())
}

/// Copies each of `libraries` into the Plugin directory in `beside`,
/// which it makes when it is missing. A library that is missing places
/// none of them.
pub(crate) fn place(libraries: &[PathBuf], beside: &Path) -> Result<(), String> {
    if let Some(missing) = libraries.iter().find(|library| !library.is_file()) {
        return Err(format!(
            "{} is missing; a Workshop build writes it, because the server depends on the \
             `workshop-plugin-*` crate that exports it",
            missing.display()
        ));
    }
    let directory = beside.join(PLUGIN_DIR_NAME);
    fs::create_dir_all(&directory)
        .map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
    for library in libraries {
        let name = library
            .file_name()
            .ok_or_else(|| format!("{} names no file", library.display()))?;
        let placed = directory.join(name);
        // A Workshop from an earlier run may still have the library
        // loaded, and writing into a loaded library's file crashes that
        // process on Linux. A rename replaces the name and leaves that
        // file alone.
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
        })?;
    }
    Ok(())
}
