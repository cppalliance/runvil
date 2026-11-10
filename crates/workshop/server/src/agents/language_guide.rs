//! A demo of the virtual filesystem: the Runvil prompting language guide,
//! mounted read-only in every run at [`MOUNT`], so an agent can read the
//! language's own documentation with the file tools.
//!
//! The guide's pages are read from [`SOURCE`], a directory on one
//! machine, so this is a hack, and it is all here. [`ENABLED`] is its one
//! switch: `true` mounts the guide in each run, and `false` leaves each
//! run its plain in-memory root. The chat agent's system prompt, which
//! names the mount, is in `chat.md` and is toggled there.

use std::path::Path;

use runvil_engine::vfs::{RealBackend, VfsRef, Writability};

/// The switch: `true` mounts the guide in every run.
const ENABLED: bool = false;

/// The directory on this machine that holds the guide's pages.
const SOURCE: &str = r"C:\Users\Vinnie\cursor\runvil-docs\src\language";

/// The virtual path a run sees the guide at.
const MOUNT: &str = "/_runvil/language-guide";

/// The filesystem for one run: the in-memory root, with the guide mounted
/// at [`MOUNT`] when [`ENABLED`].
pub(super) fn run_vfs() -> VfsRef {
    let base = VfsRef::default();
    if ENABLED {
        mount_at(base, MOUNT, Path::new(SOURCE))
    } else {
        base
    }
}

/// `base` with the directory `source` mounted read-only at `mount`, an
/// absolute virtual path. A `source` that cannot be mounted leaves `base`
/// as it is and logs a warning, because a run without the mount still
/// works.
fn mount_at(base: VfsRef, mount: &str, source: &Path) -> VfsRef {
    match RealBackend::rooted(source) {
        Ok(backend) => base.overlay(mount, backend.with_writability(Writability::ReadOnly)),
        Err(error) => {
            tracing::warn!(
                source = %source.display(),
                mount,
                %error,
                "a directory is not mounted; runs start without it"
            );
            base
        }
    }
}

#[cfg(test)]
mod tests;
