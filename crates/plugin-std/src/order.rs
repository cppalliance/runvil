//! The order and the one name the directory-walking tools share.
//!
//! ListDir and Grep both walk a directory tree, and the order they visit
//! entries in is a promise to the model: it is the order `Access::glob`
//! gives, so a walk and a glob over the same tree list the same paths in
//! the same sequence. Both tools take the order and the skipped name from
//! here, so the two cannot drift apart.

use runvil_plugin::{Entry, FileType};

/// The name of the entry a walk skips, a file or a directory, at any
/// depth.
pub(crate) const GIT: &str = ".git";

/// `entries` in the order a walk visits them: by name, with `/` added to
/// a directory's name.
///
/// `Access::list` promises no order. This is the order `Access::glob`
/// gives, which sorts full path strings bytewise.
pub(crate) fn sorted(mut entries: Vec<Entry>) -> Vec<Entry> {
    entries.sort_by_cached_key(|entry| {
        if matches!(entry.stat.file_type, FileType::Directory) {
            format!("{}/", entry.name)
        } else {
            entry.name.clone()
        }
    });
    entries
}

#[cfg(test)]
mod tests;
