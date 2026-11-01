//! A [`Source`] the tests build by hand, for what the in-memory backend
//! cannot produce: a link, a control-character name, an over-size entry,
//! a denied read, a claim conflict, and an I/O failure.

use std::collections::BTreeMap;

use runvil_plugin::{Entry, FileType, Stat, VfsError};

use crate::grep::source::Source;

/// One node of the fake tree.
#[derive(Debug, Clone)]
pub(super) enum Node {
    /// A file with these bytes.
    File(Vec<u8>),
    /// A symbolic link to a directory.
    Link,
    /// A symbolic link to a file with these bytes.
    LinkToFile(Vec<u8>),
    /// A pipe.
    Pipe,
    /// A file whose listing says it holds this many bytes, and whose
    /// bytes are the second field.
    Sized(u64, Vec<u8>),
    /// A directory whose listing fails with this error.
    ListFails(VfsError),
    /// A file whose read fails with this error.
    ReadFails(VfsError),
}

/// A tree of nodes by absolute path, with `/` always a directory.
#[derive(Debug, Default)]
pub(super) struct FakeSource {
    /// Every node but the root.
    nodes: BTreeMap<String, Node>,
}

impl FakeSource {
    /// The tree with `node` added at `path`.
    pub(super) fn with(mut self, path: &str, node: Node) -> FakeSource {
        self.nodes.insert(path.to_owned(), node);
        self
    }

    /// The tree with the text file `text` at `path`.
    pub(super) fn file(self, path: &str, text: &str) -> FakeSource {
        self.with(path, Node::File(text.as_bytes().to_vec()))
    }

    /// The metadata of `node`.
    fn stat_of(node: &Node) -> Stat {
        match node {
            Node::File(bytes) => Stat::new(FileType::File, bytes.len() as u64),
            Node::Sized(size, _) => Stat::new(FileType::File, *size),
            Node::ReadFails(_) => Stat::new(FileType::File, 1),
            Node::ListFails(_) => Stat::new(FileType::Directory, 0),
            Node::Link | Node::LinkToFile(_) => Stat::new(FileType::Symlink, 0),
            Node::Pipe => Stat::new(FileType::Fifo, 0),
        }
    }

    /// The error for a path that is not there.
    fn missing(path: &str) -> VfsError {
        VfsError::NotFound {
            path: path.to_owned(),
        }
    }
}

/// The directory that holds `path`, and the name it has there.
fn split(path: &str) -> (&str, &str) {
    match path.rsplit_once('/') {
        Some(("", name)) => ("/", name),
        Some((parent, name)) => (parent, name),
        None => ("/", path),
    }
}

impl Source for FakeSource {
    fn stat(&self, path: &str) -> Result<Stat, VfsError> {
        if path == "/" {
            return Ok(Stat::new(FileType::Directory, 0));
        }
        self.nodes
            .get(path)
            .map(Self::stat_of)
            .ok_or_else(|| Self::missing(path))
    }

    fn list(&self, path: &str) -> Result<Vec<Entry>, VfsError> {
        if path != "/" {
            match self.nodes.get(path) {
                None => return Err(Self::missing(path)),
                Some(Node::ListFails(error)) => return Err(error.clone()),
                Some(Node::Link) => {}
                Some(_) => {
                    return Err(VfsError::NotADirectory {
                        path: path.to_owned(),
                    });
                }
            }
        }
        Ok(self
            .nodes
            .iter()
            .filter(|(child, _)| split(child).0 == path)
            .map(|(child, node)| Entry::new(split(child).1, Self::stat_of(node)))
            .collect())
    }

    fn read(&self, path: &str) -> Result<Vec<u8>, VfsError> {
        match self.nodes.get(path) {
            Some(Node::File(bytes) | Node::Sized(_, bytes) | Node::LinkToFile(bytes)) => {
                Ok(bytes.clone())
            }
            Some(Node::ReadFails(error)) => Err(error.clone()),
            Some(Node::Link | Node::ListFails(_)) => Err(VfsError::IsADirectory {
                path: path.to_owned(),
            }),
            Some(Node::Pipe) | None => Err(Self::missing(path)),
        }
    }

    fn read_prefix(&self, path: &str, len: u64) -> Result<Vec<u8>, VfsError> {
        let mut bytes = self.read(path)?;
        bytes.truncate(usize::try_from(len).unwrap_or(usize::MAX));
        Ok(bytes)
    }
}
