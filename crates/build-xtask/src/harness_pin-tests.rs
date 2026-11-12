//! The runvil-harness pin over this workspace, plus fixtures that each
//! rule fails for the entry that breaks it, every spelling of the
//! repository's URL matches, and a git dependency on another repository
//! is left alone.

use super::*;
use crate::product::test_support::{workspace_root, write_crate};

/// The `git` field of a dependency on the repository, spelled out here so
/// the fixtures do not follow the constant they test.
const GIT: &str = "git = \"https://github.com/cppalliance/runvil-harness\"";

/// The commit the fixtures pin.
const REV: &str = "732429962c5873458213bb0072a0828a40ee4fb6";

/// A second commit, which sorts before [`REV`].
const OTHER_REV: &str = "03a9fa37d2fe690d42ee0aa5c7a8289173e8e0f2";

/// A fake workspace root whose `Cargo.toml` is `manifest`, with an empty
/// `crates/` directory.
fn workspace(manifest: &str) -> tempfile::TempDir {
    let root = tempfile::TempDir::new().expect("tempdir");
    std::fs::write(root.path().join("Cargo.toml"), manifest).expect("the root manifest writes");
    std::fs::create_dir_all(root.path().join("crates")).expect("the crates directory creates");
    root
}

#[test]
fn the_workspace_takes_only_the_public_crates_from_runvil_harness() {
    let violations = harness_pin_violations(&workspace_root());
    assert!(
        violations.is_empty(),
        "runvil-harness pin violations:\n{}",
        violations.join("\n")
    );
}

#[test]
fn the_four_public_crates_at_one_rev_pass() {
    let root = workspace(&format!(
        "[workspace.dependencies]\n\
         base64 = \"0.22\"\n\
         workshop-protocol = {{ path = \"crates/workshop/protocol\" }}\n\
         runvil = {{ version = \"0.3.0\", {GIT}, rev = \"{REV}\" }}\n\
         harness-gateway-client = {{ {GIT}, rev = \"{REV}\" }}\n\
         plugin-web = {{ {GIT}, rev = \"{REV}\" }}\n\
         plugin-mcp = {{ {GIT}, rev = \"{REV}\" }}\n"
    ));
    write_crate(
        root.path(),
        "workshop/server",
        "workshop-server",
        "[dependencies]\nrunvil.workspace = true\nplugin-web = { workspace = true }\n\
         [dev-dependencies]\nworkshop-protocol.workspace = true\n",
    );
    let violations = harness_pin_violations(root.path());
    assert!(violations.is_empty(), "{violations:?}");
}

#[test]
fn a_fifth_crate_from_the_repository_is_reported() {
    let root = workspace(&format!(
        "[workspace.dependencies]\n\
         runvil = {{ {GIT}, rev = \"{REV}\" }}\n\
         harness-runner = {{ {GIT}, rev = \"{REV}\" }}\n"
    ));
    let violations = harness_pin_violations(root.path());
    assert_eq!(
        violations,
        [
            "[workspace.dependencies] harness-runner: harness-runner comes from runvil-harness; \
             crates here may take only runvil, harness-gateway-client, plugin-web, and \
             plugin-mcp from it"
        ]
    );
}

#[test]
fn a_renamed_entry_is_reported_by_its_package_name() {
    let root = workspace(&format!(
        "[workspace.dependencies]\n\
         engine = {{ package = \"runvil-engine\", {GIT}, rev = \"{REV}\" }}\n\
         facade = {{ package = \"runvil\", {GIT}, rev = \"{REV}\" }}\n"
    ));
    let violations = harness_pin_violations(root.path());
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(
        violations[0].starts_with(
            "[workspace.dependencies] engine: runvil-engine comes from runvil-harness;"
        ),
        "the key locates the entry and the package names the crate: {violations:?}"
    );
}

#[test]
fn an_entry_with_a_branch_and_no_rev_is_reported() {
    for pin in ["branch = \"main\"", "tag = \"v0.3.0\""] {
        let root = workspace(&format!(
            "[workspace.dependencies]\n\
             runvil = {{ {GIT}, {pin} }}\n\
             plugin-web = {{ {GIT}, rev = \"{REV}\" }}\n"
        ));
        let violations = harness_pin_violations(root.path());
        assert_eq!(
            violations,
            [
                "[workspace.dependencies] runvil: comes from runvil-harness with no rev; \
                 required one pinned commit"
            ],
            "{pin}"
        );
    }
}

#[test]
fn two_different_revs_are_reported_once() {
    let root = workspace(&format!(
        "[workspace.dependencies]\n\
         runvil = {{ {GIT}, rev = \"{REV}\" }}\n\
         harness-gateway-client = {{ {GIT}, rev = \"{REV}\" }}\n\
         plugin-web = {{ {GIT}, rev = \"{OTHER_REV}\" }}\n\
         plugin-mcp = {{ {GIT}, rev = \"{OTHER_REV}\" }}\n"
    ));
    let violations = harness_pin_violations(root.path());
    assert_eq!(
        violations,
        [format!(
            "[workspace.dependencies]: the entries from runvil-harness name 2 commits \
             ({OTHER_REV}, {REV}); required one, because two commits are two copies of every \
             type the crates share"
        )]
    );
}

#[test]
fn a_member_naming_the_repository_directly_is_reported() {
    let root = workspace(&format!(
        "[workspace.dependencies]\nrunvil = {{ {GIT}, rev = \"{REV}\" }}\n"
    ));
    write_crate(
        root.path(),
        "workshop/server",
        "workshop-server",
        &format!(
            "[dependencies]\nrunvil = {{ {GIT}, rev = \"{REV}\" }}\n\
             [dev-dependencies]\nplugin-web.workspace = true\n"
        ),
    );
    write_crate(
        root.path(),
        "workshop/desktop",
        "workshop",
        &format!(
            "[target.'cfg(windows)'.dependencies]\nplugin-mcp = {{ {GIT}, rev = \"{REV}\" }}\n"
        ),
    );
    let mut violations = harness_pin_violations(root.path());
    violations.sort();
    assert_eq!(
        violations,
        [
            "workshop-server: runvil names runvil-harness directly; required \
             runvil.workspace = true",
            "workshop: plugin-mcp names runvil-harness directly; required \
             plugin-mcp.workspace = true",
        ]
    );
}

#[test]
fn the_git_suffix_and_a_trailing_slash_still_match() {
    for url in [
        "https://github.com/cppalliance/runvil-harness.git",
        "https://github.com/cppalliance/runvil-harness/",
        "https://github.com/cppalliance/runvil-harness.git/",
        "https://GitHub.com/CppAlliance/Runvil-Harness",
    ] {
        let root = workspace(&format!(
            "[workspace.dependencies]\n\
             harness-runner = {{ git = \"{url}\", rev = \"{REV}\" }}\n"
        ));
        write_crate(
            root.path(),
            "workshop/server",
            "workshop-server",
            &format!("[dependencies]\nrunvil = {{ git = \"{url}\", rev = \"{REV}\" }}\n"),
        );
        let violations = harness_pin_violations(root.path());
        assert_eq!(violations.len(), 2, "{url}: {violations:?}");
        assert!(
            violations[0].starts_with("[workspace.dependencies] harness-runner: harness-runner")
                && violations[1].starts_with("workshop-server: runvil names runvil-harness"),
            "{url} names the repository in the root manifest and in a member: {violations:?}"
        );
    }
}

#[test]
fn a_git_dependency_on_another_repository_is_ignored() {
    let other = "git = \"https://github.com/cppalliance/runvil-plugin-api\"";
    let near = "git = \"https://github.com/cppalliance/runvil-harness-tools\"";
    let root = workspace(&format!(
        "[workspace.dependencies]\n\
         runvil = {{ {GIT}, rev = \"{REV}\" }}\n\
         runvil-plugin-sdk = {{ {other}, branch = \"main\" }}\n\
         harness-runner = {{ {near}, rev = \"{OTHER_REV}\" }}\n"
    ));
    write_crate(
        root.path(),
        "workshop/plugin-std",
        "workshop-plugin-std",
        &format!("[dependencies]\nrunvil-plugin-sdk = {{ {other}, rev = \"{OTHER_REV}\" }}\n"),
    );
    let violations = harness_pin_violations(root.path());
    assert!(
        violations.is_empty(),
        "another repository's crates, pins, and commits are not this check's: {violations:?}"
    );
}

#[test]
fn an_unreadable_or_unparseable_root_manifest_is_reported() {
    let missing = tempfile::TempDir::new().expect("tempdir");
    let broken = workspace("not [valid toml");
    for (root, problem) in [
        (&missing, ": unreadable manifest: "),
        (&broken, ": unparseable manifest: "),
    ] {
        let manifest = root.path().join("Cargo.toml");
        let violations = harness_pin_violations(root.path());
        assert_eq!(violations.len(), 1, "{violations:?}");
        assert!(
            violations[0].starts_with(&manifest.display().to_string())
                && violations[0].contains(problem),
            "a table that was never read cannot be shown pinned: {violations:?}"
        );
    }
}
