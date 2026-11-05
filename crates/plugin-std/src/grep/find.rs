//! The search: walks the tree, reads each file, and gathers the entries
//! the answer shows.
//!
//! [`find`] is a pure function of its [`Source`]: it reads no clock, keeps
//! nothing between calls, and returns what it found without writing any
//! text. It yields to the executor now and then, so a running search can
//! be canceled.

mod collect;

use runvil_plugin::{Entry, FileType, VfsError};

use super::args::{Mode, Request};
use super::found::{Found, Limits, Reason, Stop};
use super::matching::each_match_line;
use super::pause::Pause;
use super::source::Source;
use super::types;
use crate::order::{GIT, sorted};
use crate::refusal::{missing_root, neither_file_nor_directory, refusal};
use crate::text::{decode, sniff_binary, split_lines};

/// A path as the file operations are given it, beside the absolute path
/// the answer prints.
///
/// The two differ only when the model wrote a path with no leading `/`:
/// the operations get it as written, and the answer adds the `/`.
struct Node {
    /// The path to pass to the [`Source`].
    access: String,
    /// The absolute virtual path to print and to match a glob against.
    shown: String,
}

impl Node {
    /// The node for the directory the model named, without a trailing `/`.
    fn directory(path: &str) -> Node {
        let trimmed = path.trim_end_matches('/');
        let access = if trimmed.is_empty() {
            "/".to_owned()
        } else {
            trimmed.to_owned()
        };
        let shown = if access.starts_with('/') {
            access.clone()
        } else {
            format!("/{access}")
        };
        Node { access, shown }
    }

    /// The node for the file the model named.
    fn file(path: &str) -> Node {
        let shown = if path.starts_with('/') {
            path.to_owned()
        } else {
            format!("/{path}")
        };
        Node {
            access: path.to_owned(),
            shown,
        }
    }

    /// The node for the entry `name` of this directory.
    fn join(&self, name: &str) -> Node {
        let slash = |path: &str| {
            if path.ends_with('/') {
                format!("{path}{name}")
            } else {
                format!("{path}/{name}")
            }
        };
        Node {
            access: slash(&self.access),
            shown: slash(&self.shown),
        }
    }
}

/// A directory the walk is part way through.
struct Frame {
    /// The directory.
    dir: Node,
    /// Its entries not yet visited, in the order they are visited.
    entries: std::vec::IntoIter<Entry>,
}

/// Searches the tree at the request's path.
///
/// `tool` is the name the model called the tool by.
///
/// # Errors
/// Returns the sentence the model reads when the root cannot be searched:
/// it does not exist, it cannot be listed or read, or it is neither a
/// file nor a directory. Nothing below the root fails the search.
pub(super) async fn find<S: Source>(
    source: &S,
    tool: &str,
    request: &Request<'_>,
    limits: &Limits,
) -> Result<Found, String> {
    let root = request.path.unwrap_or("/");
    let stat = match source.stat(root) {
        Ok(stat) => stat,
        Err(VfsError::NotFound { .. }) => return Err(missing_root(root)),
        Err(error) => return Err(refusal(tool, root, &error)),
    };
    let mut search = Search {
        source,
        tool,
        request,
        limits,
        found: Found::default(),
        cost: 0,
        considered: 0,
        bytes: 0,
        since_files: 0,
        since_bytes: 0,
        finished: false,
    };
    match stat.file_type {
        FileType::Directory => search.directory(root).await?,
        FileType::File => search.root_file(root, Some(stat.size))?,
        FileType::Symlink => search.link(root).await?,
        _ => return Err(neither_file_nor_directory(root)),
    }
    Ok(search.found)
}

/// One search in progress.
struct Search<'a, 'r, S: Source> {
    /// Where the tree is read from.
    source: &'a S,
    /// The name the model called the tool by.
    tool: &'a str,
    /// What the call asked for.
    request: &'a Request<'r>,
    /// The limits the search runs under.
    limits: &'a Limits,
    /// What has been found so far.
    found: Found,
    /// The bytes the kept lines will take in the answer, counted the way
    /// the render counts them.
    cost: usize,
    /// How many files have been considered.
    considered: usize,
    /// How many bytes have been read.
    bytes: u64,
    /// The files considered since the last pause.
    since_files: usize,
    /// The bytes read since the last pause.
    since_bytes: u64,
    /// Whether the search has everything it needs, or hit a limit.
    finished: bool,
}

impl<S: Source> Search<'_, '_, S> {
    /// Searches the directory the model named.
    async fn directory(&mut self, root: &str) -> Result<(), String> {
        let node = Node::directory(root);
        let entries = self
            .source
            .list(&node.access)
            .map_err(|error| refusal(self.tool, root, &error))?;
        self.walk(node, entries).await;
        Ok(())
    }

    /// Searches the link the model named: as a directory when it lists,
    /// and as a file when it does not.
    async fn link(&mut self, root: &str) -> Result<(), String> {
        let node = Node::directory(root);
        match self.source.list(&node.access) {
            Ok(entries) => {
                self.walk(node, entries).await;
                Ok(())
            }
            Err(VfsError::NotADirectory { .. }) => self.root_file(root, None),
            Err(error) => Err(refusal(self.tool, root, &error)),
        }
    }

    /// Searches the one file the model named. No ignore rule, glob, or
    /// type applies, because the model named it.
    ///
    /// `size` is the file's size when the stat gave it, and `None` for a
    /// link, whose own size says nothing about its target.
    fn root_file(&mut self, root: &str, size: Option<u64>) -> Result<(), String> {
        let node = Node::file(root);
        self.considered += 1;
        let bytes = if let Some(size) = size {
            if size > self.limits.max_file_size {
                self.skip(Reason::OverSize, Some(&node.shown));
                return Ok(());
            }
            self.source.read(&node.access)
        } else {
            self.source
                .read_prefix(&node.access, self.limits.max_file_size.saturating_add(1))
        }
        .map_err(|error| refusal(self.tool, root, &error))?;
        if size.is_none() && length(&bytes) > self.limits.max_file_size {
            self.skip(Reason::OverSize, Some(&node.shown));
            return Ok(());
        }
        self.bytes += length(&bytes);
        self.consume(&node.shown, &bytes);
        Ok(())
    }

    /// Walks the directory `root`, whose `entries` are already listed,
    /// depth first.
    async fn walk(&mut self, root: Node, entries: Vec<Entry>) {
        let mut stack = vec![Frame {
            dir: root,
            entries: sorted(entries).into_iter(),
        }];
        while let Some(frame) = stack.last_mut() {
            if self.finished {
                return;
            }
            let Some(entry) = frame.entries.next() else {
                stack.pop();
                continue;
            };
            if entry.name == GIT {
                continue;
            }
            let child = frame.dir.join(&entry.name);
            if entry.name.chars().any(|c| c < ' ' || c == '\u{7f}') {
                self.skip(Reason::ControlName, None);
                continue;
            }
            match entry.stat.file_type {
                FileType::Symlink => self.skip(Reason::SymbolicLink, Some(&child.shown)),
                FileType::Directory => match self.source.list(&child.access) {
                    Ok(entries) => stack.push(Frame {
                        dir: child,
                        entries: sorted(entries).into_iter(),
                    }),
                    Err(error) => self.skip(directory_reason(&error), Some(&child.shown)),
                },
                // A pipe, a socket, or a device is never opened: reading one
                // can block for good, so it is skipped as unreadable.
                FileType::File => {
                    self.visit(&child, &entry.name, entry.stat.size);
                    self.pace().await;
                }
                _ => self.skip(Reason::UnreadableFile, Some(&child.shown)),
            }
        }
    }

    /// Visits one file the walk met: counts it, applies the filters,
    /// reads it, and searches it.
    fn visit(&mut self, node: &Node, name: &str, size: u64) {
        if self.considered >= self.limits.max_files {
            self.finish(Stop::Files);
            return;
        }
        self.considered += 1;
        self.since_files += 1;
        if let Some((_, matcher)) = &self.request.glob
            && !matcher.is_match(&node.shown)
        {
            return;
        }
        if let Some(extensions) = self.request.kind
            && !types::matches(extensions, name)
        {
            return;
        }
        if size > self.limits.max_file_size {
            self.skip(Reason::OverSize, Some(&node.shown));
            return;
        }
        if self.bytes >= self.limits.max_bytes {
            self.finish(Stop::Bytes);
            return;
        }
        match self.source.read(&node.access) {
            Ok(bytes) => {
                self.bytes += length(&bytes);
                self.since_bytes += length(&bytes);
                self.consume(&node.shown, &bytes);
            }
            Err(error) => self.skip(file_reason(&error), Some(&node.shown)),
        }
    }

    /// Pauses when enough files or bytes have gone by since the last
    /// pause, so a cancel or a stop can drop the future.
    async fn pace(&mut self) {
        if self.since_files >= self.limits.slice_files
            || self.since_bytes >= self.limits.slice_bytes
        {
            self.since_files = 0;
            self.since_bytes = 0;
            Pause::default().await;
        }
    }

    /// Counts one skipped entry, and keeps its path for the skip line
    /// when it has a printable one.
    fn skip(&mut self, reason: Reason, path: Option<&str>) {
        self.found.skips.record(reason, path);
    }

    /// Ends the search at a limit.
    fn finish(&mut self, stop: Stop) {
        self.found.stop = Some(stop);
        self.finished = true;
    }

    /// Searches the bytes of the file at `path`.
    fn consume(&mut self, path: &str, bytes: &[u8]) {
        if sniff_binary(bytes) {
            self.skip(Reason::Binary, Some(path));
            return;
        }
        let decoded = decode(bytes);
        self.found.searched += 1;
        if decoded.invalid > 0 {
            self.found.replaced.record(path);
        }
        let text = decoded.text;
        let lines = split_lines(&text);
        match self.request.mode {
            Mode::Content => self.collect_lines(path, &text, &lines),
            Mode::FilesWithMatches => {
                let mut matched = false;
                each_match_line(
                    &self.request.regex,
                    self.request.multiline,
                    &text,
                    &lines,
                    |_, _| {
                        matched = true;
                        false
                    },
                );
                if matched {
                    self.collect_file(path, 0);
                }
            }
            Mode::Count => {
                let mut matching = 0;
                each_match_line(
                    &self.request.regex,
                    self.request.multiline,
                    &text,
                    &lines,
                    |_, _| {
                        matching += 1;
                        true
                    },
                );
                if matching > 0 {
                    self.collect_file(path, matching);
                }
            }
        }
    }
}

/// The length of `bytes` as a `u64`.
fn length(bytes: &[u8]) -> u64 {
    u64::try_from(bytes.len()).unwrap_or(u64::MAX)
}

/// Why a directory that failed to list is skipped.
fn directory_reason(error: &VfsError) -> Reason {
    match error {
        VfsError::Conflict { .. } => Reason::Conflict,
        VfsError::PermissionDenied { .. } | VfsError::ApprovalRequired { .. } => Reason::Denied,
        _ => Reason::UnreadableDirectory,
    }
}

/// Why a file that failed to read is skipped.
fn file_reason(error: &VfsError) -> Reason {
    match error {
        VfsError::Conflict { .. } => Reason::Conflict,
        VfsError::PermissionDenied { .. } | VfsError::ApprovalRequired { .. } => Reason::Denied,
        _ => Reason::UnreadableFile,
    }
}
