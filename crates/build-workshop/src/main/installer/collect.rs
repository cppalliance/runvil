//! Moves Tauri's bundle output and places the Gateway (on macOS as its own
//! bundle) into the installer output directory under stable names.
//!
//! `publish/` holds what a release uploads; `payload/` (macOS and Linux)
//! holds what the installer packages install, under their installed names.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::bundle::{self, BundleSources};
use super::{Arch, Gateway, Platform, System, VERSION};

const PRODUCT: &str = "Runvil";
pub(crate) const GATEWAY: &str = "runvil-gateway";

// The installed names Workshop's Gateway lookup searches
// (`crates/workshop/gateway-discovery/src/peer.rs`); a test reads that file
// and requires the same three.
/// The macOS Workshop bundle's directory name.
pub(crate) const WORKSHOP_BUNDLE_NAME: &str = "Runvil.app";
/// The macOS Gateway bundle's directory name.
pub(crate) const GATEWAY_BUNDLE_NAME: &str = "Runvil Gateway.app";
/// The Linux Workshop AppImage's file name.
pub(crate) const WORKSHOP_APPIMAGE_NAME: &str = "Runvil.AppImage";

/// `workspace_root` holds the sources of the macOS Gateway bundle, and
/// `gateway` is the Gateway release the payload carries.
pub(crate) fn collect(
    workspace_root: &Path,
    release: &Path,
    gateway: &Gateway,
    platform: Platform,
    sign: bool,
    output: &Path,
) -> Result<(), String> {
    match fs::remove_dir_all(output) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("cannot clear {}: {error}", output.display())),
    }
    let publish = output.join("publish");
    create_directory(&publish)?;
    let bundle = release.join("bundle").join(platform.bundle_directory());
    match platform.system {
        System::Windows => {
            let setup = single_file(&bundle, "-setup.exe")?;
            let published = publish.join(installer_name(platform));
            if sign {
                move_path(&signature_path(&setup), &signature_path(&published))?;
            }
            move_path(&setup, &published)
        }
        System::MacOs => {
            let payload = output.join("payload");
            create_directory(&payload)?;
            if sign {
                let archive = bundle.join(format!("{PRODUCT}.app.tar.gz"));
                let published = publish.join(workshop_updater_name(platform));
                move_path(&signature_path(&archive), &signature_path(&published))?;
                move_path(&archive, &published)?;
            }
            move_path(
                &bundle.join(format!("{PRODUCT}.app")),
                &payload.join(WORKSHOP_BUNDLE_NAME),
            )?;
            let payload_gateway = payload.join(GATEWAY_BUNDLE_NAME);
            bundle::assemble(
                &BundleSources::in_workspace(workspace_root),
                &gateway.binary,
                &gateway.version,
                &payload_gateway,
            )
        }
        System::Linux => {
            let payload = output.join("payload");
            create_directory(&payload)?;
            let appimage = single_file(&bundle, ".AppImage")?;
            if sign {
                let published = publish.join(workshop_updater_name(platform));
                move_path(&signature_path(&appimage), &signature_path(&published))?;
                copy_file(&appimage, &published)?;
            }
            move_path(&appimage, &payload.join(WORKSHOP_APPIMAGE_NAME))?;
            let payload_gateway = payload.join(GATEWAY);
            copy_executable(&gateway.binary, &payload_gateway)
        }
    }
}

/// The platform's installer as published: Tauri's NSIS setup on Windows,
/// the Qt IFW installer elsewhere, which a DMG wraps on macOS. The
/// collection, the IFW stage, and `cargo workshop manifest` all read the
/// release's names from this function and the two below.
pub(crate) fn installer_name(platform: Platform) -> String {
    let arch = platform.arch;
    match platform.system {
        System::Windows => format!("{PRODUCT}_{VERSION}_{}-setup.exe", windows_arch(arch)),
        System::MacOs => format!("{PRODUCT}_{VERSION}_{}-setup.dmg", arch_name(arch)),
        System::Linux => format!("{PRODUCT}_{VERSION}_{}-setup.run", linux_arch(arch)),
    }
}

/// The file the Tauri updater replaces Workshop from, which on Windows is
/// the setup itself.
pub(crate) fn workshop_updater_name(platform: Platform) -> String {
    let arch = platform.arch;
    match platform.system {
        System::Windows => installer_name(platform),
        System::MacOs => format!("{PRODUCT}_{VERSION}_{}.app.tar.gz", arch_name(arch)),
        System::Linux => format!("{PRODUCT}_{VERSION}_{}.AppImage", linux_arch(arch)),
    }
}

pub(crate) fn signature_path(path: &Path) -> PathBuf {
    let mut signature = OsString::from(path.as_os_str());
    signature.push(".sig");
    PathBuf::from(signature)
}

fn single_file(directory: &Path, suffix: &str) -> Result<PathBuf, String> {
    let entries = fs::read_dir(directory).map_err(|error| {
        format!(
            "cannot read the Tauri bundle directory {}: {error}",
            directory.display()
        )
    })?;
    let mut matches = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|error| format!("cannot read {}: {error}", directory.display()))?
            .path();
        let named = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(suffix));
        if named && path.is_file() {
            matches.push(path);
        }
    }
    match matches.as_slice() {
        [path] => Ok(path.clone()),
        _ => Err(format!(
            "expected exactly one `*{suffix}` in {}, found {}",
            directory.display(),
            matches.len()
        )),
    }
}

fn move_path(source: &Path, destination: &Path) -> Result<(), String> {
    if !source.exists() {
        return Err(format!(
            "{} is missing from the Tauri bundle output",
            source.display()
        ));
    }
    fs::rename(source, destination).map_err(|error| {
        format!(
            "cannot move {} to {}: {error}",
            source.display(),
            destination.display()
        )
    })
}

/// Copies a binary the installer packages install as a program. On Unix
/// the copy gets mode `0755` whatever the source's mode; Windows has no
/// mode to set.
pub(crate) fn copy_executable(source: &Path, destination: &Path) -> Result<(), String> {
    copy_file(source, destination)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(destination, fs::Permissions::from_mode(0o755)).map_err(|error| {
            format!("cannot make {} executable: {error}", destination.display())
        })?;
    }
    Ok(())
}

fn copy_file(source: &Path, destination: &Path) -> Result<(), String> {
    fs::copy(source, destination).map(|_| ()).map_err(|error| {
        format!(
            "cannot copy {} to {}: {error}",
            source.display(),
            destination.display()
        )
    })
}

pub(crate) fn create_directory(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|error| format!("cannot create {}: {error}", path.display()))
}

pub(crate) fn arch_name(arch: Arch) -> &'static str {
    match arch {
        Arch::X86_64 => "x86_64",
        Arch::Aarch64 => "aarch64",
    }
}

/// Tauri's NSIS and AppImage file names use these spellings.
fn windows_arch(arch: Arch) -> &'static str {
    match arch {
        Arch::X86_64 => "x64",
        Arch::Aarch64 => "arm64",
    }
}

fn linux_arch(arch: Arch) -> &'static str {
    match arch {
        Arch::X86_64 => "amd64",
        Arch::Aarch64 => "aarch64",
    }
}
