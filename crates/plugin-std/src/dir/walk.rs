//! The walk: lists a directory tree breadth first and stops at the byte
//! cap, so a huge tree is never read whole.
//!
//! The walk returns a [`Tree`] and writes no text. Each directory's
//! entries are sorted here, by name with `/` added to a directory's name,
//! because `Access::list` promises no order. That order equals the order
//! `Access::glob` gives, which sorts full path strings bytewise.

use std::collections::VecDeque;

use runvil_plugin::{Entry, FileType, VfsError};

use crate::matcher::Matcher;

/// The name of the entry the walk skips, a file or a directory, at any
/// depth.
const GIT: &str = ".git";

/// One line of the tree: a name under its parent.
#[derive(Debug)]
pub(super) struct Node {
    /// The entry's name. The root's is empty.
    pub(super) name: String,
    /// Whether the entry is a directory, which ends in `/` and is entered.
    /// A link and every other kind of node is not.
    pub(super) directory: bool,
    /// Whether the walk got as far as showing this directory's contents.
    /// It is `false` only for a directory that was cut off before its
    /// first child was shown, and it prints with `...` after its name.
    pub(super) expanded: bool,
    /// The indexes in [`Tree::nodes`] of the entries shown under this one,
    /// in sorted order.
    pub(super) children: Vec<usize>,
}

/// The tree the walk found.
#[derive(Debug)]
pub(super) struct Tree {
    /// The root's line: its absolute virtual path, ending in `/`.
    pub(super) root_line: String,
    /// Every node shown. The first is the root.
    pub(super) nodes: Vec<Node>,
    /// Whether the walk stopped because the next line would pass the cap.
    pub(super) truncated: bool,
}

/// A listing that failed and ended the walk.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Failure {
    /// The directory that could not be listed, written as the model wrote
    /// the target with the names below it added. For the root it is the
    /// target exactly as written.
    pub(super) path: String,
    /// Why the listing failed.
    pub(super) error: VfsError,
}

/// A directory the walk still has to list.
struct Pending {
    /// Its index in the tree's nodes.
    node: usize,
    /// The path the lister is given for it.
    path: String,
    /// The depth of the entries under it: 1 for the root's.
    depth: usize,
}

/// Walks the directory `target`, breadth first, calling `list` for each
/// directory it enters.
///
/// `target` is the path as the model wrote it. `ignore` leaves out each
/// entry whose absolute virtual path a pattern matches, and a left-out
/// directory is not entered. The walk counts the bytes of every line it
/// adds, the root's included, each with its newline, and when the next line
/// would pass `cap` it stops everything: the directory being listed keeps
/// the children already added, and every directory shown but not expanded
/// is marked.
///
/// # Errors
/// Returns the lister's error, with the directory it was for, when the
/// root cannot be listed, and for a directory below the root when the
/// failure is anything but the directory being gone or no longer a
/// directory. A claim conflict or a refusal by policy is returned rather
/// than shown as an empty directory, because the model would read
/// `locked/` as empty when its contents were never seen. Only a directory
/// that is gone, or is no longer a directory, is shown with no children,
/// because an empty look at it is true.
pub(super) fn walk<F>(
    target: &str,
    ignore: &[Matcher],
    cap: usize,
    mut list: F,
) -> Result<Tree, Failure>
where
    F: FnMut(&str) -> Result<Vec<Entry>, VfsError>,
{
    let root_line = root_line(target);
    let mut used = root_line.len() + 1;
    let mut nodes = vec![Node {
        name: String::new(),
        directory: true,
        expanded: true,
        children: Vec::new(),
    }];
    let mut queue = VecDeque::from([Pending {
        node: 0,
        path: directory_path(target),
        depth: 1,
    }]);
    let mut truncated = false;
    while let Some(pending) = queue.pop_front() {
        let entries = match list(&pending.path) {
            Ok(entries) => entries,
            Err(error) if pending.node != 0 && is_gone(&error) => Vec::new(),
            Err(error) => {
                let path = if pending.node == 0 {
                    target.to_owned()
                } else {
                    pending.path
                };
                return Err(Failure { path, error });
            }
        };
        for entry in visible(entries, &pending.path, ignore) {
            let directory = matches!(entry.stat.file_type, FileType::Directory);
            let cost = line_cost(pending.depth, &entry.name, directory);
            if used + cost > cap {
                truncated = true;
                break;
            }
            used += cost;
            let index = nodes.len();
            if directory {
                queue.push_back(Pending {
                    node: index,
                    path: join(&pending.path, &entry.name),
                    depth: pending.depth + 1,
                });
            }
            nodes.push(Node {
                name: entry.name,
                directory,
                expanded: !directory,
                children: Vec::new(),
            });
            nodes[pending.node].children.push(index);
        }
        let listed = &mut nodes[pending.node];
        listed.expanded = !truncated || !listed.children.is_empty();
        if truncated {
            break;
        }
    }
    Ok(Tree {
        root_line,
        nodes,
        truncated,
    })
}

/// Whether `error` from listing a directory below the root means the
/// directory is gone, or is no longer a directory, so showing it with no
/// children is true.
fn is_gone(error: &VfsError) -> bool {
    matches!(
        error,
        VfsError::NotFound { .. } | VfsError::NotADirectory { .. }
    )
}

/// The entries of one directory the tree shows, in order: everything but
/// `.git` and what `ignore` matches, sorted by name with `/` added to a
/// directory's name.
fn visible(entries: Vec<Entry>, directory: &str, ignore: &[Matcher]) -> Vec<Entry> {
    let mut kept: Vec<Entry> = entries
        .into_iter()
        .filter(|entry| entry.name != GIT && !is_ignored(directory, &entry.name, ignore))
        .collect();
    kept.sort_by_cached_key(|entry| {
        if matches!(entry.stat.file_type, FileType::Directory) {
            format!("{}/", entry.name)
        } else {
            entry.name.clone()
        }
    });
    kept
}

/// Whether a pattern in `ignore` matches the entry `name` of `directory`,
/// by the entry's absolute virtual path, which has no trailing `/` even
/// for a directory.
fn is_ignored(directory: &str, name: &str, ignore: &[Matcher]) -> bool {
    if ignore.is_empty() {
        return false;
    }
    let path = virtual_path(&join(directory, name));
    ignore.iter().any(|matcher| matcher.is_match(&path))
}

/// The bytes a line costs against the cap: the indent of two spaces per
/// level, the bullet, the name, the `/` of a directory, and the newline.
fn line_cost(depth: usize, name: &str, directory: bool) -> usize {
    2 * depth + "- ".len() + name.len() + usize::from(directory) + 1
}

/// `name` inside `directory`, with one `/` between them.
fn join(directory: &str, name: &str) -> String {
    if directory.ends_with('/') {
        format!("{directory}{name}")
    } else {
        format!("{directory}/{name}")
    }
}

/// The path the lister is given for the root: the target as written,
/// without a trailing `/`, and `/` alone for the filesystem root.
fn directory_path(target: &str) -> String {
    let trimmed = target.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// `path` with a leading `/` added when it lacks one. No canonical path is
/// available, so this is as far as the model's path is turned into an
/// absolute one.
fn virtual_path(path: &str) -> String {
    if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("/{path}")
    }
}

/// The root's line: the target as an absolute virtual path, with `/`
/// appended unless it already ends in one.
fn root_line(target: &str) -> String {
    let mut line = virtual_path(target);
    if !line.ends_with('/') {
        line.push('/');
    }
    line
}
