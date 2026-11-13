//! Product-boundary check: the dependency matrix between the crates this
//! workspace holds.
//!
//! Every workspace crate is classified by package name into a family:
//! build crates (`build-*`) and unaffiliated crates, which are all the
//! rest. Its dependencies of every kind (normal, dev, build, and
//! target-specific) are checked against the matrix, which holds one rule:
//!
//! - Container privacy: a manifestless directory under `crates/` is a
//!   container, private to the crates inside it; only they and the
//!   `build-*` crates, which are meta tooling, may depend on the crates it
//!   holds. Containers may nest. The workspace holds no container today,
//!   so the rule binds the first one added.
//!
//! Crates from other repositories are outside the matrix: it classifies
//! and binds only workspace members.
//!
//! The crate walk, `workspace_crates`, is here too: the one enumeration of
//! the workspace's crates that every structural check shares.

use std::fs;
use std::path::{Path, PathBuf};

/// The dependency tables cargo recognizes, directly and under `[target]`.
const DEP_KINDS: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

/// The family a package name belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Build,
    /// Named after no family; has no matrix rules of its own.
    Unaffiliated,
}

/// Classifies a package name into its family.
fn family(package: &str) -> Family {
    if package.starts_with("build-") {
        Family::Build
    } else {
        Family::Unaffiliated
    }
}

/// A workspace crate: its package name, its manifest directory relative to
/// the workspace root, and its dependency package names.
pub(crate) struct CrateInfo {
    package: String,
    pub(crate) dir: PathBuf,
    deps: Vec<String>,
}

/// Every crate under `crates/`, as the one enumeration the structural
/// checks share.
pub(crate) struct CrateWalk {
    /// The crates whose manifests named a package.
    pub(crate) crates: Vec<CrateInfo>,
    /// Crate directories, relative to the workspace root, whose manifest
    /// could not be read, parsed, or named. A crate that cannot be read
    /// cannot be shown to pass, so the checks that bind every crate bind
    /// these too.
    pub(crate) unread: Vec<PathBuf>,
    /// Every read failure, in walk order.
    pub(crate) violations: Vec<String>,
}

/// Checks every workspace manifest against the product-boundary matrix.
///
/// The walk's read failures belong to `tidy::crate_walk_violations`, the
/// one owner: both checks share the walk, so reporting them here too would
/// name every unreadable manifest twice in `tidy::all_violations`.
#[must_use]
pub(crate) fn product_boundary_violations(root: &Path) -> Vec<String> {
    let CrateWalk { crates, .. } = workspace_crates(root);
    let mut violations = Vec::new();
    for package in &crates {
        for dep_name in &package.deps {
            // Only workspace members are bound by the matrix; a package
            // from crates.io or another repository is outside it.
            let Some(dep) = crates.iter().find(|krate| &krate.package == dep_name) else {
                continue;
            };
            if let Some(reason) = boundary_breach(package, dep) {
                violations.push(format!(
                    "{} depends on {}: {reason}",
                    package.package, dep.package
                ));
            }
        }
    }
    violations
}

/// The reason a dependency from `package` to `dep` breaches the matrix,
/// or `None` when the edge is legal.
fn boundary_breach(package: &CrateInfo, dep: &CrateInfo) -> Option<String> {
    if let Some(container) = container_of(&dep.dir) {
        // Inside is physical containment: a crate anywhere under
        // crates/<container>/ may name the container's crates, so the
        // crates of a nested container may name the crates of the
        // container around it, while those may not name the nested
        // container's crates back.
        let inside = package
            .dir
            .starts_with(Path::new("crates").join(&container));
        // The build-* crates are meta tooling: they may depend into any
        // container.
        let meta = family(&package.package) == Family::Build;
        if !inside && !meta {
            return Some(format!(
                "crates/{container} is private to the crates inside it; no outside crate may \
                 depend into it"
            ));
        }
    }
    None
}

/// The root-relative path of the deepest manifestless `crates/<container>/`
/// directory holding the crate whose root-relative manifest directory is
/// `dir` (`crates/suite/server` is in `suite`), or `None` for a crate
/// directly under `crates/`.
fn container_of(dir: &Path) -> Option<String> {
    let mut components = dir.components();
    if components.next()?.as_os_str() != "crates" {
        return None;
    }
    let rest: Vec<&str> = components.filter_map(|c| c.as_os_str().to_str()).collect();
    // The last component is the crate's own directory; everything between
    // `crates/` and it is the container path.
    match rest.len() {
        0 | 1 => None,
        _ => Some(rest[..rest.len() - 1].join("/")),
    }
}

/// Every workspace crate's package name, manifest directory, and
/// dependency package names, plus the directories and violations for
/// manifests that could not be read or parsed. A directory under `crates/`
/// containing a `Cargo.toml` is a crate and is not descended into; any
/// other directory is a container and the walk descends, so containers
/// may nest.
pub(crate) fn workspace_crates(root: &Path) -> CrateWalk {
    let mut walk = CrateWalk {
        crates: Vec::new(),
        unread: Vec::new(),
        violations: Vec::new(),
    };
    let crates_dir = root.join("crates");
    walk_crates(root, &crates_dir, &mut walk);
    walk
}

/// Walks one directory level: crates are read, manifestless containers are
/// descended into.
fn walk_crates(root: &Path, dir: &Path, walk: &mut CrateWalk) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            walk.violations.push(format!(
                "{}: unreadable crates directory: {error}",
                dir.display()
            ));
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                walk.violations.push(format!(
                    "{}: unreadable directory entry: {error}",
                    dir.display()
                ));
                continue;
            }
        };
        if !entry.path().is_dir() {
            continue;
        }
        let sub = entry.path();
        if sub.join("Cargo.toml").exists() {
            read_crate(root, &sub, walk);
        } else {
            walk_crates(root, &sub, walk);
        }
    }
}

/// Reads one crate's manifest into the walk; a failure names the crate in
/// `unread` and the failure mode in `violations`.
fn read_crate(root: &Path, dir: &Path, walk: &mut CrateWalk) {
    let relative = dir.strip_prefix(root).unwrap_or(dir).to_path_buf();
    let manifest_path = dir.join("Cargo.toml");
    let text = match fs::read_to_string(&manifest_path) {
        Ok(text) => text,
        Err(error) => {
            walk.unread.push(relative);
            walk.violations.push(format!(
                "{}: unreadable manifest: {error}",
                manifest_path.display()
            ));
            return;
        }
    };
    let manifest = match toml::from_str::<toml::Value>(&text) {
        Ok(manifest) => manifest,
        Err(error) => {
            walk.unread.push(relative);
            walk.violations.push(format!(
                "{}: unparseable manifest: {error}",
                manifest_path.display()
            ));
            return;
        }
    };
    let Some(package) = manifest
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(toml::Value::as_str)
    else {
        walk.unread.push(relative);
        walk.violations.push(format!(
            "{}: manifest has no package name",
            manifest_path.display()
        ));
        return;
    };
    walk.crates.push(CrateInfo {
        package: package.to_owned(),
        dir: relative,
        deps: manifest_dependencies(&manifest),
    });
}

/// Every dependency package name declared in a manifest, across normal,
/// dev, build, and target-specific tables, resolving `package` renames.
fn manifest_dependencies(manifest: &toml::Value) -> Vec<String> {
    let mut names = Vec::new();
    for (_, table) in crate::manifest::dependency_tables(manifest, &DEP_KINDS) {
        collect_deps(table, &mut names);
    }
    names
}

fn collect_deps(table: &toml::map::Map<String, toml::Value>, names: &mut Vec<String>) {
    for (key, value) in table {
        let package = value
            .get("package")
            .and_then(toml::Value::as_str)
            .unwrap_or(key);
        if !names.iter().any(|name| name == package) {
            names.push(package.to_owned());
        }
    }
}

#[cfg(test)]
mod container_tests;
#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;
