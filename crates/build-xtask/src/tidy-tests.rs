use super::*;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("build-xtask lives at <root>/crates/build-xtask")
        .to_path_buf()
}

#[test]
fn every_workspace_crate_inherits_workspace_lints() {
    let violations = lint_inheritance_violations(&workspace_root());
    assert!(
        violations.is_empty(),
        "lint violations:\n{}",
        violations.join("\n")
    );
}

#[test]
fn the_crate_walk_reads_every_workspace_manifest() {
    let violations = crate_walk_violations(&workspace_root());
    assert!(
        violations.is_empty(),
        "crate walk violations:\n{}",
        violations.join("\n")
    );
}

#[test]
fn a_crate_whose_manifest_has_no_package_name_is_reported_not_skipped() {
    let root = tempfile::TempDir::new().expect("tempdir");
    let dir = root.path().join("crates").join("nameless");
    std::fs::create_dir_all(&dir).expect("the crate directory creates");
    std::fs::write(dir.join("Cargo.toml"), "[lints]\nworkspace = true\n")
        .expect("the manifest writes");
    let violations = crate_walk_violations(root.path());
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(
        violations[0].contains("nameless") && violations[0].contains("no package name"),
        "the directory and the failure mode are named: {violations:?}"
    );
}

#[test]
fn a_manifest_read_failure_is_reported_once_across_the_checks() {
    let root = tempfile::TempDir::new().expect("tempdir");
    let dir = root.path().join("crates").join("broken");
    std::fs::create_dir_all(&dir).expect("the crate directory creates");
    std::fs::write(dir.join("Cargo.toml"), "not [valid toml").expect("the manifest writes");
    let violations = crate_walk_violations(root.path());
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(
        violations[0].contains("unparseable manifest"),
        "the crate walk check owns the walk's read failures: {violations:?}"
    );
    assert!(
        crate::product::product_boundary_violations(root.path()).is_empty(),
        "the two checks share one walk and report its read failures once"
    );
}

/// Writes a crate under `crates/<dir>/` named `name` that inherits the
/// workspace lints.
fn write_crate(root: &Path, dir: &str, name: &str) {
    let dir = root.join("crates").join(dir);
    std::fs::create_dir_all(&dir).expect("the crate directory creates");
    std::fs::write(
        dir.join("Cargo.toml"),
        format!("[package]\nname = \"{name}\"\n[lints]\nworkspace = true\n"),
    )
    .expect("the manifest writes");
}

/// Writes a workspace root manifest that sets `unreachable_pub`, the one
/// workspace lint the inheritance check requires of the root.
fn write_root_manifest(root: &Path) {
    std::fs::write(
        root.join("Cargo.toml"),
        "[workspace.lints.rust]\nunreachable_pub = \"warn\"\n",
    )
    .expect("the root manifest writes");
}

#[test]
fn crates_that_inherit_the_workspace_lints_pass() {
    let root = tempfile::TempDir::new().expect("tempdir");
    write_root_manifest(root.path());
    write_crate(root.path(), "build-ui", "build-ui");
    write_crate(root.path(), "suite/status", "suite-status");
    let violations = lint_inheritance_violations(root.path());
    assert!(violations.is_empty(), "{violations:?}");
}

#[test]
fn a_crate_with_its_own_lint_table_is_reported() {
    let root = tempfile::TempDir::new().expect("tempdir");
    write_root_manifest(root.path());
    write_crate(root.path(), "build-ui", "build-ui");
    std::fs::write(
        root.path()
            .join("crates")
            .join("build-ui")
            .join("Cargo.toml"),
        "[package]\nname = \"build-ui\"\n[lints.clippy]\npedantic = \"warn\"\n",
    )
    .expect("the manifest rewrites");
    let violations = lint_inheritance_violations(root.path());
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(
        violations[0].contains("build-ui") && violations[0].contains("[lints] workspace = true"),
        "a crate that sets its own lints does not inherit the workspace's: {violations:?}"
    );
}

#[test]
fn a_lint_inheritance_scan_that_finds_no_crate_fails() {
    let root = tempfile::TempDir::new().expect("tempdir");
    write_root_manifest(root.path());
    std::fs::create_dir_all(root.path().join("crates")).expect("the crates directory creates");
    let violations = lint_inheritance_violations(root.path());
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(
        violations[0].contains("scanned nothing"),
        "a check that read no crate cannot show any inherits: {violations:?}"
    );
}

#[test]
fn a_crate_whose_manifest_does_not_parse_is_held_to_lint_inheritance_not_skipped() {
    let root = tempfile::TempDir::new().expect("tempdir");
    write_root_manifest(root.path());
    write_crate(root.path(), "build-ui", "build-ui");
    let dir = root.path().join("crates").join("broken");
    std::fs::create_dir_all(&dir).expect("the crate directory creates");
    std::fs::write(dir.join("Cargo.toml"), "not [valid toml").expect("the manifest writes");
    let violations = lint_inheritance_violations(root.path());
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(
        violations[0].contains("broken") && violations[0].contains("[lints] workspace = true"),
        "a crate that was never read cannot be shown to inherit: {violations:?}"
    );
}

/// A name no UTF-8 string can hold: a stray byte on unix, an unpaired
/// surrogate on windows.
#[cfg(unix)]
fn undecodable_name() -> std::ffi::OsString {
    use std::os::unix::ffi::OsStringExt;
    std::ffi::OsString::from_vec(b"lib\xff".to_vec())
}

#[cfg(windows)]
fn undecodable_name() -> std::ffi::OsString {
    use std::os::windows::ffi::OsStringExt;
    std::ffi::OsString::from_wide(&[u16::from(b'l'), u16::from(b'i'), u16::from(b'b'), 0xd800])
}

#[test]
fn an_undecodable_path_component_keeps_its_place_in_the_relative_path() {
    let root = Path::new("root");
    let file = root
        .join("crates")
        .join(undecodable_name())
        .join("src")
        .join("lib.rs");
    let relative = slash_path(root, &file);
    assert_eq!(
        relative.split('/').count(),
        4,
        "a component that does not decode must not vanish and let the path \
         collide with an allowlist entry: {relative}"
    );
}
