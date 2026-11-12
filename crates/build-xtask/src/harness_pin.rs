//! The pin on the crates this workspace takes from runvil-harness, the
//! repository that holds the Engine and the Harness. Those crates arrive
//! as git dependencies, which the product matrix cannot see, so this check
//! reads the manifests instead: crates here reach the Engine and the
//! Harness only through the public crates in [`HARNESS_PUBLIC`], all at
//! one commit. Runs as part of `cargo test -p build-xtask` and
//! `cargo xtask tidy`.
//!
//! The rules:
//!
//! 1. An entry of the root `[workspace.dependencies]` that comes from the
//!    repository names one of the four public crates, by its `package`
//!    field when it has one and by its key otherwise.
//! 2. Each such entry pins a `rev`; a `branch` or a `tag` does not count.
//! 3. The entries name one `rev` between them, because two commits are two
//!    copies of every type the crates share.
//! 4. No workspace member names the repository in its own manifest, in any
//!    dependency table, the target-specific ones included: a member takes
//!    these crates with `<dep>.workspace = true`.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// The repository that holds the Engine and the Harness.
const HARNESS_REPOSITORY: &str = "https://github.com/cppalliance/runvil-harness";

/// The four crates of that repository a crate here may depend on: the
/// Harness facade, which is the one crate a Host depends on, the standard
/// Gateway client, and the two Rust Plugins Workshop links in.
const HARNESS_PUBLIC: [&str; 4] = [
    "runvil",
    "harness-gateway-client",
    "plugin-web",
    "plugin-mcp",
];

/// Runs the four rules over the root manifest's `[workspace.dependencies]`
/// and every workspace member's manifest. A root manifest that cannot be
/// read or parsed is reported, not skipped: a table that was never read
/// cannot be shown pinned.
#[must_use]
pub(crate) fn harness_pin_violations(root: &Path) -> Vec<String> {
    let mut violations = workspace_violations(&root.join("Cargo.toml"));
    violations.extend(member_violations(root));
    violations
}

/// The breaches of rules 1 to 3 in the `[workspace.dependencies]` of the
/// root manifest at `path`, or the one reason the manifest could not be
/// read.
fn workspace_violations(path: &Path) -> Vec<String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => {
            return vec![format!("{}: unreadable manifest: {error}", path.display())];
        }
    };
    let manifest: toml::Value = match toml::from_str(&text) {
        Ok(manifest) => manifest,
        Err(error) => {
            return vec![format!("{}: unparseable manifest: {error}", path.display())];
        }
    };
    let Some(entries) = manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("dependencies"))
        .and_then(toml::Value::as_table)
    else {
        return Vec::new();
    };
    let mut violations = Vec::new();
    let mut revs = BTreeSet::new();
    for (key, entry) in entries {
        if !from_harness(entry) {
            continue;
        }
        let package = entry
            .get("package")
            .and_then(toml::Value::as_str)
            .unwrap_or(key);
        if !HARNESS_PUBLIC.contains(&package) {
            violations.push(format!(
                "[workspace.dependencies] {key}: {package} comes from runvil-harness; crates \
                 here may take only {} from it",
                public_crates()
            ));
        }
        match entry.get("rev").and_then(toml::Value::as_str) {
            Some(rev) => {
                revs.insert(rev);
            }
            None => violations.push(format!(
                "[workspace.dependencies] {key}: comes from runvil-harness with no rev; \
                 required one pinned commit"
            )),
        }
    }
    if revs.len() > 1 {
        violations.push(format!(
            "[workspace.dependencies]: the entries from runvil-harness name {} commits ({}); \
             required one, because two commits are two copies of every type the crates share",
            revs.len(),
            revs.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    violations
}

/// The breaches of rule 4: each dependency, in any dependency table of any
/// workspace member's manifest, that names the repository itself.
///
/// The walk's read failures belong to `tidy::marker_violations`, the one
/// owner, as they do for the product-boundary check.
fn member_violations(root: &Path) -> Vec<String> {
    let walk = crate::product::workspace_crates(root);
    let mut violations = Vec::new();
    for krate in &walk.crates {
        let tables =
            crate::manifest::dependency_tables(&krate.manifest, &crate::product::DEP_KINDS);
        for (_, table) in tables {
            for (dep, entry) in table {
                if from_harness(entry) {
                    violations.push(format!(
                        "{}: {dep} names runvil-harness directly; required \
                         {dep}.workspace = true",
                        krate.package
                    ));
                }
            }
        }
    }
    violations
}

/// Whether a dependency entry's `git` names [`HARNESS_REPOSITORY`]. The
/// URLs are compared without a trailing `/` or `.git` and without regard
/// to ASCII case, since each of those spellings reaches the same
/// repository.
fn from_harness(entry: &toml::Value) -> bool {
    entry
        .get("git")
        .and_then(toml::Value::as_str)
        .is_some_and(|git| {
            let url = git.to_ascii_lowercase();
            let url = url.strip_suffix('/').unwrap_or(&url);
            let url = url.strip_suffix(".git").unwrap_or(url);
            url.eq_ignore_ascii_case(HARNESS_REPOSITORY)
        })
}

/// [`HARNESS_PUBLIC`] as a violation lists it: `a, b, c, and d`.
fn public_crates() -> String {
    let [rest @ .., last] = HARNESS_PUBLIC;
    format!("{}, and {last}", rest.join(", "))
}

#[cfg(test)]
#[path = "harness_pin-tests.rs"]
mod tests;
