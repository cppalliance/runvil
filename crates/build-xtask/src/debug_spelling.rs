//! The `Debug` spelling rule, live: the code base names the `Debug` trait
//! one way, `fmt::Debug` with `use std::fmt;` in scope. No line of any
//! `.rs` file in a workspace crate spells the trait by its full `std` path,
//! and no group import from `std::fmt` lists `Debug` as an item, bare or
//! renamed, because that brings in a bare `Debug`, a second spelling. Every
//! other `std::fmt` path, the `Debug`-prefixed builders included, passes.
//! Runs as part of `cargo test -p build-xtask` and `cargo xtask tidy`.
//!
//! No clippy lint can tell the spellings apart, because both resolve to the
//! same trait, so the check reads source text rather than tokens: the rule
//! is about the spelling wherever it appears, in code, comments, doc
//! comments, string literals, and macro bodies alike.

use std::fs;
use std::path::{Path, PathBuf};

const RULE: &str = "Debug trait spelled with its std path";
const REQUIRED: &str = "`fmt::Debug`, with `use std::fmt;` in scope";

/// The trait's full path. The split keeps this crate's source from
/// spelling the path it refuses.
const DEBUG_PATH: &str = concat!("std::fmt", "::Debug");

/// The opening of a group import from `std::fmt`.
const GROUP_OPEN: &str = "std::fmt::{";

/// Runs the rule over every `.rs` file in every workspace crate, the
/// crates whose manifest the shared walk could not read included. An
/// unreadable file is reported, not skipped, and a scan that found no
/// source at all fails: a file that was never scanned cannot be shown
/// clean.
#[must_use]
pub(crate) fn debug_spelling_violations(root: &Path) -> Vec<String> {
    let walk = crate::product::workspace_crates(root);
    let mut files: Vec<PathBuf> = walk
        .crates
        .iter()
        .map(|krate| &krate.dir)
        .chain(&walk.unread)
        .flat_map(|dir| crate::tidy::rust_files(&root.join(dir)))
        .collect();
    if files.is_empty() {
        return vec![format!(
            "{}: the Debug spelling check scanned nothing: no workspace crate holds a `.rs` \
             file, so none can be shown free of the trait's long spelling",
            root.join("crates").display()
        )];
    }
    files.sort();
    let mut violations = Vec::new();
    for file in &files {
        let path = crate::tidy::slash_path(root, file);
        match fs::read_to_string(file) {
            Ok(text) => violations.extend(source_violations(&path, &text)),
            Err(error) => violations.push(format!("{path}: unreadable source: {error}")),
        }
    }
    violations
}

/// The refused spellings in one source file, at most one per line,
/// labeled with `path`.
fn source_violations(path: &str, text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut flagged: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| holds_debug_path(line))
        .map(|(index, _)| index + 1)
        .collect();
    flagged.extend(group_debug_lines(text));
    flagged.sort_unstable();
    flagged.dedup();
    flagged
        .into_iter()
        .map(|line| {
            let found = line
                .checked_sub(1)
                .and_then(|index| lines.get(index))
                .map_or("", |text| text.trim());
            format!("{path}:{line}: {RULE}: required {REQUIRED}, found `{found}`")
        })
        .collect()
}

/// Whether `line` holds the trait's full path as a whole path, with no
/// identifier character touching it on either side.
fn holds_debug_path(line: &str) -> bool {
    line.match_indices(DEBUG_PATH).any(|(start, path)| {
        is_boundary(line[..start].chars().next_back())
            && is_boundary(line[start + path.len()..].chars().next())
    })
}

/// The line of each `Debug` that a group import from `std::fmt` lists at
/// its top level, bare or renamed with `as`, on one line or across several.
/// A `;` before the closing brace ends the scan, because a group never
/// holds one.
fn group_debug_lines(text: &str) -> Vec<usize> {
    let mut lines = Vec::new();
    for (start, open) in text.match_indices(GROUP_OPEN) {
        if !is_boundary(text[..start].chars().next_back()) {
            continue;
        }
        let body = start + open.len();
        let mut depth = 0_usize;
        let mut item = body;
        for (offset, c) in text[body..].char_indices() {
            let at = body + offset;
            match c {
                '{' => depth += 1,
                '}' if depth > 0 => depth -= 1,
                ',' if depth == 0 => {
                    lines.extend(debug_item_line(text, item, at));
                    item = at + 1;
                }
                '}' => {
                    lines.extend(debug_item_line(text, item, at));
                    break;
                }
                ';' => break,
                _ => {}
            }
        }
    }
    lines
}

/// The line of the group item `text[start..end]` when it is `Debug`, bare
/// or renamed with `as`.
fn debug_item_line(text: &str, start: usize, end: usize) -> Option<usize> {
    let item = &text[start..end];
    let mut words = item.split_whitespace();
    if words.next() != Some("Debug") || !matches!(words.next(), None | Some("as")) {
        return None;
    }
    let name = start + (item.len() - item.trim_start().len());
    Some(text[..name].matches('\n').count() + 1)
}

/// Whether `c`, the character beside a match, leaves it a whole path: it is
/// absent or cannot continue an identifier.
fn is_boundary(c: Option<char>) -> bool {
    !c.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
#[path = "debug_spelling-tests.rs"]
mod tests;
