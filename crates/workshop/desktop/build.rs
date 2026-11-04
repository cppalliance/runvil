//! Build script for the workshop desktop app: puts the Gateway sidecar in
//! the slot tauri-build copies from, then runs tauri-build so
//! `tauri::generate_context!` sees the config-derived environment it
//! requires.
//!
//! On Windows, `tauri.windows.conf.json` declares the Gateway as an
//! `externalBin`, so on every workshop build tauri-build copies
//! `binaries/runvil-gateway-<target-triple>.exe` to
//! `target/<profile>/runvil-gateway.exe`. The sidecar step decides what that
//! slot holds before tauri-build reads it, and leaves an empty slot for
//! tauri-build to report.
//!
//! Two sources fill the slot, in order:
//!
//! 1. `RUNVIL_GATEWAY_SIDECAR`, an explicit file some other step staged.
//!    `cargo workshop` sets it to the Gateway `--gateway-binary` or the
//!    variable named. The named file is copied in as is.
//! 2. Whatever `tools/fetch-gateway-sidecar.mjs` left in place: a stub for
//!    compile-only jobs or the pinned release binary. Nothing here touches
//!    it, so a downloaded Gateway survives every rebuild.
//!
//! macOS and Linux bundle no Gateway: their installers place it beside
//! Workshop, so those targets skip the step.

use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

/// The variable naming a sidecar to install, shared with `build-workshop`.
const GATEWAY_SIDECAR_VARIABLE: &str = "RUNVIL_GATEWAY_SIDECAR";

fn main() -> Result<(), Box<dyn Error>> {
    install_gateway_sidecar()?;
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&["desktop_update_supported", "quit"]),
    ))?;
    Ok(())
}

/// Puts the sidecar where `bundle.externalBin` resolves it: an explicit
/// `RUNVIL_GATEWAY_SIDECAR` file when one is named, else what is already
/// staged, with a warning when the staged file is the stub.
fn install_gateway_sidecar() -> Result<(), Box<dyn Error>> {
    if env::var("CARGO_CFG_TARGET_OS")? != "windows" {
        return Ok(());
    }
    let target = env::var("TARGET")?;
    let exe_suffix = ".exe";
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let sidecar = manifest_dir
        .join("binaries")
        .join(format!("runvil-gateway-{target}{exe_suffix}"));

    println!("cargo:rerun-if-env-changed={GATEWAY_SIDECAR_VARIABLE}");
    // The marker is written only by `tools/fetch-gateway-sidecar.mjs`, never
    // here, so watching it cannot loop. Watching it means re-staging a
    // release over a stub re-runs this script and the warning below clears.
    println!("cargo:rerun-if-changed={}", pin_marker(&sidecar).display());
    if let Some(named) = env::var_os(GATEWAY_SIDECAR_VARIABLE) {
        let named = PathBuf::from(named);
        if named.as_os_str().is_empty() {
            return Err(format!("{GATEWAY_SIDECAR_VARIABLE} names no file").into());
        }
        if !named.is_file() {
            return Err(format!(
                "{GATEWAY_SIDECAR_VARIABLE} names {}, which is not a file",
                named.display()
            )
            .into());
        }
        copy_forward(&named, &sidecar)?;
        return Ok(());
    }

    if sidecar_is_stub(&sidecar) {
        // CI stages a stub for compile-only jobs, so a build that copies
        // one is expected there. A person building locally gets the same
        // warning, which is the last chance to notice before an app that
        // cannot start exists on disk.
        println!(
            "cargo:warning={} is a compile-only stub Gateway and cannot run; \
             run `node tools/fetch-gateway-sidecar.mjs --target {}` and rebuild for a \
             Gateway that starts",
            sidecar.display(),
            target
        );
    }
    Ok(())
}

/// Copies `source` over `sidecar`, creating the directory and reporting
/// the copy so a surprising sidecar is visible in the build log.
fn copy_forward(source: &Path, sidecar: &Path) -> Result<(), String> {
    // `cargo workshop` stages the file it names here before this script
    // runs. Copying it again would fail on Windows when the file is
    // read-only, since the first copy carries that attribute to the slot:
    // https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-copyfilew
    if fs::read(sidecar).is_ok_and(|staged| fs::read(source).is_ok_and(|named| named == staged)) {
        return Ok(());
    }
    if let Some(parent) = sidecar.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    fs::copy(source, sidecar).map_err(|e| {
        format!(
            "copy the gateway sidecar {} to {}: {e}",
            source.display(),
            sidecar.display()
        )
    })?;
    println!(
        "cargo:warning=installed gateway sidecar {} from {}",
        sidecar.display(),
        source.display()
    );
    Ok(())
}

/// The marker beside the sidecar: `<sidecar>.pinned`.
fn pin_marker(sidecar: &Path) -> PathBuf {
    let mut marker = sidecar.as_os_str().to_owned();
    marker.push(".pinned");
    PathBuf::from(marker)
}

/// Whether the marker says the staged sidecar is the compile-only stub
/// rather than the pinned release. The marker is the fetch script's JSON,
/// so a substring check keeps this script free of a JSON dependency. A
/// missing or malformed marker reads as unknown, and unknown is not a stub:
/// the app's own boot check still refuses to run one.
fn sidecar_is_stub(sidecar: &Path) -> bool {
    let Ok(text) = fs::read_to_string(pin_marker(sidecar)) else {
        return false;
    };
    text.contains("\"kind\": \"stub\"") || text.contains("\"kind\":\"stub\"")
}
