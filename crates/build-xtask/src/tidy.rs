//! Tidy-style structural checks over every workspace crate: the shared
//! crate walk's read failures and lint inheritance, which are checked
//! here, and the unsafe allowlist (run from `unsafe_allowlist`), the
//! product boundary (run from `product`), the doctest ban (run from
//! `no_doctests`), the `Debug` trait's one spelling (run from
//! `debug_spelling`), and the type-size limits (run from `type_size`).
//!
//! Each check returns a list of human-readable violations. The `#[test]`
//! wrappers assert the lists are empty, so `cargo test -p build-xtask`
//! enforces the checks; `cargo xtask tidy` prints the same report on
//! demand. The shared walk's read failures are reported by
//! `crate_walk_violations`, their one owner.
//!
//! The helpers the checks share for finding and naming source files,
//! `rust_files` and `slash_path`, are here too.

use std::fs;
use std::path::{Path, PathBuf};

/// Runs every check and returns all violations.
#[must_use]
pub(crate) fn all_violations(root: &Path) -> Vec<String> {
    let mut violations = crate_walk_violations(root);
    violations.extend(lint_inheritance_violations(root));
    violations.extend(crate::unsafe_allowlist::unsafe_allowlist_violations(root));
    violations.extend(crate::product::product_boundary_violations(root));
    violations.extend(crate::no_doctests::no_doctests_violations(root));
    violations.extend(crate::debug_spelling::debug_spelling_violations(root));
    violations.extend(crate::type_size::type_size_violations(root));
    violations
}

/// The shared crate walk's read failures.
///
/// A manifest the walk could not read, parse, or find a package name in is
/// reported as itself, not skipped: the product-boundary check binds a
/// crate by its package name, so a crate with no readable name cannot be
/// shown to pass it. This check is the one owner of the walk's read
/// failures; every other check shares the walk and leaves them here.
#[must_use]
fn crate_walk_violations(root: &Path) -> Vec<String> {
    crate::product::workspace_crates(root).violations
}

/// Checks that every workspace crate, unread ones included, inherits
/// `[lints] workspace = true` (which includes `unreachable_pub`) and that
/// the workspace root sets it. A walk that found no crate fails: a crate
/// that was never read cannot be shown to inherit.
#[must_use]
fn lint_inheritance_violations(root: &Path) -> Vec<String> {
    let mut violations = Vec::new();
    let root_manifest = root.join("Cargo.toml");
    match fs::read_to_string(&root_manifest)
        .ok()
        .and_then(|text| toml::from_str::<toml::Value>(&text).ok())
    {
        Some(manifest) => {
            let set = manifest
                .get("workspace")
                .and_then(|w| w.get("lints"))
                .and_then(|l| l.get("rust"))
                .and_then(|r| r.get("unreachable_pub"));
            if set.is_none() {
                violations.push(
                    "workspace root does not set `unreachable_pub` in [workspace.lints.rust]"
                        .to_owned(),
                );
            }
        }
        None => violations.push(format!("{}: unparseable manifest", root_manifest.display())),
    }
    let walk = crate::product::workspace_crates(root);
    let dirs: Vec<&PathBuf> = walk
        .crates
        .iter()
        .map(|krate| &krate.dir)
        .chain(&walk.unread)
        .collect();
    if dirs.is_empty() {
        violations.push(format!(
            "{}: the lint inheritance check scanned nothing: no workspace crate was found, \
             so none can be shown to inherit `[lints] workspace = true`",
            root.join("crates").display()
        ));
    }
    for dir in dirs {
        let manifest_path = root.join(dir).join("Cargo.toml");
        let inherits = fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|text| toml::from_str::<toml::Value>(&text).ok())
            .and_then(|manifest| {
                manifest
                    .get("lints")
                    .and_then(|l| l.get("workspace"))
                    .and_then(toml::Value::as_bool)
            })
            .unwrap_or(false);
        if !inherits {
            violations.push(format!(
                "{} does not inherit `[lints] workspace = true`",
                manifest_path.display()
            ));
        }
    }
    violations
}

/// A file's path relative to the workspace root with `/` separators, so
/// the allowlist and the violations read the same on every platform.
///
/// A component that is not valid UTF-8 is rendered lossily rather than
/// dropped: dropping it would shorten the path and could make it match an
/// allowlist entry, exempting a file the rule should report.
pub(crate) fn slash_path(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Every `.rs` file under `dir`, recursively.
pub(crate) fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rust_files(dir, &mut files);
    files
}

fn collect_rust_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if entry.file_name() != "target" {
                collect_rust_files(&path, files);
            }
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
}

#[cfg(test)]
#[path = "tidy-tests.rs"]
mod tests;
