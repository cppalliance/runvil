//! Container-privacy fixtures: the manifestless `crates/<family>/`
//! containers are private to their families; from outside, only the
//! `build-*` crates may depend into one.

use super::product_boundary_violations;
use super::test_support::write_crate;

#[test]
fn an_outside_crate_depending_into_the_workshop_container_is_reported() {
    let root = tempfile::TempDir::new().expect("tempdir");
    write_crate(
        root.path(),
        "outside-tool",
        "outside-tool",
        "[dependencies]\nworkshop-protocol = { path = \"../workshop/protocol\" }\n",
    );
    write_crate(root.path(), "workshop/protocol", "workshop-protocol", "");
    let violations = product_boundary_violations(root.path());
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(
        violations[0].contains("outside-tool depends on workshop-protocol")
            && violations[0].contains("crates/workshop is private to its family")
            && violations[0].contains("no outside crate"),
        "the violation includes the workshop container privacy message: {violations:?}"
    );
}

#[test]
fn workshop_container_siblings_may_depend_on_each_other() {
    let root = tempfile::TempDir::new().expect("tempdir");
    write_crate(
        root.path(),
        "workshop/server",
        "workshop-server",
        "[dependencies]\nworkshop-protocol = { path = \"../protocol\" }\n",
    );
    write_crate(root.path(), "workshop/protocol", "workshop-protocol", "");
    let violations = product_boundary_violations(root.path());
    assert!(
        violations.is_empty(),
        "workshop container siblings may depend on each other: {violations:?}"
    );
}

#[test]
fn a_build_crate_depending_into_a_container_passes() {
    let root = tempfile::TempDir::new().expect("tempdir");
    write_crate(
        root.path(),
        "build-workshop",
        "build-workshop",
        "[dependencies]\nworkshop-protocol = { path = \"../workshop/protocol\" }\n",
    );
    write_crate(root.path(), "workshop/protocol", "workshop-protocol", "");
    let violations = product_boundary_violations(root.path());
    assert!(
        violations.is_empty(),
        "the build-* crates are meta tooling, exempt from container privacy: {violations:?}"
    );
}

#[test]
fn a_crate_outside_the_workshop_container_cannot_reach_a_nested_one() {
    let root = tempfile::TempDir::new().expect("tempdir");
    write_crate(
        root.path(),
        "outside-tool",
        "outside-tool",
        "[dependencies]\nworkshop-protocol = { path = \"../workshop/nested/protocol\" }\n",
    );
    write_crate(
        root.path(),
        "workshop/nested/protocol",
        "workshop-protocol",
        "",
    );
    let violations = product_boundary_violations(root.path());
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(
        violations[0].contains("crates/workshop/nested is private to its family")
            && violations[0].contains("no outside crate"),
        "the violation names the nested container and refuses every outside crate: {violations:?}"
    );
}
