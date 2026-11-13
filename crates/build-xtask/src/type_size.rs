//! The type-size limits: a type's inherent methods, summed across every
//! `impl` block in its crate, stay at or under [`MAX_METHODS`], and a
//! struct's named fields stay at or under [`MAX_FIELDS`], unless
//! [`ALLOWED`] records the type with a reason. Runs as part of
//! `cargo test -p build-xtask` and `cargo xtask tidy`.
//!
//! The limits sit on types because a file has no length limit: a type's
//! methods may live in any number of files, so the count sums every
//! inherent `impl` block in the crate. Trait impls are not counted,
//! because the trait fixes their methods.
//!
//! The check reads syntax, so it keys a type by its crate and the last
//! segment of the name an `impl` gives it: two types of one name in one
//! crate are summed, and an `impl` that a macro expands to is not seen.
//! It reads each crate's `src/` tree, skipping test files by name and
//! every item behind a `cfg` that names `test`; a test-support file that
//! only a gated `mod` line reaches is still read.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use proc_macro2::{TokenStream, TokenTree};
use syn::{Attribute, Fields, Ident, ImplItem, Item, Meta, Type};

/// The most inherent methods one type may have, summed across every
/// `impl` block in its crate.
const MAX_METHODS: usize = 25;

/// The most named fields one struct may have.
const MAX_FIELDS: usize = 12;

/// The types over a limit today. A new entry is a plan decision with a
/// reason, never a way to make a change pass.
const ALLOWED: &[Allowed] = &[];

/// The limit an [`Allowed`] entry lifts.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Limit {
    /// [`MAX_METHODS`].
    Methods,
    /// [`MAX_FIELDS`].
    Fields,
}

impl Limit {
    /// The count every type without an entry is held to.
    fn bound(self) -> usize {
        match self {
            Limit::Methods => MAX_METHODS,
            Limit::Fields => MAX_FIELDS,
        }
    }

    /// What the limit counts, as a violation names it.
    fn noun(self) -> &'static str {
        match self {
            Limit::Methods => "inherent methods across its `impl` blocks",
            Limit::Fields => "named fields",
        }
    }

    /// The rule a type without an entry breaks by passing the limit.
    fn rule(self) -> &'static str {
        match self {
            Limit::Methods => "type over the inherent method limit",
            Limit::Fields => "struct over the named field limit",
        }
    }
}

/// A type that may pass one limit, up to the count recorded when the
/// entry was written. The type may shrink without the entry changing; it
/// fails when it grows past `recorded`, and the entry fails as stale once
/// the type is gone or back within the limit.
struct Allowed {
    /// The crate directory, relative to the workspace root, with `/`
    /// separators.
    krate: &'static str,
    /// The type's name.
    name: &'static str,
    /// The limit the entry lifts.
    limit: Limit,
    /// The most the type may count; one more fails.
    recorded: usize,
    /// What the type is and why its size holds, in one sentence.
    reason: &'static str,
}

/// Each counted type's count, keyed by crate directory, type name, and
/// limit, for the stale-entry pass.
type Counts = BTreeMap<(String, String, Limit), usize>;

/// A definition or `impl` block: its file, relative to the workspace root
/// with `/` separators, and its line.
#[derive(Clone)]
struct Site {
    path: String,
    line: usize,
}

/// One type's inherent methods: its first `impl` block, and how many
/// methods each file, relative to the crate, gives it.
struct Tally {
    first: Site,
    files: BTreeMap<String, usize>,
}

impl Tally {
    /// Each file's share, as `src/a.rs 8, src/b.rs 3`.
    fn breakdown(&self) -> String {
        self.files
            .iter()
            .map(|(file, count)| format!("{file} {count}"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// What one crate's sources hold.
#[derive(Default)]
struct Census {
    /// Each type's inherent methods, by type name.
    methods: BTreeMap<String, Tally>,
    /// Each struct with named fields: its name, its definition, and its
    /// field count.
    structs: Vec<(String, Site, usize)>,
    /// The first struct, enum, or union definition of each name.
    defined: BTreeMap<String, Site>,
}

/// Runs the limits over every workspace crate's `src/` tree, the crates
/// whose manifest the shared walk could not read included, against
/// [`ALLOWED`]. An unreadable or unparseable file is reported, not
/// skipped, and a scan that read no source at all fails: a file that was
/// never read cannot be shown within the limits.
#[must_use]
pub(crate) fn type_size_violations(root: &Path) -> Vec<String> {
    violations_against(root, ALLOWED)
}

/// [`type_size_violations`] against any allowlist, so a fixture can pass
/// its own.
fn violations_against(root: &Path, allowed: &[Allowed]) -> Vec<String> {
    let walk = crate::product::workspace_crates(root);
    let mut dirs: Vec<&PathBuf> = walk
        .crates
        .iter()
        .map(|krate| &krate.dir)
        .chain(&walk.unread)
        .collect();
    dirs.sort();
    let mut violations = Vec::new();
    let mut counts = Counts::new();
    let mut read = 0;
    for dir in dirs {
        let crate_dir = root.join(dir);
        let src = crate_dir.join("src");
        let mut files: Vec<PathBuf> = crate::tidy::rust_files(&src)
            .into_iter()
            .filter(|file| !is_test_file(&crate::tidy::slash_path(&src, file)))
            .collect();
        files.sort();
        read += files.len();
        let mut census = Census::default();
        for file in &files {
            census.read(root, &crate_dir, file, &mut violations);
        }
        let krate = crate::tidy::slash_path(root, &crate_dir);
        census.judge(&krate, allowed, &mut counts, &mut violations);
    }
    if read == 0 {
        return vec![format!(
            "{}: the type-size check read nothing: no workspace crate holds a `.rs` file under \
             `src/`, so none can be shown within the limits",
            root.join("crates").display()
        )];
    }
    violations.extend(entry_violations(allowed, &counts));
    violations
}

impl Census {
    /// Parses `file` and records its types, or reports why it could not.
    fn read(&mut self, root: &Path, crate_dir: &Path, file: &Path, violations: &mut Vec<String>) {
        let path = crate::tidy::slash_path(root, file);
        let text = match fs::read_to_string(file) {
            Ok(text) => text,
            Err(error) => {
                violations.push(format!("{path}: unreadable source: {error}"));
                return;
            }
        };
        match syn::parse_file(&text) {
            Ok(parsed) if gated_on_test(&parsed.attrs) => {}
            Ok(parsed) => {
                let local = crate::tidy::slash_path(crate_dir, file);
                self.items(&parsed.items, &path, &local);
            }
            Err(error) => violations.push(format!(
                "{path}:{}: unparseable source: required a file that parses as Rust, found {error}",
                error.span().start().line
            )),
        }
    }

    /// Records the inherent methods, structs, and definitions in `items`,
    /// inline modules included, from the file at `path` (`local` within
    /// its crate).
    fn items(&mut self, items: &[Item], path: &str, local: &str) {
        for item in items {
            match item {
                Item::Impl(item) if item.trait_.is_none() && !gated_on_test(&item.attrs) => {
                    let Some(name) = self_type_name(&item.self_ty) else {
                        continue;
                    };
                    let count = item
                        .items
                        .iter()
                        .filter(|member| {
                            matches!(member, ImplItem::Fn(function) if !gated_on_test(&function.attrs))
                        })
                        .count();
                    let tally = self.methods.entry(name).or_insert_with(|| Tally {
                        first: Site {
                            path: path.to_owned(),
                            line: item.impl_token.span.start().line,
                        },
                        files: BTreeMap::new(),
                    });
                    *tally.files.entry(local.to_owned()).or_default() += count;
                }
                Item::Struct(item) if !gated_on_test(&item.attrs) => {
                    if let Fields::Named(fields) = &item.fields {
                        self.structs.push((
                            item.ident.to_string(),
                            site(&item.ident, path),
                            fields.named.len(),
                        ));
                    }
                    self.define(&item.ident, path);
                }
                Item::Enum(item) if !gated_on_test(&item.attrs) => self.define(&item.ident, path),
                Item::Union(item) if !gated_on_test(&item.attrs) => self.define(&item.ident, path),
                Item::Mod(item) if !gated_on_test(&item.attrs) => {
                    if let Some((_, items)) = &item.content {
                        self.items(items, path, local);
                    }
                }
                _ => {}
            }
        }
    }

    /// Records `ident`'s definition in `path`, unless the name has one.
    fn define(&mut self, ident: &Ident, path: &str) {
        self.defined
            .entry(ident.to_string())
            .or_insert_with(|| site(ident, path));
    }

    /// Judges this crate's counts against the limits and `allowed`, and
    /// records each count in `counts` for [`entry_violations`].
    fn judge(
        &self,
        krate: &str,
        allowed: &[Allowed],
        counts: &mut Counts,
        violations: &mut Vec<String>,
    ) {
        for (name, tally) in &self.methods {
            let total: usize = tally.files.values().sum();
            counts.insert((krate.to_owned(), name.clone(), Limit::Methods), total);
            let site = self.defined.get(name).unwrap_or(&tally.first);
            let found = format!("`{name}` with {total} ({})", tally.breakdown());
            let entry = entry(allowed, krate, name, Limit::Methods);
            violations.extend(judge_one(site, entry, Limit::Methods, total, &found));
        }
        for (name, site, fields) in &self.structs {
            let count = counts
                .entry((krate.to_owned(), name.clone(), Limit::Fields))
                .or_default();
            *count = (*count).max(*fields);
            let found = format!("`{name}` with {fields}");
            let entry = entry(allowed, krate, name, Limit::Fields);
            violations.extend(judge_one(site, entry, Limit::Fields, *fields, &found));
        }
    }
}

/// The site of the item `ident` names, in `path`.
fn site(ident: &Ident, path: &str) -> Site {
    Site {
        path: path.to_owned(),
        line: ident.span().start().line,
    }
}

/// The entry in `allowed` for `name` in the crate at `krate` under `limit`.
fn entry<'a>(allowed: &'a [Allowed], krate: &str, name: &str, limit: Limit) -> Option<&'a Allowed> {
    allowed
        .iter()
        .find(|entry| entry.krate == krate && entry.name == name && entry.limit == limit)
}

/// The violation `count` makes against `limit` at `site`, where `entry` is
/// the allowlist's entry for the type, if it has one.
fn judge_one(
    site: &Site,
    entry: Option<&Allowed>,
    limit: Limit,
    count: usize,
    found: &str,
) -> Option<String> {
    let Site { path, line } = site;
    match entry {
        None if count > limit.bound() => Some(format!(
            "{path}:{line}: {}: required at most {} {}, or an allowlist entry with a reason, \
             found {found}",
            limit.rule(),
            limit.bound(),
            limit.noun()
        )),
        Some(entry) if count > entry.recorded => Some(format!(
            "{path}:{line}: allowlisted type past its recorded count: required at most {} {}, \
             the count its entry records, found {found}",
            entry.recorded,
            limit.noun()
        )),
        _ => None,
    }
}

/// The entries in `allowed` with a blank reason, and the stale ones: a type
/// the scan never counted, or one back within its limit.
fn entry_violations(allowed: &[Allowed], counts: &Counts) -> Vec<String> {
    const ALLOWLIST: &str = "crates/build-xtask/src/type_size.rs";
    let mut violations = Vec::new();
    for entry in allowed {
        let Allowed {
            krate,
            name,
            limit,
            reason,
            ..
        } = *entry;
        if reason.trim().is_empty() {
            violations.push(format!(
                "{ALLOWLIST}: allowlist entry without a reason: required one sentence saying what \
                 `{name}` in `{krate}` is and why its size holds, found a blank reason"
            ));
        }
        let required = format!(
            "`{name}` in `{krate}` over {} {}",
            limit.bound(),
            limit.noun()
        );
        match counts.get(&(krate.to_owned(), name.to_owned(), limit)) {
            None => violations.push(format!(
                "{ALLOWLIST}: stale allowlist entry: required {required}, found no such type"
            )),
            Some(count) if *count <= limit.bound() => violations.push(format!(
                "{ALLOWLIST}: stale allowlist entry: required {required}, found {count}"
            )),
            Some(_) => {}
        }
    }
    violations
}

/// Whether `path`, relative to a crate's `src/` with `/` separators, is a
/// test file: one under a `tests/` directory, or one whose name marks it as
/// tests (`tests.rs`, `foo-tests.rs`, `foo-tests-bar.rs`, `tests-foo.rs`,
/// or `foo_tests.rs`).
fn is_test_file(path: &str) -> bool {
    let mut parts: Vec<&str> = path.split('/').collect();
    let stem = parts
        .pop()
        .map_or("", |file| file.strip_suffix(".rs").unwrap_or(file));
    parts.contains(&"tests")
        || stem == "tests"
        || stem.ends_with("-tests")
        || stem.contains("-tests-")
        || stem.starts_with("tests-")
        || stem.ends_with("_tests")
}

/// Whether `attrs` hold a `cfg` that names `test` outside a `not`, as
/// `cfg(test)` and `cfg(any(test, feature = "..."))` do.
fn gated_on_test(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg")
            && matches!(&attr.meta, Meta::List(list) if names_test(list.tokens.clone()))
    })
}

/// Whether `tokens` name `test` at any depth outside a `not(...)`.
fn names_test(tokens: TokenStream) -> bool {
    let mut tokens = tokens.into_iter();
    while let Some(token) = tokens.next() {
        match token {
            TokenTree::Ident(ident) if ident == "not" => {
                tokens.next();
            }
            TokenTree::Ident(ident) if ident == "test" => return true,
            TokenTree::Group(group) if names_test(group.stream()) => return true,
            _ => {}
        }
    }
    false
}

/// The last path segment of an `impl`'s self type, through any grouping,
/// or `None` for a self type that is not a plain path.
fn self_type_name(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(path) if path.qself.is_none() => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string()),
        Type::Group(group) => self_type_name(&group.elem),
        Type::Paren(paren) => self_type_name(&paren.elem),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;
    use std::path::Path;

    use super::*;
    use crate::product::test_support::workspace_root;

    /// Writes a crate at `crates/fixture/` holding each `(path, text)`
    /// under its `src/`.
    fn write_fixture(root: &Path, files: &[(&str, &str)]) {
        crate::product::test_support::write_crate(root, "fixture", "fixture", "");
        let src = root.join("crates").join("fixture").join("src");
        for (path, text) in files {
            let file = src.join(path);
            let dir = file.parent().expect("a fixture file sits in a directory");
            fs::create_dir_all(dir).expect("the fixture directory creates");
            fs::write(&file, text).expect("the fixture file writes");
        }
    }

    /// An inherent `impl` of `name` holding `count` methods named after `tag`.
    fn methods(name: &str, tag: &str, count: usize) -> String {
        let mut text = format!("impl {name} {{\n");
        for index in 0..count {
            writeln!(text, "    fn {tag}_{index}(&self) {{}}").expect("a String takes every write");
        }
        text + "}\n"
    }

    /// A struct `name` with `count` named fields.
    fn fields(name: &str, count: usize) -> String {
        let mut text = format!("struct {name} {{\n");
        for index in 0..count {
            writeln!(text, "    f{index}: u8,").expect("a String takes every write");
        }
        text + "}\n"
    }

    /// An entry lifting `limit` for `name` in the fixture crate.
    fn fixture_entry(
        name: &'static str,
        limit: Limit,
        recorded: usize,
        reason: &'static str,
    ) -> Allowed {
        Allowed {
            krate: "crates/fixture",
            name,
            limit,
            recorded,
            reason,
        }
    }

    #[test]
    fn the_workspace_keeps_every_type_within_its_size_limits() {
        let violations = type_size_violations(&workspace_root());
        assert!(
            violations.is_empty(),
            "type-size violations:\n{}",
            violations.join("\n")
        );
    }

    #[test]
    fn methods_are_summed_across_every_inherent_impl_block_in_the_crate() {
        let root = tempfile::TempDir::new().expect("tempdir");
        let lib = format!("mod more;\npub struct Wide;\n{}", methods("Wide", "a", 13));
        let more = format!("use super::Wide;\n{}", methods("Wide", "b", 13));
        write_fixture(root.path(), &[("lib.rs", &lib), ("more.rs", &more)]);
        let violations = violations_against(root.path(), &[]);
        assert_eq!(violations.len(), 1, "{violations:?}");
        assert!(
            violations[0].starts_with(
                "crates/fixture/src/lib.rs:2: type over the inherent method limit: required at \
                 most 25 inherent methods"
            ) && violations[0].ends_with("found `Wide` with 26 (src/lib.rs 13, src/more.rs 13)"),
            "the type is named at its definition with each file's share: {violations:?}"
        );
    }

    #[test]
    fn trait_impl_methods_are_not_counted() {
        let root = tempfile::TempDir::new().expect("tempdir");
        let lib = format!(
            "struct Wide;\n{}impl Clone for Wide {{\n    fn clone(&self) -> Self {{\n        \
             Wide\n    }}\n}}\n",
            methods("Wide", "a", 25)
        );
        write_fixture(root.path(), &[("lib.rs", &lib)]);
        let violations = violations_against(root.path(), &[]);
        assert!(violations.is_empty(), "{violations:?}");
    }

    #[test]
    fn a_struct_over_the_field_limit_is_reported_at_its_definition() {
        let root = tempfile::TempDir::new().expect("tempdir");
        let lib = format!(
            "//! Fixture.\n{}{}",
            fields("Narrow", 12),
            fields("Broad", 13)
        );
        write_fixture(root.path(), &[("lib.rs", &lib)]);
        let violations = violations_against(root.path(), &[]);
        assert_eq!(violations.len(), 1, "{violations:?}");
        assert!(
            violations[0].starts_with(
                "crates/fixture/src/lib.rs:16: struct over the named field limit: required at \
                 most 12 named fields"
            ) && violations[0].ends_with("found `Broad` with 13"),
            "{violations:?}"
        );
    }

    #[test]
    fn an_allowlisted_type_passes_at_its_recorded_count_and_fails_one_past_it() {
        let root = tempfile::TempDir::new().expect("tempdir");
        let allowed = [fixture_entry("Wide", Limit::Methods, 27, "a fixture type")];
        let at = format!("struct Wide;\n{}", methods("Wide", "a", 27));
        write_fixture(root.path(), &[("lib.rs", &at)]);
        let violations = violations_against(root.path(), &allowed);
        assert!(violations.is_empty(), "{violations:?}");

        let past = format!("struct Wide;\n{}", methods("Wide", "a", 28));
        write_fixture(root.path(), &[("lib.rs", &past)]);
        let violations = violations_against(root.path(), &allowed);
        assert_eq!(violations.len(), 1, "{violations:?}");
        assert!(
            violations[0].contains("allowlisted type past its recorded count: required at most 27")
                && violations[0].ends_with("found `Wide` with 28 (src/lib.rs 28)"),
            "{violations:?}"
        );
    }

    #[test]
    fn stale_entries_and_a_blank_reason_are_reported() {
        let root = tempfile::TempDir::new().expect("tempdir");
        let allowed = [
            fixture_entry("Gone", Limit::Methods, 30, "a type that no longer exists"),
            fixture_entry("Small", Limit::Fields, 20, "a struct back within the limit"),
            fixture_entry("Wide", Limit::Methods, 26, " "),
        ];
        let lib = format!(
            "{}struct Wide;\n{}",
            fields("Small", 12),
            methods("Wide", "a", 26)
        );
        write_fixture(root.path(), &[("lib.rs", &lib)]);
        let violations = violations_against(root.path(), &allowed);
        assert_eq!(violations.len(), 3, "{violations:?}");
        assert!(
            violations[0].contains("stale allowlist entry: required `Gone` in `crates/fixture`")
                && violations[0].ends_with("found no such type"),
            "a type the scan never counted is stale: {violations:?}"
        );
        assert!(
            violations[1].contains("stale allowlist entry: required `Small` in `crates/fixture`")
                && violations[1].ends_with("found 12"),
            "a type back within its limit is stale: {violations:?}"
        );
        assert!(
            violations[2].contains("allowlist entry without a reason")
                && violations[2].contains("`Wide`"),
            "an entry needs a reason: {violations:?}"
        );
    }

    #[test]
    fn test_files_and_items_gated_on_test_are_not_counted() {
        let root = tempfile::TempDir::new().expect("tempdir");
        let wide = methods("Wide", "a", 30);
        let lib = format!(
            "pub struct Wide;\n#[cfg(test)]\nmod tests {{\n{wide}}}\n\
             #[cfg(any(test, feature = \"fixture\"))]\n{wide}"
        );
        let gated = format!("#![cfg(test)]\n{wide}");
        write_fixture(
            root.path(),
            &[
                ("lib.rs", &lib),
                ("support.rs", &gated),
                ("tests.rs", &wide),
                ("wide-tests.rs", &wide),
                ("wide-tests-more.rs", &wide),
                ("tests-wide.rs", &wide),
                ("wide_tests.rs", &wide),
                ("inner/tests/deep.rs", &wide),
            ],
        );
        let violations = violations_against(root.path(), &[]);
        assert!(violations.is_empty(), "{violations:?}");

        let ungated = format!("pub struct Wide;\n#[cfg(not(test))]\n{wide}");
        write_fixture(root.path(), &[("lib.rs", &ungated)]);
        let violations = violations_against(root.path(), &[]);
        assert_eq!(
            violations.len(),
            1,
            "a `not(test)` item is counted: {violations:?}"
        );
    }

    #[test]
    fn methods_in_inline_modules_count_and_test_gated_methods_do_not() {
        let root = tempfile::TempDir::new().expect("tempdir");
        let lib = format!(
            "pub struct Wide;\nmod inner {{\n{}}}\nimpl Wide {{\n    #[cfg(test)]\n    fn \
             helper(&self) {{}}\n}}\n",
            methods("Wide", "a", 26)
        );
        write_fixture(root.path(), &[("lib.rs", &lib)]);
        let violations = violations_against(root.path(), &[]);
        assert_eq!(violations.len(), 1, "{violations:?}");
        assert!(
            violations[0].ends_with("found `Wide` with 26 (src/lib.rs 26)"),
            "{violations:?}"
        );
    }

    #[test]
    fn a_crate_whose_manifest_does_not_parse_is_still_scanned() {
        let root = tempfile::TempDir::new().expect("tempdir");
        let lib = format!("struct Wide;\n{}", methods("Wide", "a", 26));
        write_fixture(root.path(), &[("lib.rs", &lib)]);
        fs::write(
            root.path()
                .join("crates")
                .join("fixture")
                .join("Cargo.toml"),
            "not [valid toml",
        )
        .expect("the manifest rewrites");
        let violations = violations_against(root.path(), &[]);
        assert_eq!(violations.len(), 1, "{violations:?}");
        assert!(
            violations[0].contains("type over the inherent method limit"),
            "a crate whose manifest was never read cannot be shown within the limits: \
             {violations:?}"
        );
    }

    #[test]
    fn a_source_file_that_does_not_parse_is_reported() {
        let root = tempfile::TempDir::new().expect("tempdir");
        write_fixture(root.path(), &[("lib.rs", "fn broken( {\n")]);
        let violations = violations_against(root.path(), &[]);
        assert_eq!(violations.len(), 1, "{violations:?}");
        assert!(
            violations[0].starts_with("crates/fixture/src/lib.rs:")
                && violations[0].contains("unparseable source"),
            "a file that was never read cannot be shown within the limits: {violations:?}"
        );
    }

    #[test]
    fn a_type_size_scan_that_reads_no_source_fails() {
        let root = tempfile::TempDir::new().expect("tempdir");
        fs::create_dir_all(root.path().join("crates")).expect("the crates directory creates");
        let violations = violations_against(root.path(), &[]);
        assert_eq!(violations.len(), 1, "{violations:?}");
        assert!(
            violations[0].contains("read nothing"),
            "a check that read no source cannot show it within the limits: {violations:?}"
        );
    }
}
