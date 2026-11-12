//! Two demos of the virtual filesystem, each a read-only mount in every
//! run behind a switch that is committed as off.
//!
//! [`ENABLED`] mounts the Runvil prompting language guide at [`MOUNT`], so
//! an agent can read the language's own documentation with the file
//! tools. The guide's pages are read from [`SOURCE`], a directory on one
//! machine. The chat agent's system prompt, which names the mount, is in
//! `chat.md` and is toggled there.
//!
//! [`C_DRIVE`] mounts this machine's whole C drive at [`C_DRIVE_MOUNT`],
//! so the shell and the file tools can be tried on real files.
//!
//! Both are hacks, and both are all here. With both switches off, a run
//! has its plain in-memory root.

use std::path::Path;

use runvil::vfs::{RealBackend, VfsRef, Writability};

/// The switch: `true` mounts the guide in every run.
const ENABLED: bool = false;

/// The directory on this machine that holds the guide's pages.
const SOURCE: &str = r"C:\Users\Vinnie\cursor\runvil-docs\src\language";

/// The virtual path a run sees the guide at.
const MOUNT: &str = "/_runvil/language-guide";

/// The switch: `true` mounts the whole C drive, read-only, in every run.
///
/// It is committed as `false`. While it is on, a model can read every
/// file the person who runs Workshop can, what it reads goes to the
/// model's provider, and a search that starts at `/` walks the drive.
const C_DRIVE: bool = false;

/// The root of the drive [`C_DRIVE`] mounts.
const C_DRIVE_SOURCE: &str = r"C:\";

/// The virtual path a run sees the drive at, so `C:\Users` is `/c/Users`,
/// the path Git Bash gives it.
const C_DRIVE_MOUNT: &str = "/c";

/// The filesystem for one run: the in-memory root, with the guide mounted
/// at [`MOUNT`] when [`ENABLED`], and the C drive mounted at
/// [`C_DRIVE_MOUNT`] when [`C_DRIVE`].
pub(super) fn run_vfs() -> VfsRef {
    let mut vfs = VfsRef::default();
    if ENABLED {
        vfs = mount_at(vfs, MOUNT, Path::new(SOURCE));
    }
    if C_DRIVE {
        vfs = mount_at(vfs, C_DRIVE_MOUNT, Path::new(C_DRIVE_SOURCE));
    }
    vfs
}

/// `base` with the directory `source` mounted read-only at `mount`, an
/// absolute virtual path. A `source` that cannot be mounted leaves `base`
/// as it is and logs a warning, because a run without the mount still
/// works. So a machine with no such directory, as one with no C drive,
/// runs with the switch on as it does with it off.
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
