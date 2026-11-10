//! Builds rustdoc JSON for a facade and every crate under its container
//! with default features, and parses it.
//!
//! One `cargo doc` invocation selects the facade and every internal
//! crate, so each internal crate is documented with the features the
//! facade activates. Selecting an internal crate as a root also turns on
//! its `default` feature, which the facade's own graph turns on too,
//! since no internal crate declares one. The JSON lands under
//! `target/xtask-api/`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use rustdoc_types::{Crate, FORMAT_VERSION};

use super::toolchain::PINNED;

/// One facade the command checks.
#[derive(Debug)]
pub(super) struct Facade {
    /// The package `cargo doc -p` takes. Its crate name swaps `-` for `_`.
    pub(super) package: &'static str,
    /// The crate directory, relative to the workspace root, which holds
    /// `public-api.txt`.
    pub(super) dir: [&'static str; 2],
    /// The container under `crates/` whose crates may define a
    /// re-exported item.
    pub(super) container: &'static str,
}

impl Facade {
    /// The crate name, which roots every facade path.
    pub(super) fn crate_name(&self) -> String {
        self.package.replace('-', "_")
    }
}

/// Every facade the command checks, in the order it reports them.
pub(super) const FACADES: [Facade; 1] = [Facade {
    package: "runvil-engine",
    dir: crate::facade_shape::FACADE_DIR,
    container: crate::engine_guards::ENGINE_CONTAINER,
}];

/// The rustdoc flags every build passes. Hidden items are documented
/// because dependents can still name them, and the transport codec is
/// hidden in its defining crate. Lints are capped at warn: documenting
/// hidden items lints docs the workspace docs gate never renders, and
/// link integrity is that gate's to deny - an unresolved link never
/// reaches the JSON `links` table this command reads.
const RUSTDOC_FLAGS: [&str; 6] = [
    "-Zunstable-options",
    "--output-format",
    "json",
    "--document-hidden-items",
    "--cap-lints",
    "warn",
];

/// The parsed rustdoc JSON of a facade and its internal crates.
#[derive(Debug)]
pub(super) struct Loaded {
    /// The facade's crate name.
    pub(super) name: String,
    /// The facade crate.
    pub(super) facade: Crate,
    /// Every internal crate, keyed by its crate name (`runvil_runtime`).
    pub(super) internal: BTreeMap<String, Crate>,
}

/// The package names of every crate under `container`, sorted.
/// A manifest that cannot be read or names no package is an error: a
/// crate the check cannot name cannot be documented or matched.
fn internal_packages(root: &Path, container: &str) -> Result<Vec<String>, String> {
    let container = root.join("crates").join(container);
    let mut dirs = Vec::new();
    crate::engine_guards::collect_crates(&container, &mut dirs);
    let mut packages = Vec::new();
    for dir in dirs {
        let manifest = dir.join("Cargo.toml");
        let name = std::fs::read_to_string(&manifest)
            .ok()
            .and_then(|text| toml::from_str::<toml::Value>(&text).ok())
            .and_then(|value| {
                value
                    .get("package")
                    .and_then(|package| package.get("name"))
                    .and_then(toml::Value::as_str)
                    .map(str::to_owned)
            })
            .ok_or_else(|| format!("{}: no readable package name", manifest.display()))?;
        packages.push(name);
    }
    if packages.is_empty() {
        return Err(format!("{}: no internal crates found", container.display()));
    }
    packages.sort();
    Ok(packages)
}

/// Builds and parses the rustdoc JSON of `facade` and its internal crates
/// in the workspace at `root`.
pub(super) fn load(root: &Path, facade: &Facade) -> Result<Loaded, String> {
    let packages = internal_packages(root, facade.container)?;
    let target = root.join("target").join("xtask-api");
    run_cargo_doc(root, &target, facade.package, &packages)?;
    let doc = target.join("doc");
    let name = facade.crate_name();
    let krate = read_crate(&doc, &name)?;
    let mut internal = BTreeMap::new();
    for package in &packages {
        let name = package.replace('-', "_");
        let krate = read_crate(&doc, &name)?;
        internal.insert(name, krate);
    }
    Ok(Loaded {
        name,
        facade: krate,
        internal,
    })
}

/// A cargo command in `root` on the pinned nightly: the cargo that runs
/// this process when there is one, with rustup's proxies told the pinned
/// toolchain so the rustc and rustdoc it spawns match.
pub(super) fn cargo(root: &Path) -> Command {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command
        .current_dir(root)
        .env("RUSTUP_TOOLCHAIN", PINNED.nightly);
    command
}

fn run_cargo_doc(
    root: &Path,
    target: &Path,
    facade: &str,
    packages: &[String],
) -> Result<(), String> {
    let mut command = cargo(root);
    command
        .env("CARGO_ENCODED_RUSTDOCFLAGS", RUSTDOC_FLAGS.join("\u{1f}"))
        .env_remove("RUSTDOCFLAGS")
        .args(["doc", "--locked", "--no-deps", "--lib", "--target-dir"])
        .arg(target)
        .args(["-p", facade]);
    for package in packages {
        command.args(["-p", package]);
    }
    let output = command
        .output()
        .map_err(|error| format!("cannot run cargo: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let tail: Vec<&str> = stderr.lines().rev().take(40).collect();
    Err(format!(
        "`cargo doc` for rustdoc JSON failed ({}):\n{}",
        output.status,
        tail.into_iter().rev().collect::<Vec<_>>().join("\n")
    ))
}

/// Parses `<doc>/<name>.json`. A file in another `format_version` is
/// reported as that, naming the pinned nightly, rather than as whatever
/// field the parse tripped on first.
fn read_crate(doc: &Path, name: &str) -> Result<Crate, String> {
    let path: PathBuf = doc.join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("{}: unreadable rustdoc JSON: {error}", path.display()))?;
    let wrong_version = |found: u64| {
        format!(
            "{}: required rustdoc JSON format_version {FORMAT_VERSION} (from `{}`, read by \
             rustdoc-types {}), found {found}",
            path.display(),
            PINNED.nightly,
            PINNED.rustdoc_types
        )
    };
    let krate = serde_json::from_str::<Crate>(&text).map_err(|error| {
        let version = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|value| {
                value
                    .get("format_version")
                    .and_then(serde_json::Value::as_u64)
            });
        match version {
            Some(found) if found != u64::from(FORMAT_VERSION) => wrong_version(found),
            _ => format!("{}: unparseable rustdoc JSON: {error}", path.display()),
        }
    })?;
    if krate.format_version != FORMAT_VERSION {
        return Err(wrong_version(krate.format_version.into()));
    }
    Ok(krate)
}

#[cfg(test)]
#[path = "load-tests.rs"]
mod tests;
