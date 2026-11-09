//! Assembles `Runvil Gateway.app`, the macOS Gateway payload, around
//! the pinned Gateway binary: the bundle's `Info.plist` template with the
//! Gateway's version filled in, and Workshop's icon.

use std::fs;
use std::path::{Path, PathBuf};

use super::collect::{GATEWAY, copy_executable, create_directory};

/// Stands for the Gateway's version in the template's two version keys.
const VERSION_PLACEHOLDER: &str = "@VERSION@";

/// The checked-in files the bundle is assembled from.
#[derive(Debug)]
pub(crate) struct BundleSources {
    pub(crate) info_plist: PathBuf,
    pub(crate) icon: PathBuf,
}

impl BundleSources {
    pub(crate) fn in_workspace(workspace_root: &Path) -> Self {
        let crates = workspace_root.join("crates");
        Self {
            info_plist: crates
                .join("build-workshop")
                .join("packaging")
                .join("gateway-Info.plist"),
            icon: crates
                .join("workshop")
                .join("desktop")
                .join("icons")
                .join("icon.icns"),
        }
    }
}

/// Writes the bundle at `bundle`, which must not exist yet:
/// `Contents/Info.plist` stating `version`, `Contents/MacOS/runvil-gateway`,
/// and `Contents/Resources/icon.icns`, the file `CFBundleIconFile` names.
pub(crate) fn assemble(
    sources: &BundleSources,
    gateway: &Path,
    version: &str,
    bundle: &Path,
) -> Result<(), String> {
    let template = fs::read_to_string(&sources.info_plist).map_err(|error| {
        format!(
            "cannot read the Gateway bundle's Info.plist template {}: {error}",
            sources.info_plist.display()
        )
    })?;
    if !template.contains(VERSION_PLACEHOLDER) {
        return Err(format!(
            "the Gateway bundle's Info.plist template {} has no {VERSION_PLACEHOLDER} \
             placeholder for the version",
            sources.info_plist.display()
        ));
    }
    if !sources.icon.is_file() {
        return Err(format!(
            "the Gateway bundle's icon {} is missing",
            sources.icon.display()
        ));
    }
    let contents = bundle.join("Contents");
    let executables = contents.join("MacOS");
    let resources = contents.join("Resources");
    create_directory(&executables)?;
    create_directory(&resources)?;
    let info_plist = contents.join("Info.plist");
    fs::write(&info_plist, template.replace(VERSION_PLACEHOLDER, version))
        .map_err(|error| format!("cannot write {}: {error}", info_plist.display()))?;
    copy_executable(gateway, &executables.join(GATEWAY))?;
    let icon = resources.join("icon.icns");
    fs::copy(&sources.icon, &icon).map(|_| ()).map_err(|error| {
        format!(
            "cannot copy {} to {}: {error}",
            sources.icon.display(),
            icon.display()
        )
    })
}
