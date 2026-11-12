//! Product-boundary fixtures: the desktop-app boundary, the dependency
//! tables and renames the check reads, and the classification itself.
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
fn workshop_depends_on_workshop_server_api_only() {
    let walk = workspace_crates(&workspace_root());
    assert!(walk.violations.is_empty(), "{:?}", walk.violations);
    let desktop = walk
        .crates
        .iter()
        .find(|krate| krate.package == "workshop")
        .expect("the workshop desktop app crate is a workspace member");
    assert!(
        desktop.deps.iter().any(|dep| dep == "workshop-server-api"),
        "the desktop app reaches the server through the api crate: {:?}",
        desktop.deps
    );
    assert!(
        !desktop.deps.iter().any(|dep| dep == "workshop-server"),
        "the desktop app never depends on workshop-server directly: {:?}",
        desktop.deps
    );
}

#[test]
fn the_desktop_app_re_adding_workshop_server_is_reported() {
    let root = tempfile::TempDir::new().expect("tempdir");
    write_crate(
        root.path(),
        "workshop",
        "workshop",
        "[dependencies]\nworkshop-server-api = { path = \"../workshop-server-api\" }\n\
         [dev-dependencies]\nworkshop-server = { path = \"../workshop-server\" }\n",
    );
    write_crate(
        root.path(),
        "workshop-server-api",
        "workshop-server-api",
        "",
    );
    write_crate(root.path(), "workshop-server", "workshop-server", "");
    let violations = product_boundary_violations(root.path());
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(
        violations[0].starts_with("workshop depends on workshop-server:")
            && violations[0].contains("workshop-server-api"),
        "the violation names the desktop app, the forbidden dep, and the facade: {violations:?}"
    );
}

#[test]
fn the_desktop_boundarys_allowed_edges_pass() {
    let root = tempfile::TempDir::new().expect("tempdir");
    let api = "workshop-server-api = { path = \"../workshop-server-api\" }\n";
    write_crate(
        root.path(),
        "workshop",
        "workshop",
        &format!("[dependencies]\n{api}[dev-dependencies]\n{api}"),
    );
    write_crate(
        root.path(),
        "workshop-server-api",
        "workshop-server-api",
        "[dependencies]\nworkshop-server = { path = \"../workshop-server\" }\n\
         [dev-dependencies]\nworkshop-server-api = { path = \".\", features = [\"test-fixtures\"] }\n",
    );
    write_crate(root.path(), "workshop-server", "workshop-server", "");
    let violations = product_boundary_violations(root.path());
    assert!(
        violations.is_empty(),
        "the desktop app names the facade, and the facade names the server and itself: {violations:?}"
    );
}

#[test]
fn a_desktop_dependency_on_a_workshop_crate_other_than_the_facade_is_reported() {
    let root = tempfile::TempDir::new().expect("tempdir");
    write_crate(
        root.path(),
        "workshop",
        "workshop",
        "[dependencies]\nworkshop-status = { path = \"../workshop-status\" }\n",
    );
    write_crate(root.path(), "workshop-status", "workshop-status", "");
    let violations = product_boundary_violations(root.path());
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(
        violations[0].starts_with("workshop depends on workshop-status:")
            && violations[0].ends_with("only through workshop-server-api"),
        "the violation names the desktop app, the forbidden dep, and the facade: {violations:?}"
    );
}

#[test]
fn a_facade_dependency_on_a_workshop_crate_other_than_the_server_is_reported() {
    let root = tempfile::TempDir::new().expect("tempdir");
    write_crate(
        root.path(),
        "workshop-server-api",
        "workshop-server-api",
        "[dev-dependencies]\nworkshop-status = { path = \"../workshop-status\" }\n",
    );
    write_crate(root.path(), "workshop-status", "workshop-status", "");
    let violations = product_boundary_violations(root.path());
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(
        violations[0].starts_with("workshop-server-api depends on workshop-status:")
            && violations[0].ends_with("only through workshop-server"),
        "the violation names the facade, the forbidden dep, and the server: {violations:?}"
    );
}

#[test]
fn a_crate_named_like_the_former_gateway_family_is_an_outside_crate() {
    // The Gateway moved to its own repository, so no crate here is
    // classified into a gateway family. A package that would have been one
    // is an ordinary outside crate and is kept out of the workshop
    // container like any other.
    let root = tempfile::TempDir::new().expect("tempdir");
    write_crate(
        root.path(),
        "gateway-routing",
        "gateway-routing",
        "[dependencies]\nworkshop-protocol = { path = \"../workshop/protocol\" }\n",
    );
    write_crate(root.path(), "workshop/protocol", "workshop-protocol", "");
    let violations = product_boundary_violations(root.path());
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(
        violations[0].starts_with("gateway-routing depends on workshop-protocol:")
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
        "[dev-dependencies]\nworkshop-protocol = { path = \"../workshop/protocol\" }\n\
         [target.'cfg(windows)'.dependencies]\nworkshop-status = { path = \"../workshop/status\" }\n",
    );
    write_crate(root.path(), "workshop/protocol", "workshop-protocol", "");
    write_crate(root.path(), "workshop/status", "workshop-status", "");
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
        "[dependencies]\nwire = { package = \"workshop-protocol\", path = \"../workshop/protocol\" }\n",
    );
    write_crate(root.path(), "workshop/protocol", "workshop-protocol", "");
    let violations = product_boundary_violations(root.path());
    assert_eq!(
        violations.len(),
        1,
        "the renamed dependency on workshop-protocol is reported: {violations:?}"
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
        "the read failure belongs to the marker check, not this one"
    );
}

#[test]
fn family_classification_follows_the_naming_rules() {
    assert_eq!(family("workshop"), Family::Workshop);
    assert_eq!(family("workshop-server"), Family::Workshop);
    assert_eq!(family("build-xtask"), Family::Build);
    assert_eq!(family("serde"), Family::Unaffiliated);
    assert_eq!(family("runvil"), Family::Unaffiliated);
    assert_eq!(family("harness-runner"), Family::Unaffiliated);
}
