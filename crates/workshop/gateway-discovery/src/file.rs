//! The `gateway.json` type: what the Gateway writes after a successful bind
//! and what a reader validates before attaching.

use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::SidecarError;
use crate::paths::gateway_discovery_file_path;

/// The on-disk gateway discovery file naming a running gateway's loopback
/// port and bearer key.
///
/// Readers must tolerate unknown fields: a newer gateway may write fields
/// an older reader does not know, and serde ignores them. This is the six
/// fields the contract names; see `CONTRACT.md`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayDiscoveryFile {
    /// The gateway's bound port; readers connect to `127.0.0.1:{port}`.
    pub port: u16,
    /// The bearer key the gateway expects.
    pub api_key: String,
    /// The gateway process id.
    pub pid: u32,
    /// The gateway's boot instant in unix seconds.
    pub epoch: u64,
    /// The gateway crate version.
    pub version: String,
    /// RFC 3339 rendering of the boot instant.
    pub started_at: String,
}

impl fmt::Debug for GatewayDiscoveryFile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GatewayDiscoveryFile")
            .field("port", &self.port)
            .field("api_key", &"[REDACTED]")
            .field("pid", &self.pid)
            .field("epoch", &self.epoch)
            .field("version", &"[REDACTED]")
            .field("started_at", &"[REDACTED]")
            .finish()
    }
}

impl GatewayDiscoveryFile {
    /// Whether the connection names one particular Gateway boot.
    pub(crate) fn has_boot_identity(&self) -> bool {
        self.epoch != 0 && !self.started_at.trim().is_empty()
    }

    /// The reason the file fails validation, or `None` when it is valid.
    ///
    /// Validation covers the fields attach depends on: a real port, a
    /// real key, a real pid. The remaining fields are informational.
    #[must_use]
    pub fn validation_error(&self) -> Option<&'static str> {
        if self.port == 0 {
            return Some("port must not be 0");
        }
        if self.api_key.is_empty() {
            return Some("api_key must not be empty");
        }
        if self.api_key.contains(['\r', '\n']) {
            return Some("api_key must not contain line breaks");
        }
        if self.pid == 0 {
            return Some("pid must not be 0");
        }
        None
    }

    /// Reads and validates the gateway discovery file in `run_dir`,
    /// returning `None` when no file exists.
    ///
    /// # Errors
    /// Returns [`SidecarError::Read`] when the file exists but cannot be
    /// read, [`SidecarError::Parse`] when it is not valid JSON, and
    /// [`SidecarError::Invalid`] when it fails validation.
    pub fn read(run_dir: &Path) -> Result<Option<GatewayDiscoveryFile>, SidecarError> {
        let path = gateway_discovery_file_path(run_dir);
        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(SidecarError::Read {
                    path,
                    source: error,
                });
            }
        };
        let file: GatewayDiscoveryFile =
            serde_json::from_str(&raw).map_err(|source| SidecarError::Parse {
                path: path.clone(),
                source: source.into(),
            })?;
        if let Some(reason) = file.validation_error() {
            return Err(SidecarError::Invalid {
                path,
                reason: reason.to_owned(),
            });
        }
        Ok(Some(file))
    }
}

/// The writer half of the contract, compiled only for Workshop's test
/// doubles. The Gateway owns the production writer.
#[cfg(feature = "test-fixtures")]
impl GatewayDiscoveryFile {
    /// Writes the gateway discovery file into `run_dir`, creating the
    /// directory when missing. The write is atomic (temp sibling, sync,
    /// rename) and the file lands owner-only; see `CONTRACT.md` for the
    /// permission contract.
    ///
    /// # Errors
    /// Returns [`SidecarError::Invalid`] when the file fails validation,
    /// [`SidecarError::CreateDir`] when the run directory cannot be
    /// created, [`SidecarError::Serialize`] when the file cannot be
    /// serialized, and [`SidecarError::Write`] when the atomic write
    /// fails.
    pub fn write_to(&self, run_dir: &Path) -> Result<(), SidecarError> {
        let path = gateway_discovery_file_path(run_dir);
        if let Some(reason) = self.validation_error() {
            return Err(SidecarError::Invalid {
                path,
                reason: reason.to_owned(),
            });
        }
        fs::create_dir_all(run_dir).map_err(|source| SidecarError::CreateDir {
            path: run_dir.to_owned(),
            source,
        })?;
        let bytes = serde_json::to_vec_pretty(self).map_err(|source| SidecarError::Serialize {
            source: source.into(),
        })?;
        crate::atomic::write_atomic_owner_only(&path, &bytes)
            .map_err(|source| SidecarError::Write { path, source })
    }
}

/// Removes the gateway discovery file in `run_dir` when it still belongs to
/// process `pid`, so a shutting-down writer withdraws itself from discovery
/// but can never delete a replacement's file. A missing file is not an
/// error; an unreadable or foreign file is left alone for the next
/// [`crate::resolve`] to clean.
///
/// A test-double seam: the Gateway's own writer removes its file when it
/// exits, and Workshop's doubles need the same withdrawal.
///
/// # Errors
/// Returns [`SidecarError::Remove`] when the file is this process's own
/// but cannot be removed.
#[cfg(feature = "test-fixtures")]
pub fn remove_if_mine(run_dir: &Path, pid: u32) -> Result<(), SidecarError> {
    let path = gateway_discovery_file_path(run_dir);
    match fs::read_to_string(&path) {
        Ok(raw) => {
            let owned = serde_json::from_str::<GatewayDiscoveryFile>(&raw)
                .is_ok_and(|file| file.pid == pid);
            if !owned {
                return Ok(());
            }
        }
        // A missing or unreadable file is left alone.
        Err(_) => return Ok(()),
    }
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(SidecarError::Remove {
            path,
            source: error,
        }),
    }
}

#[cfg(test)]
#[path = "file-tests.rs"]
mod tests;
