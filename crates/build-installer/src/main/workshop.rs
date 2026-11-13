//! The runvil-workshop checkout an installer is built from: the check that
//! it agrees with this workspace, the command that builds Workshop's
//! bundle there, and where its desktop crate is.
//!
//! Workshop is its own product in `cppalliance/runvil-workshop`, so
//! nothing here builds it. The checkout's own tool builds the Tauri bundle
//! each installer is made from, as `cargo workshop bundle`.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use super::installer::VERSION;
use super::{BuildEnvironment, CommandSpec, OutputMode};

/// The environment variable naming the root of the checkout. This
/// repository's Cargo configuration sets it to the checkout next to the
/// repository unless it is already set.
pub(super) const WORKSHOP_VARIABLE: &str = "RUNVIL_WORKSHOP";

const TOOLCHAIN_FILE: &str = "rust-toolchain.toml";
const VERSION_KEY: &str = "workspace.package.version";
const CHANNEL_KEY: &str = "toolchain.channel";

/// Requires the checkout to be a directory at this workspace's version
/// and toolchain channel. The release file names and the release tag
/// check use this workspace's version while Tauri names its files with
/// Workshop's, and the Cargo that runs the installer builds Workshop, on
/// this workspace's toolchain.
pub(super) fn check(environment: &BuildEnvironment) -> Result<(), String> {
    let workshop_root = &environment.workshop_root;
    let workspace_root = &environment.workspace_root;
    if !workshop_root.is_dir() {
        return Err(format!(
            "the Workshop checkout {} is not a directory; set {WORKSHOP_VARIABLE} to the root \
             of a runvil-workshop checkout",
            workshop_root.display()
        ));
    }
    let manifest = workshop_root.join("Cargo.toml");
    let version = toml_string(&manifest, VERSION_KEY)?;
    if version != VERSION {
        return Err(format!(
            "the Workshop checkout is not at this workspace's version: {} has {VERSION_KEY} \
             `{version}`, required `{VERSION}`, the version in {}; the release file names use \
             this workspace's version and Tauri names its files with Workshop's",
            manifest.display(),
            workspace_root.join("Cargo.toml").display()
        ));
    }
    let ours = workspace_root.join(TOOLCHAIN_FILE);
    let theirs = workshop_root.join(TOOLCHAIN_FILE);
    let required = toml_string(&ours, CHANNEL_KEY)?;
    let channel = toml_string(&theirs, CHANNEL_KEY)?;
    if channel != required {
        return Err(format!(
            "the Workshop checkout is not on this workspace's toolchain: {} has {CHANNEL_KEY} \
             `{channel}`, required `{required}`, the channel in {}; the Cargo that runs the \
             installer builds Workshop",
            theirs.display(),
            ours.display()
        ));
    }
    Ok(())
}

/// The string at the dotted `key` in the TOML file at `path`.
fn toml_string(path: &Path, key: &str) -> Result<String, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let document: toml::Value = toml::from_str(&text)
        .map_err(|error| format!("{} is not TOML: {error}", path.display()))?;
    key.split('.')
        .try_fold(&document, |value, part| value.get(part))
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("{} has no string at {key}", path.display()))
}

/// `cargo workshop bundle` for `target`, run in the checkout, where Cargo
/// finds the `workshop` alias. `gateway` is the Gateway a flag or the
/// variable named, as an absolute path: the child's working directory is
/// not this process's, so a relative name would point elsewhere.
pub(super) fn bundle_command(
    environment: &BuildEnvironment,
    target: &str,
    sign: bool,
    gateway: Option<&Path>,
) -> CommandSpec {
    let mut args = ["workshop", "bundle", "--target", target]
        .map(OsString::from)
        .to_vec();
    if sign {
        args.push(OsString::from("--sign"));
    }
    if let Some(gateway) = gateway {
        args.push(OsString::from("--gateway-binary"));
        args.push(gateway.as_os_str().to_owned());
    }
    CommandSpec {
        program: environment.cargo.clone(),
        args,
        current_dir: environment.workshop_root.clone(),
        envs: environment.cargo_envs(),
        output_mode: OutputMode::Inherit,
    }
}

/// Workshop's desktop crate in the checkout, which holds the icons,
/// `tauri.conf.json`, and the staged Gateway.
pub(super) fn desktop(workshop_root: &Path) -> PathBuf {
    workshop_root
        .join("crates")
        .join("workshop")
        .join("desktop")
}
