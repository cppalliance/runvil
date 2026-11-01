//! The file operations Grep needs, behind a trait.
//!
//! The call's [`Access`] implements it, so every operation Grep performs
//! goes through the run's mode and policy. The seam exists so that a test
//! can supply what the in-memory backend cannot produce: a symbolic link,
//! a name with a control character, an over-size entry, a denied read, and
//! a claim conflict.

use runvil_plugin::{Access, Entry, Stat, VfsError};

/// Where Grep reads the tree from.
pub(super) trait Source {
    /// Metadata for `path`, without following a link.
    fn stat(&self, path: &str) -> Result<Stat, VfsError>;

    /// The entries of the directory at `path`, in no promised order.
    fn list(&self, path: &str) -> Result<Vec<Entry>, VfsError>;

    /// The whole contents of the file at `path`.
    fn read(&self, path: &str) -> Result<Vec<u8>, VfsError>;

    /// At most the first `len` bytes of the file at `path`, for a path
    /// whose size is not known beforehand.
    fn read_prefix(&self, path: &str, len: u64) -> Result<Vec<u8>, VfsError>;
}

impl Source for Access {
    fn stat(&self, path: &str) -> Result<Stat, VfsError> {
        Access::stat(self, path)
    }

    fn list(&self, path: &str) -> Result<Vec<Entry>, VfsError> {
        Access::list(self, path)
    }

    fn read(&self, path: &str) -> Result<Vec<u8>, VfsError> {
        Access::read(self, path)
    }

    fn read_prefix(&self, path: &str, len: u64) -> Result<Vec<u8>, VfsError> {
        Access::read_range(self, path, 0, len)
    }
}
