//! The Gateway a build ships, and the sidecar slot Tauri's `externalBin`
//! bundles it from for Windows targets (`tauri.windows.conf.json`). macOS
//! and Linux ship the Gateway beside Workshop instead of inside it.
//!
//! The Gateway is its own product in `cppalliance/runvil-gateway`, so
//! nothing here compiles it. It comes from `--gateway-binary`, then the
//! `RUNVIL_GATEWAY_SIDECAR` environment variable, then the slot, where
//! `tools/fetch-gateway-sidecar.mjs` stages the release
//! `gateway-pin/pin.json` names for any target.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The environment variable naming a Gateway binary, so a CI job can point
/// a build at one without passing a flag. The desktop build script reads it
/// too.
pub(super) const GATEWAY_SIDECAR_VARIABLE: &str = "RUNVIL_GATEWAY_SIDECAR";

/// Where this run's Gateway comes from.
#[derive(Debug, Eq, PartialEq)]
pub(super) enum GatewaySource {
    /// A binary `--gateway-binary` or `RUNVIL_GATEWAY_SIDECAR` named. A
    /// Windows build stages it in the slot and removes it afterward.
    Given(PathBuf),
    /// The binary already in the slot, used in place and left there, so a
    /// downloaded Gateway serves every later build. `stub` is true when
    /// `tools/fetch-gateway-sidecar.mjs --stub` staged a compile-only file.
    Staged { stub: bool },
}

impl GatewaySource {
    /// The Gateway executable this source names for `target`.
    pub(super) fn binary(&self, workspace_root: &Path, target: &str) -> PathBuf {
        match self {
            Self::Given(path) => path.clone(),
            Self::Staged { .. } => sidecar_path(workspace_root, target),
        }
    }

    /// The environment a Workshop build gets for this source. The desktop
    /// build script copies the file `RUNVIL_GATEWAY_SIDECAR` names into the
    /// sidecar slot, and a child inherits that variable, so a given Gateway
    /// is named there too: otherwise an inherited value would replace the
    /// `--gateway-binary` file this run checked and staged.
    pub(super) fn child_envs(&self) -> Vec<(OsString, OsString)> {
        match self {
            Self::Given(path) => vec![(
                OsString::from(GATEWAY_SIDECAR_VARIABLE),
                path.clone().into_os_string(),
            )],
            Self::Staged { .. } => Vec::new(),
        }
    }
}

/// Chooses the Gateway for `target`: `given` when a flag or the variable
/// named one, else the staged slot. With neither, the error names the fetch
/// script.
pub(super) fn gateway_source(
    given: Option<&Path>,
    workspace_root: &Path,
    target: &str,
) -> Result<GatewaySource, String> {
    if let Some(path) = given {
        // One absolute path serves every later use: the version check, the
        // copies, and the desktop build script each run from a different
        // directory, and a bare name would otherwise resolve through PATH.
        let path = std::path::absolute(path)
            .map_err(|error| format!("cannot resolve the Gateway {}: {error}", path.display()))?;
        if !path.is_file() {
            return Err(format!(
                "--gateway-binary or {GATEWAY_SIDECAR_VARIABLE} names the Gateway {}, which is \
                 not a file",
                path.display()
            ));
        }
        return Ok(GatewaySource::Given(path));
    }
    let staged = sidecar_path(workspace_root, target);
    if !staged.is_file() {
        return Err(format!(
            "no Gateway for {target}: nothing is staged at {} and neither --gateway-binary nor \
             {GATEWAY_SIDECAR_VARIABLE} names one; the Gateway is a separate product, so run \
             `node tools/fetch-gateway-sidecar.mjs --target {target}` to stage the release \
             gateway-pin/pin.json names",
            staged.display()
        ));
    }
    Ok(GatewaySource::Staged {
        stub: marker_says_stub(&staged),
    })
}

/// Whether the fetch script's marker beside `staged` records a stub. A
/// missing or unreadable marker means some other step staged the file, and
/// it counts as a real Gateway.
fn marker_says_stub(staged: &Path) -> bool {
    let mut marker = staged.as_os_str().to_owned();
    marker.push(".pinned");
    let Ok(text) = fs::read_to_string(PathBuf::from(marker)) else {
        return false;
    };
    // The marker is the fetch script's JSON; a substring check is enough.
    text.contains("\"kind\": \"stub\"") || text.contains("\"kind\":\"stub\"")
}

/// The Gateway version `gateway-pin/pin.json` names, the one every
/// installer ships.
pub(super) fn pinned_version(workspace_root: &Path) -> Result<String, String> {
    let pin = workspace_root.join("gateway-pin").join("pin.json");
    let text = fs::read_to_string(&pin)
        .map_err(|error| format!("cannot read the Gateway pin {}: {error}", pin.display()))?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| format!("the Gateway pin {} is not JSON: {error}", pin.display()))?;
    match value.get("version").and_then(serde_json::Value::as_str) {
        Some(version) if !version.is_empty() && version != "PLACEHOLDER" => Ok(version.to_owned()),
        _ => Err(format!(
            "the Gateway pin {} names no version; run `node tools/bump-gateway.mjs <tag>` to \
             fill it from a published Gateway release",
            pin.display()
        )),
    }
}

pub(super) fn gateway_binary_name(target: &str) -> &'static str {
    if is_windows(target) {
        "runvil-gateway.exe"
    } else {
        "runvil-gateway"
    }
}

/// Whether Workshop's bundle for `target` carries the Gateway sidecar.
pub(super) fn bundles_sidecar(target: &str) -> bool {
    is_windows(target)
}

/// Tauri resolves `externalBin` entries with the target triple appended to
/// the file stem.
pub(super) fn sidecar_path(workspace_root: &Path, target: &str) -> PathBuf {
    let extension = if is_windows(target) { ".exe" } else { "" };
    workspace_root
        .join("crates")
        .join("workshop")
        .join("desktop")
        .join("binaries")
        .join(format!("runvil-gateway-{target}{extension}"))
}

pub(super) fn stage(workspace_root: &Path, target: &str, source: &Path) -> Result<PathBuf, String> {
    let metadata = match fs::symlink_metadata(source) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(format!(
                "Gateway source binary does not exist: {}",
                source.display()
            ));
        }
        Err(error) => {
            return Err(format!(
                "Gateway source binary {} cannot be read: {error}",
                source.display()
            ));
        }
    };
    if !metadata.is_file() {
        return Err(format!(
            "Gateway source binary is not a file: {}",
            source.display()
        ));
    }
    let expected = gateway_binary_name(target);
    if source.file_name().and_then(|name| name.to_str()) != Some(expected) {
        return Err(format!(
            "Gateway source binary must be named {expected} for {target}, got {}",
            source.display()
        ));
    }
    let destination = sidecar_path(workspace_root, target);
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    fs::copy(source, &destination).map_err(|error| {
        format!(
            "cannot copy {} to {}: {error}",
            source.display(),
            destination.display()
        )
    })?;
    Ok(destination)
}

/// Removing a sidecar that was never staged succeeds, so cleanup can run
/// after any failure.
pub(super) fn remove(workspace_root: &Path, target: &str) -> Result<PathBuf, String> {
    let path = sidecar_path(workspace_root, target);
    match fs::remove_file(&path) {
        Ok(()) => Ok(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(path),
        Err(error) => Err(format!("cannot remove {}: {error}", path.display())),
    }
}

fn is_windows(target: &str) -> bool {
    target.split('-').any(|part| part == "windows")
}
