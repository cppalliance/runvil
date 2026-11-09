//! Installed-peer lookup: where Workshop finds the Gateway it launches.
//!
//! The installers lay the two programs out three ways:
//!
//! - Beside each other in one directory (Windows, development builds).
//! - On macOS, as sibling bundles in one directory: `Runvil.app` and
//!   `Runvil Gateway.app`, each running from `Contents/MacOS/`.
//! - On Linux, as `Runvil.AppImage` beside `runvil-gateway`.
//!   Workshop runs from the AppImage's mount, so it finds the gateway
//!   through `$APPIMAGE`, the path of the AppImage file, when `$APPDIR`
//!   confirms Workshop runs from that AppImage, and looks there
//!   before its own directory: a gateway inside the mount loses its files
//!   when Workshop exits and the mount goes away.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

// `build-workshop` lays the payload out under these names and its tests
// read them from this file, so a rename here fails there.

/// The macOS Workshop bundle's directory name.
pub const WORKSHOP_BUNDLE_NAME: &str = "Runvil.app";

/// The macOS gateway bundle's directory name.
pub const GATEWAY_BUNDLE_NAME: &str = "Runvil Gateway.app";

/// The Linux Workshop AppImage's file name.
pub const WORKSHOP_APPIMAGE_NAME: &str = "Runvil.AppImage";

/// The installer layout a lookup searches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "a build constructs only its own target's layout; tests construct all three"
    )
)]
pub(crate) enum Layout {
    Windows,
    MacOs,
    Linux,
}

impl Layout {
    #[cfg(target_os = "windows")]
    pub(crate) const CURRENT: Self = Self::Windows;
    #[cfg(target_os = "macos")]
    pub(crate) const CURRENT: Self = Self::MacOs;
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    pub(crate) const CURRENT: Self = Self::Linux;

    pub(crate) const fn gateway_exe(self) -> &'static str {
        match self {
            Self::Windows => "runvil-gateway.exe",
            Self::MacOs | Self::Linux => "runvil-gateway",
        }
    }
}

/// The AppImage Workshop runs from, given its `$APPIMAGE` and `$APPDIR`.
///
/// The AppImage runtime sets both: `$APPIMAGE` names the file and `$APPDIR`
/// the mount Workshop's executable runs from. A value counts only when both
/// are absolute and `workshop_exe` sits under `$APPDIR`, so a relative
/// value never resolves against the working directory and a value inherited
/// from another AppImage's environment is ignored. `$APPDIR` is compared
/// both as given and with symlinks resolved: the runtime builds it from the
/// temporary directory unresolved, while the executable path arrives
/// resolved.
#[must_use]
pub fn running_appimage(
    workshop_exe: &Path,
    appimage: Option<&OsStr>,
    appdir: Option<&OsStr>,
) -> Option<PathBuf> {
    let appimage = Path::new(appimage?);
    let appdir = Path::new(appdir?);
    let under = |dir: &Path| workshop_exe.starts_with(dir);
    let inside = appimage.is_absolute()
        && appdir.is_absolute()
        && (under(appdir) || std::fs::canonicalize(appdir).is_ok_and(|dir| under(&dir)));
    inside.then(|| appimage.to_path_buf())
}

/// Locates the installed gateway executable for the Workshop running from
/// `workshop_exe`, inside `appimage` when [`running_appimage`] found one.
/// Checks, in order: on Linux `runvil-gateway` beside the AppImage, then
/// beside `workshop_exe`, then the sibling gateway bundle on macOS.
#[must_use]
pub fn installed_gateway(workshop_exe: &Path, appimage: Option<&Path>) -> Option<PathBuf> {
    first_file(gateway_candidates(Layout::CURRENT, workshop_exe, appimage))
}

fn first_file(candidates: Vec<PathBuf>) -> Option<PathBuf> {
    candidates.into_iter().find(|candidate| runnable(candidate))
}

/// Whether `path` is a regular file this process can run. On Unix that
/// needs an execute bit: a downloaded AppImage starts without one, and
/// launching it would fail.
fn runnable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        metadata.is_file()
    }
}

fn gateway_candidates(
    layout: Layout,
    workshop_exe: &Path,
    appimage: Option<&Path>,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if matches!(layout, Layout::Linux)
        && let Some(dir) = appimage.and_then(Path::parent)
    {
        candidates.push(dir.join(layout.gateway_exe()));
    }
    if let Some(dir) = workshop_exe.parent() {
        candidates.push(dir.join(layout.gateway_exe()));
    }
    if matches!(layout, Layout::MacOs)
        && let Some(dir) = bundle_dir(workshop_exe)
    {
        candidates.push(bundle_exe(dir, GATEWAY_BUNDLE_NAME, layout.gateway_exe()));
    }
    candidates
}

/// The `.app` bundle `exe` runs from, when `exe` sits at
/// `<name>.app/Contents/MacOS/<exe>`, or `None` for an unbundled executable
/// such as a development build. Matches the path shape only and never
/// touches the disk.
fn app_bundle(exe: &Path) -> Option<&Path> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    let is_bundle = macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && bundle.extension()? == "app";
    is_bundle.then_some(bundle)
}

/// The directory holding the `.app` bundle `exe` runs from.
fn bundle_dir(exe: &Path) -> Option<&Path> {
    app_bundle(exe)?.parent()
}

fn bundle_exe(dir: &Path, bundle: &str, exe: &str) -> PathBuf {
    dir.join(bundle).join("Contents").join("MacOS").join(exe)
}

#[cfg(test)]
#[path = "peer-tests.rs"]
mod tests;
