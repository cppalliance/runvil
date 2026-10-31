//! Embeds the Gateway pin's `min_gateway` so Workshop can compare it with
//! the version of the Gateway it attaches to, then runs the compile-time
//! file-size check.
//!
//! The pin is one file for the whole repository, and this crate is where
//! the Gateway version is known, so the value is baked into this crate's
//! environment and read back with `env!`.

use std::path::PathBuf;

fn main() -> Result<(), build_ceiling::Violations> {
    embed_min_gateway();
    build_ceiling::check()
}

/// Reads `gateway-pin/pin.json` and sets `RUNVIL_MIN_GATEWAY` to its
/// `min_gateway` value. A pin that is missing or still a placeholder sets
/// an empty value, which the comparison reads as "no minimum recorded": a
/// fresh checkout has to build before anyone can fill the pin in.
fn embed_min_gateway() {
    let pin = pin_path();
    if let Some(path) = &pin {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    let value = pin
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|pin| pin.get("min_gateway")?.as_str().map(str::to_owned))
        .filter(|value| value != "PLACEHOLDER")
        .unwrap_or_default();
    println!("cargo:rustc-env=RUNVIL_MIN_GATEWAY={value}");
}

/// `<workspace>/gateway-pin/pin.json`, from this crate's manifest directory.
fn pin_path() -> Option<PathBuf> {
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR")?);
    let root = manifest.parent()?.parent()?.parent()?;
    Some(root.join("gateway-pin").join("pin.json"))
}
