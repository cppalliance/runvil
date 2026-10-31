//! Conformance between the Gateway's Realtime wire fixtures and the copies
//! Workshop owns.
//!
//! Workshop reads the canonical Realtime examples from its own tree, because
//! a test input it does not own stops the Workshop UI suite and the
//! `workshop-server` relay test from running once the Gateway moves. A copy
//! drifts, so this test compares the copies byte for byte and names the file
//! and the first difference whenever they disagree.
//!
//! The check lives here because `build-xtask` is the one crate the product
//! rules let read both families; the workshop crates may not depend on a
//! gateway crate. Like `discovery_conformance.rs`, it is expected to be
//! deleted when the Gateway moves to its own repository: at that point the
//! Gateway copy stops existing in this tree, and the Workshop copy becomes
//! the only copy to keep.

#![expect(
    clippy::expect_used,
    reason = "test helpers panic on setup failure, which is the desired behavior"
)]

use std::path::{Path, PathBuf};

/// The Gateway's copies of the Realtime fixtures, relative to the repository
/// root.
const GATEWAY_FIXTURES: &str = "crates/gateway/stt/api/tests/fixtures/realtime";

/// Workshop's copies, relative to the repository root. The SPA suite and the
/// `workshop-server` relay test read these same files.
const WORKSHOP_FIXTURES: &str = "crates/workshop/server/tests/fixtures/realtime";

/// The fixture files, in the order the Workshop suites use them. A file the
/// Gateway adds later is reported by `the_gateway_and_workshop_fixture_sets_agree`.
const FIXTURES: [&str; 5] = [
    "client-events.json",
    "effective-sessions.json",
    "invalid-sequences.json",
    "server-events.json",
    "valid-sequences.json",
];

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("build-xtask lives at <root>/crates/build-xtask")
        .to_path_buf()
}

fn json_files(dir: &Path) -> Vec<String> {
    let entries =
        std::fs::read_dir(dir).unwrap_or_else(|error| panic!("{} reads: {error}", dir.display()));
    let mut names = entries
        .map(|entry| {
            entry
                .expect("a fixture directory entry reads")
                .file_name()
                .to_str()
                .expect("a fixture file name is UTF-8")
                .to_owned()
        })
        .filter(|name| {
            Path::new(name)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
        })
        .collect::<Vec<_>>();
    names.sort();
    names
}

/// The byte offset of the first difference, or `None` when the two are equal.
///
/// The length check comes last but cannot be skipped: `zip` stops at the
/// shorter input, so a copy that only gained or lost trailing bytes would
/// otherwise compare equal.
fn first_difference(left: &[u8], right: &[u8]) -> Option<usize> {
    if let Some(offset) = left.iter().zip(right).position(|(a, b)| a != b) {
        return Some(offset);
    }
    (left.len() != right.len()).then_some(left.len().min(right.len()))
}

#[test]
fn every_workshop_copy_matches_the_gateway_original() {
    let root = repository_root();
    let gateway_dir = root.join(GATEWAY_FIXTURES);
    let workshop_dir = root.join(WORKSHOP_FIXTURES);
    let mut drifts = Vec::new();

    for name in FIXTURES {
        let gateway = std::fs::read(gateway_dir.join(name))
            .unwrap_or_else(|error| panic!("{GATEWAY_FIXTURES}/{name} reads: {error}"));
        let workshop = std::fs::read(workshop_dir.join(name))
            .unwrap_or_else(|error| panic!("{WORKSHOP_FIXTURES}/{name} reads: {error}"));
        match first_difference(&gateway, &workshop) {
            None => {}
            Some(offset) => drifts.push(format!(
                "{name} differs at byte {offset}: the Gateway has {} bytes, Workshop has {}. \
                 Copy the Gateway file over the Workshop one, or change both together.",
                gateway.len(),
                workshop.len()
            )),
        }
    }

    assert!(
        drifts.is_empty(),
        "the Workshop copies of the Realtime fixtures drifted from the Gateway's originals:\n{}",
        drifts.join("\n")
    );
}

#[test]
fn the_gateway_and_workshop_fixture_sets_agree() {
    let root = repository_root();
    assert_eq!(
        json_files(&root.join(GATEWAY_FIXTURES)),
        json_files(&root.join(WORKSHOP_FIXTURES)),
        "the two fixture directories hold different files; a fixture the Gateway adds or \
         removes needs the same change in the Workshop copy"
    );
}

#[test]
fn a_trailing_byte_counts_as_a_difference() {
    // `zip` stops at the shorter input, so this is the case a plain zipped
    // comparison misses. A copy that only gains a trailing newline is drift.
    assert_eq!(first_difference(b"abc", b"abc"), None);
    assert_eq!(first_difference(b"abc", b"abd"), Some(2));
    assert_eq!(first_difference(b"abc", b"abc "), Some(3));
    assert_eq!(first_difference(b"abc ", b"abc"), Some(3));
    assert_eq!(first_difference(b"", b"x"), Some(0));
}
