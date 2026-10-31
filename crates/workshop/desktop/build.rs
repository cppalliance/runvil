//! Build script for the workshop desktop app: puts the Gateway sidecar in
//! the slot tauri-build copies from, then runs tauri-build so
//! `tauri::generate_context!` sees the config-derived environment it
//! requires.
//!
//! Why a sidecar step exists at all: `tauri.conf.json` declares the Gateway
//! as an `externalBin`, so tauri-build copies `binaries/runvil-gateway-
//! <target-triple>` over `target/<profile>/runvil-gateway` on every
//! workshop build. That is the same path cargo writes the Gateway crate's
//! own binary to, so without a step here a `cargo build -p gateway`
//! followed by `cargo build -p workshop` ends with the stale sidecar copy
//! in place of the Gateway just built.
//!
//! Three sources fill that slot, in order:
//!
//! 1. `RUNVIL_GATEWAY_SIDECAR`, an explicit file some other step staged.
//!    CI sets it, and so does `cargo workshop` when it has a downloaded or
//!    locally built Gateway. The named file is copied in as is.
//! 2. The workspace's own `target/<profile>/runvil-gateway`, copied
//!    forward when it is newer than the sidecar already there. This keeps
//!    the plain `cargo build -p gateway` then `cargo build -p workshop`
//!    order correct.
//! 3. Whatever `tools/fetch-gateway-sidecar.mjs` left in place: a stub for
//!    compile-only jobs or the pinned release binary. Nothing here touches
//!    it, so a downloaded Gateway survives every rebuild.

use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

/// The variable naming a sidecar to install, shared with `build-workshop`.
const GATEWAY_SIDECAR_VARIABLE: &str = "RUNVIL_GATEWAY_SIDECAR";

fn main() -> Result<(), Box<dyn Error>> {
    build_ceiling::check()?;
    install_gateway_sidecar()?;
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&["desktop_update_supported", "quit"]),
    ))?;
    Ok(())
}

/// Puts the sidecar where `bundle.externalBin` resolves it: an explicit
/// `RUNVIL_GATEWAY_SIDECAR` file when one is named, else the newest
/// workspace-built Gateway, else what is already staged.
fn install_gateway_sidecar() -> Result<(), Box<dyn Error>> {
    let target = env::var("TARGET")?;
    let exe_suffix = if env::var("CARGO_CFG_TARGET_OS")? == "windows" {
        ".exe"
    } else {
        ""
    };
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let sidecar = manifest_dir
        .join("binaries")
        .join(format!("runvil-gateway-{target}{exe_suffix}"));

    println!("cargo:rerun-if-env-changed={GATEWAY_SIDECAR_VARIABLE}");
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

    let Some(profile_dir) = profile_dir_from_out_dir(&PathBuf::from(env::var("OUT_DIR")?)) else {
        return Ok(());
    };
    let built = profile_dir.join(format!("runvil-gateway{exe_suffix}"));
    // A rebuilt gateway must re-run this script; a missing one is not an
    // error here because tauri-build reports the absent sidecar itself.
    println!("cargo:rerun-if-changed={}", built.display());
    // `tools/fetch-gateway-sidecar.mjs` marks what it staged: a release
    // binary it verified against the pin, or a compile-only stub. That mark
    // outranks any workspace build, because a stale
    // `target/<profile>/runvil-gateway` from an earlier day must not
    // silently replace a downloaded release.
    if sidecar_is_pinned(&sidecar) {
        return Ok(());
    }
    if !built.is_file() || sidecar_is_current(&built, &sidecar) {
        return Ok(());
    }
    copy_forward(&built, &sidecar)?;
    Ok(())
}

/// Copies `source` over `sidecar`, creating the directory and reporting
/// the copy so a surprising sidecar is visible in the build log.
fn copy_forward(source: &Path, sidecar: &Path) -> std::io::Result<()> {
    if let Some(parent) = sidecar.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(source, sidecar)?;
    println!(
        "cargo:warning=installed gateway sidecar {} from {}",
        sidecar.display(),
        source.display()
    );
    Ok(())
}

/// `OUT_DIR` is `<target>/<profile>/build/<pkg-hash>/out`; the profile
/// directory holding the built binaries is three levels up.
fn profile_dir_from_out_dir(out_dir: &Path) -> Option<PathBuf> {
    out_dir.ancestors().nth(3).map(Path::to_path_buf)
}

/// Whether `tools/fetch-gateway-sidecar.mjs` staged the sidecar and said
/// so: the file is then the authority, whatever a workspace build wrote.
fn sidecar_is_pinned(sidecar: &Path) -> bool {
    let mut marker = sidecar.as_os_str().to_owned();
    marker.push(".pinned");
    Path::new(&marker).is_file()
}

/// The sidecar is current when it exists with the same length and a
/// modification time no older than the built gateway's.
fn sidecar_is_current(built: &Path, sidecar: &Path) -> bool {
    let (Ok(built_meta), Ok(sidecar_meta)) = (fs::metadata(built), fs::metadata(sidecar)) else {
        return false;
    };
    if built_meta.len() != sidecar_meta.len() {
        return false;
    }
    match (built_meta.modified(), sidecar_meta.modified()) {
        (Ok(built_time), Ok(sidecar_time)) => sidecar_time >= built_time,
        _ => false,
    }
}
