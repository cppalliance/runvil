//! Generated workspaces for the `cargo xtask api` fixtures: a facade
//! crate over one internal crate, `runvil-inner`, plus a stand-in
//! `serde` so the allowlist is exercised without a registry; one fixture
//! adds a second internal crate, `runvil-outer`, downstream of the
//! first, and one adds a second facade, `host`, over the first and over
//! a public crate, `contract`. Building their rustdoc JSON needs the pinned nightly, so every
//! test that calls [`findings`] or `execute` carries `#[ignore = "needs
//! the pinned nightly"]`.

use std::collections::BTreeSet;
use std::path::Path;

use super::load::Facade;
use super::{Mode, Outcome, Report};

/// The second facade of [`workspace_with_host`]: `crates/host` over the
/// same container as the first, passing on the public crate `contract`.
pub(super) const HOST: Facade = Facade {
    package: "host",
    dir: ["crates", "host"],
    containers: &["runvil-internal"],
    public: &["contract"],
    allowed: &[],
};

/// The fixture workspaces' facade: `crates/runvil` over
/// `crates/runvil-internal/`.
pub(super) const FIXTURE: Facade = Facade {
    package: "runvil",
    dir: ["crates", "runvil"],
    containers: &["runvil-internal"],
    public: &[],
    allowed: &[],
};

const WORKSPACE: &str = "[workspace]\nresolver = \"3\"\nmembers = [\
    \"crates/runvil\", \"crates/runvil-internal/*\", \"crates/serde\"]\n";

const FACADE_MANIFEST: &str = "[package]\nname = \"runvil\"\nversion = \"0.0.0\"\n\
    edition = \"2024\"\npublish = false\n\n\
    [features]\ntest-support = [\"runvil-inner/test-support\"]\n\n\
    [dependencies]\nrunvil-inner = { path = \"../runvil-internal/inner\" }\n";

const OUTER_DEPENDENCY: &str = "runvil-outer = { path = \"../runvil-internal/outer\" }\n";

const INNER_MANIFEST: &str = "[package]\nname = \"runvil-inner\"\nversion = \"0.0.0\"\n\
    edition = \"2024\"\npublish = false\n\n\
    [dependencies]\nserde = { path = \"../../serde\" }\n\n\
    [features]\ntest-support = []\n";

const OUTER_MANIFEST: &str = "[package]\nname = \"runvil-outer\"\nversion = \"0.0.0\"\n\
    edition = \"2024\"\npublish = false\n\n\
    [dependencies]\nrunvil-inner = { path = \"../inner\" }\n";

const HOST_WORKSPACE: &str = "[workspace]\nresolver = \"3\"\nmembers = [\
    \"crates/runvil\", \"crates/runvil-internal/*\", \"crates/serde\", \
    \"crates/host\", \"crates/contract\"]\n";

const HOST_MANIFEST: &str = "[package]\nname = \"host\"\nversion = \"0.0.0\"\n\
    edition = \"2024\"\npublish = false\n\n\
    [dependencies]\nrunvil = { path = \"../runvil\" }\n\
    contract = { path = \"../contract\" }\n";

const CONTRACT_MANIFEST: &str = "[package]\nname = \"contract\"\nversion = \"0.0.0\"\n\
    edition = \"2024\"\npublish = false\n";

const SERDE_MANIFEST: &str =
    "[package]\nname = \"serde\"\nversion = \"1.0.0\"\nedition = \"2024\"\npublish = false\n";

const SERDE_LIB: &str = "//! Stand-in for the allowlisted `serde`.\n\n\
    /// A stand-in serialization trait.\npub trait Serialize {}\n";

/// A workspace whose internal crate's `lib.rs` is `inner` and whose
/// facade's is `facade`, with its lockfile written so `--locked` holds.
pub(super) fn workspace(inner: &str, facade: &str) -> tempfile::TempDir {
    generate(inner, None, facade)
}

/// [`workspace`] plus `runvil-outer`, whose `lib.rs` is `outer`,
/// depending on the inner crate; the facade depends on both.
pub(super) fn workspace_with_outer(inner: &str, outer: &str, facade: &str) -> tempfile::TempDir {
    generate(inner, Some(outer), facade)
}

/// [`workspace`] plus a second facade, `host`, whose `lib.rs` is `host`.
/// It depends on the first facade and on the public crate `contract`,
/// whose `lib.rs` is `contract`.
pub(super) fn workspace_with_host(
    inner: &str,
    facade: &str,
    contract: &str,
    host: &str,
) -> tempfile::TempDir {
    let root = generate(inner, None, facade);
    for (path, text) in [
        ("Cargo.toml", HOST_WORKSPACE),
        ("crates/host/Cargo.toml", HOST_MANIFEST),
        ("crates/host/src/lib.rs", host),
        ("crates/contract/Cargo.toml", CONTRACT_MANIFEST),
        ("crates/contract/src/lib.rs", contract),
    ] {
        write(root.path(), path, text);
    }
    lock(root.path());
    root
}

fn generate(inner: &str, outer: Option<&str>, facade: &str) -> tempfile::TempDir {
    let root = tempfile::TempDir::new().expect("tempdir");
    let facade_manifest = match outer {
        Some(_) => format!("{FACADE_MANIFEST}{OUTER_DEPENDENCY}"),
        None => FACADE_MANIFEST.to_owned(),
    };
    for (path, text) in [
        ("Cargo.toml", WORKSPACE),
        ("crates/runvil/Cargo.toml", &facade_manifest),
        ("crates/runvil/src/lib.rs", facade),
        ("crates/runvil-internal/inner/Cargo.toml", INNER_MANIFEST),
        ("crates/runvil-internal/inner/src/lib.rs", inner),
        ("crates/serde/Cargo.toml", SERDE_MANIFEST),
        ("crates/serde/src/lib.rs", SERDE_LIB),
    ] {
        write(root.path(), path, text);
    }
    if let Some(outer) = outer {
        write(
            root.path(),
            "crates/runvil-internal/outer/Cargo.toml",
            OUTER_MANIFEST,
        );
        write(
            root.path(),
            "crates/runvil-internal/outer/src/lib.rs",
            outer,
        );
    }
    lock(root.path());
    root
}

/// Writes the lockfile of the workspace at `root`, so `--locked` holds.
fn lock(root: &Path) {
    let output = super::load::cargo(root)
        .args(["generate-lockfile", "--offline"])
        .output()
        .expect("cargo runs");
    assert!(
        output.status.success(),
        "the fixture lockfile generates:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Writes `text` at `path` under `root`, creating directories.
pub(super) fn write(root: &Path, path: &str, text: &str) {
    let file = root.join(path);
    std::fs::create_dir_all(file.parent().expect("a fixture file has a directory"))
        .expect("the fixture directory creates");
    std::fs::write(file, text).expect("the fixture file writes");
}

/// The report for the fixture facade of the workspace at `root`.
pub(super) fn report(root: &Path) -> Result<Report, String> {
    super::report(root, &FIXTURE)
}

/// Runs the fixture facade of the workspace at `root` in `mode`.
pub(super) fn execute(root: &Path, mode: Mode) -> Result<Outcome, String> {
    super::execute(root, &FIXTURE, mode)
}

/// The findings for `facade` in the workspace at `root`, as printed.
pub(super) fn findings_of(root: &Path, facade: &Facade) -> BTreeSet<String> {
    super::report(root, facade)
        .expect("the fixture documents")
        .findings
        .iter()
        .map(ToString::to_string)
        .collect()
}

/// The findings for the workspace at `root`, as printed.
pub(super) fn findings(root: &Path) -> BTreeSet<String> {
    report(root)
        .expect("the fixture documents")
        .findings
        .iter()
        .map(ToString::to_string)
        .collect()
}

/// The findings `expected` names, as a set to compare with [`findings`].
pub(super) fn set<const N: usize>(expected: [String; N]) -> BTreeSet<String> {
    expected.into_iter().collect()
}

/// A closure finding's text after the mention, for an item of internal
/// crate `krate` the fixture facade does not re-export.
pub(super) fn leak(krate: &str) -> String {
    format!(
        "required {}, found an item of internal crate `{krate}` the facade does not re-export",
        super::items::required_with(&[])
    )
}
