//! The Gateway an installer ships. A Windows installer is Workshop's NSIS
//! setup, which already holds the Gateway, so only a macOS or Linux
//! payload takes one from here, to install beside Workshop. A Gateway
//! named for the build is passed on to the bundle step on every platform.
//!
//! The Gateway is its own product in `cppalliance/runvil-gateway`, so
//! nothing here compiles it. It comes from `--gateway-binary`, then the
//! `RUNVIL_GATEWAY_SIDECAR` environment variable, then the file the
//! Workshop checkout's `tools/fetch-gateway-sidecar.mjs` stages there: the
//! release that checkout's `gateway-pin/pin.json` names.

use std::fs;
use std::path::{Path, PathBuf};

use super::workshop;

/// The environment variable naming a Gateway binary, so a CI job can point
/// a build at one without passing a flag. `cargo workshop bundle` reads it
/// too.
pub(super) const GATEWAY_SIDECAR_VARIABLE: &str = "RUNVIL_GATEWAY_SIDECAR";

/// The Gateway `given` names, when a flag or the variable named one, as an
/// absolute path to a file.
pub(super) fn named(given: Option<&Path>) -> Result<Option<PathBuf>, String> {
    let Some(path) = given else {
        return Ok(None);
    };
    // One absolute path serves every later use: the bundle step runs from
    // the Workshop checkout and the version check from this repository,
    // and a bare name would otherwise resolve through PATH.
    let path = std::path::absolute(path)
        .map_err(|error| format!("cannot resolve the Gateway {}: {error}", path.display()))?;
    if !path.is_file() {
        return Err(format!(
            "--gateway-binary or {GATEWAY_SIDECAR_VARIABLE} names the Gateway {}, which is \
             not a file",
            path.display()
        ));
    }
    Ok(Some(path))
}

/// The executable a macOS or Linux payload for `target` holds: `named`,
/// else the one staged in the Workshop checkout. With neither, the error
/// names the fetch script.
pub(super) fn payload_binary(
    named: Option<&Path>,
    workshop_root: &Path,
    target: &str,
) -> Result<PathBuf, String> {
    if let Some(path) = named {
        return Ok(path.to_path_buf());
    }
    let staged = staged_path(workshop_root, target);
    if !staged.is_file() {
        return Err(format!(
            "no Gateway for {target}: nothing is staged at {} and neither --gateway-binary nor \
             {GATEWAY_SIDECAR_VARIABLE} names one; the Gateway is a separate product, so run \
             `node tools/fetch-gateway-sidecar.mjs --target {target}` in the Workshop checkout \
             {} to stage the release its gateway-pin/pin.json names",
            staged.display(),
            workshop_root.display()
        ));
    }
    Ok(staged)
}

/// The Gateway version the Workshop checkout's `gateway-pin/pin.json`
/// names, the one every installer ships.
pub(super) fn pinned_version(workshop_root: &Path) -> Result<String, String> {
    let pin = workshop_root.join("gateway-pin").join("pin.json");
    let text = fs::read_to_string(&pin)
        .map_err(|error| format!("cannot read the Gateway pin {}: {error}", pin.display()))?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| format!("the Gateway pin {} is not JSON: {error}", pin.display()))?;
    match value.get("version").and_then(serde_json::Value::as_str) {
        Some(version) if !version.is_empty() && version != "PLACEHOLDER" => Ok(version.to_owned()),
        _ => Err(format!(
            "the Gateway pin {} names no version; run `node tools/bump-gateway.mjs <tag>` in \
             the Workshop checkout to fill it from a published Gateway release",
            pin.display()
        )),
    }
}

/// Where the fetch script stages the Gateway for a macOS or Linux
/// `target`: Tauri's sidecar name, the target triple appended to the file
/// stem.
pub(super) fn staged_path(workshop_root: &Path, target: &str) -> PathBuf {
    workshop::desktop(workshop_root)
        .join("binaries")
        .join(format!("runvil-gateway-{target}"))
}
