//! The `Debug` spelling rule over this workspace, plus fixtures that each
//! refused spelling fails at its line, every other `std::fmt` path passes,
//! and a crate whose manifest was never read, an unreadable file, or a scan
//! that found nothing is not shown clean.

use std::path::Path;

use super::*;
use crate::product::test_support::workspace_root;

/// The trait's bare name, as a group import lists it, kept out of this
/// file's own group imports so no line here lists it in one.
const BARE: &str = "Debug";

/// Writes a crate at `crates/<dir>/` holding each `(path, text)` under its
/// `src/`.
fn write_crate(root: &Path, dir: &str, files: &[(&str, &str)]) {
    let krate = dir
        .split('/')
        .fold(root.join("crates"), |path, part| path.join(part));
    let src = krate.join("src");
    std::fs::create_dir_all(&src).expect("the crate source directory creates");
    std::fs::write(
        krate.join("Cargo.toml"),
        format!("[package]\nname = \"{}\"\n", dir.replace('/', "-")),
    )
    .expect("the manifest writes");
    for (path, text) in files {
        std::fs::write(src.join(path), text).expect("the source file writes");
    }
}

/// The violation the rule reports for `line` of `lib`, the `src/lib.rs` of
/// the fixture crate at `crates/fixture/`.
fn violation_at(lib: &str, line: usize) -> String {
    let found = lib.lines().nth(line - 1).map_or("", str::trim);
    format!("crates/fixture/src/lib.rs:{line}: {RULE}: required {REQUIRED}, found `{found}`")
}

/// Asserts a fixture crate whose `src/lib.rs` is `lib` reports exactly
/// `lines`, each at its line, quoting it, and naming the fix.
fn assert_reported_at(lib: &str, lines: &[usize]) {
    let root = tempfile::TempDir::new().expect("tempdir");
    write_crate(root.path(), "fixture", &[("lib.rs", lib)]);
    let violations = debug_spelling_violations(root.path());
    let expected: Vec<String> = lines.iter().map(|&line| violation_at(lib, line)).collect();
    assert_eq!(violations, expected, "{lib}");
    assert!(
        violations
            .iter()
            .all(|violation| violation.contains("required `fmt::Debug`")),
        "each violation names the fix: {violations:?}"
    );
}

#[test]
fn the_workspace_spells_the_debug_trait_only_as_fmt_debug() {
    let violations = debug_spelling_violations(&workspace_root());
    assert!(
        violations.is_empty(),
        "Debug spelling violations:\n{}",
        violations.join("\n")
    );
}

#[test]
fn each_line_that_spells_the_debug_path_is_reported_once_at_its_line() {
    let lib = format!(
        r#"/// Shown through [`{DEBUG_PATH}`].
pub struct Seam;
impl {DEBUG_PATH} for Seam {{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {{
        {DEBUG_PATH}::fmt(&"seam", f)
    }}
}}
pub fn bound<T: {DEBUG_PATH}>(_value: T) {{}}
pub fn visit(_value: &dyn {DEBUG_PATH}) {{}}
pub fn rooted(_value: &dyn ::{DEBUG_PATH}) {{}}
pub const TEXT: &str = "{DEBUG_PATH}";
pub fn both<T: {DEBUG_PATH}, U: {DEBUG_PATH}>(_t: T, _u: U) {{}}
"#
    );
    assert_reported_at(&lib, &[1, 3, 5, 8, 9, 10, 11, 12]);
}

#[test]
fn fmt_debug_and_every_other_std_fmt_path_pass() {
    let lib = format!(
        r#"use std::fmt;
use std::fmt::Write as _;

/// Shown through [`fmt::Debug`], beside [`std::fmt::Display`].
pub struct Seam;

impl fmt::Debug for Seam {{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {{
        f.write_str("seam")
    }}
}}

pub fn builder(_builder: {DEBUG_PATH}Struct<'_, '_>) {{}}

mod grouped {{
    use std::fmt::{{self, Display}};

    pub fn shown<T: fmt::Debug + Display>(_value: T) {{}}
}}

mod lookalike {{
    use mystd::fmt::{{{BARE}, Display}};

    pub fn shown(_value: &dyn my{DEBUG_PATH}) {{}}
}}
"#
    );
    assert_reported_at(&lib, &[]);
}

#[test]
fn a_group_import_that_lists_debug_is_reported_at_the_line_holding_it() {
    let lib = format!(
        r"use std::fmt::{{{BARE}, Display}};
use std::fmt::{{self, {BARE} as Shown}};
use std::fmt::{{
    self,
    {BARE}Struct,
    {BARE},
}};
use std::fmt::{{self, {BARE}List, Write}};
"
    );
    assert_reported_at(&lib, &[1, 2, 6]);
}

#[test]
fn a_crate_whose_manifest_does_not_parse_is_still_scanned() {
    let root = tempfile::TempDir::new().expect("tempdir");
    let lib = format!("impl {DEBUG_PATH} for Seam {{}}\n");
    write_crate(
        root.path(),
        "runvil-internal/vfs",
        &[("lib.rs", lib.as_str())],
    );
    std::fs::write(
        root.path()
            .join("crates")
            .join("runvil-internal")
            .join("vfs")
            .join("Cargo.toml"),
        "not [valid toml",
    )
    .expect("the manifest rewrites");
    let violations = debug_spelling_violations(root.path());
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(
        violations[0].starts_with(&format!(
            "crates/runvil-internal/vfs/src/lib.rs:1: {RULE}: "
        )),
        "a crate whose manifest was never read cannot be shown clean: {violations:?}"
    );
}

#[test]
fn an_unreadable_source_is_reported_not_skipped() {
    let root = tempfile::TempDir::new().expect("tempdir");
    write_crate(root.path(), "fixture", &[("lib.rs", "pub struct Plain;\n")]);
    let src = root.path().join("crates").join("fixture").join("src");
    std::fs::write(src.join("broken.rs"), [0xff, 0xfe, 0xfd]).expect("the source file writes");
    let violations = debug_spelling_violations(root.path());
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(
        violations[0].starts_with("crates/fixture/src/broken.rs: unreadable source: "),
        "a file that was never scanned cannot be shown clean: {violations:?}"
    );
}

#[test]
fn a_scan_that_finds_no_rust_source_fails() {
    let empty = tempfile::TempDir::new().expect("tempdir");
    std::fs::create_dir_all(empty.path().join("crates")).expect("the crates directory creates");
    let sourceless = tempfile::TempDir::new().expect("tempdir");
    write_crate(sourceless.path(), "tool", &[("notes.md", "# Notes\n")]);
    for root in [&empty, &sourceless] {
        let violations = debug_spelling_violations(root.path());
        assert_eq!(violations.len(), 1, "{violations:?}");
        assert!(
            violations[0].contains("scanned nothing"),
            "a check that read no source cannot show it clean: {violations:?}"
        );
    }
}
