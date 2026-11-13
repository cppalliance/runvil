//! Product-boundary fixtures: the dependency tables and renames the check
//! reads, the walk's read failures, and the classification itself.
//! Container-privacy fixtures sit in `container_tests.rs`.

use super::test_support::{workspace_root, write_crate};
use super::*;

#[test]
fn workspace_respects_the_product_boundary() {
    let violations = product_boundary_violations(&workspace_root());
    assert!(
        violations.is_empty(),
        "product-boundary violations:\n{}",
        violations.join("\n")
    );
}

#[test]
fn a_crate_named_like_the_former_gateway_family_is_an_outside_crate() {
    // The Gateway moved to its own repository, so no crate here is
    // classified into a gateway family. A package that would have been one
    // is an ordinary outside crate and is kept out of a container like any
    // other.
    let root = tempfile::TempDir::new().expect("tempdir");
    write_crate(
        root.path(),
        "gateway-routing",
        "gateway-routing",
        "[dependencies]\nsuite-protocol = { path = \"../suite/protocol\" }\n",
    );
    write_crate(root.path(), "suite/protocol", "suite-protocol", "");
    let violations = product_boundary_violations(root.path());
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(
        violations[0].starts_with("gateway-routing depends on suite-protocol:")
            && violations[0].contains("no outside crate may depend into it"),
        "container privacy binds it as it binds any other outside crate: {violations:?}"
    );
}

#[test]
fn dev_build_and_target_dependencies_are_checked() {
    let root = tempfile::TempDir::new().expect("tempdir");
    write_crate(
        root.path(),
        "outside-tool",
        "outside-tool",
        "[dev-dependencies]\nsuite-protocol = { path = \"../suite/protocol\" }\n\
         [target.'cfg(windows)'.dependencies]\nsuite-status = { path = \"../suite/status\" }\n",
    );
    write_crate(root.path(), "suite/protocol", "suite-protocol", "");
    write_crate(root.path(), "suite/status", "suite-status", "");
    let violations = product_boundary_violations(root.path());
    assert_eq!(
        violations.len(),
        2,
        "the dev-dependency and the target-specific dependency are both reported: {violations:?}"
    );
}

#[test]
fn package_renames_are_resolved_before_classification() {
    let root = tempfile::TempDir::new().expect("tempdir");
    write_crate(
        root.path(),
        "outside-tool",
        "outside-tool",
        "[dependencies]\nwire = { package = \"suite-protocol\", path = \"../suite/protocol\" }\n",
    );
    write_crate(root.path(), "suite/protocol", "suite-protocol", "");
    let violations = product_boundary_violations(root.path());
    assert_eq!(
        violations.len(),
        1,
        "the renamed dependency on suite-protocol is reported: {violations:?}"
    );
}

#[test]
fn an_unparseable_manifest_is_reported() {
    let root = tempfile::TempDir::new().expect("tempdir");
    let dir = root.path().join("crates").join("broken");
    std::fs::create_dir_all(&dir).expect("the crate directory creates");
    std::fs::write(dir.join("Cargo.toml"), "not [valid toml").expect("the manifest writes");
    let violations = workspace_crates(root.path()).violations;
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(
        violations[0].contains("unparseable manifest"),
        "the violation reports the parse failure: {violations:?}"
    );
    assert!(
        product_boundary_violations(root.path()).is_empty(),
        "the read failure belongs to the crate walk check, not this one"
    );
}

#[test]
fn family_classification_follows_the_naming_rules() {
    assert_eq!(family("build-xtask"), Family::Build);
    assert_eq!(family("serde"), Family::Unaffiliated);
    assert_eq!(family("runvil"), Family::Unaffiliated);
    assert_eq!(family("harness-runner"), Family::Unaffiliated);
}
