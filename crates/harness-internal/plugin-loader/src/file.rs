//! The file functions an `RvToolCallArgs` carries: `file_stat`,
//! `file_list`, and `file_read`, each answered from the access of the
//! step's tool call, so the run's policy applies to every read, and
//! [`Lent`], what the last of them handed back.

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};

use runvil_plugin::{Entry, FileType, Stat, VfsError};
use runvil_plugin_sys::{
    RV_ERR_FILE, RV_FILE_ERROR_CONFLICT, RV_FILE_ERROR_DENIED, RV_FILE_ERROR_IO,
    RV_FILE_ERROR_IS_A_DIRECTORY, RV_FILE_ERROR_NOT_A_DIRECTORY, RV_FILE_ERROR_NOT_FOUND,
    RV_FILE_ERROR_OTHER, RV_FILE_TYPE_DIRECTORY, RV_FILE_TYPE_FILE, RV_FILE_TYPE_OTHER,
    RV_FILE_TYPE_SYMLINK, RV_OK, RvBytes, RvDirEntries, RvDirEntry, RvFileError, RvFileErrorKind,
    RvStat, RvStatus, RvStr,
};

use crate::session::{Field, Out, answer_file, field, out, rv_str};

#[cfg(test)]
#[path = "file-tests.rs"]
mod tests;

const PATH: Field = field!("the path", 4096);
const STAT: Out = out!("file_stat", "stat");
const STAT_ERROR: Out = out!("file_stat", "error");
const ENTRIES: Out = out!("file_list", "entries");
const LIST_ERROR: Out = out!("file_list", "error");
const BYTES: Out = out!("file_read", "bytes");
const READ_ERROR: Out = out!("file_read", "error");

/// What a step's last file function handed the Plugin. The contract
/// keeps it valid until the step's next file function or the end of the
/// step, so each file function replaces it, and the step's end frees it.
#[derive(Default)]
pub(crate) struct Lent {
    /// The bytes of a read.
    bytes: Vec<u8>,
    /// The names the entries of a listing point at, kept only so that
    /// they stay alive.
    _names: Vec<String>,
    /// The entries of a listing.
    entries: Vec<RvDirEntry>,
    /// The text of a failure.
    text: String,
}

impl Lent {
    /// Keeps `bytes` and returns them as the contract hands them back.
    fn bytes(&mut self, bytes: Vec<u8>) -> RvBytes {
        *self = Lent {
            bytes,
            ..Lent::default()
        };
        RvBytes {
            ptr: self.bytes.as_ptr(),
            len: self.bytes.len(),
        }
    }

    /// Keeps `entries` and returns them as the contract hands them back.
    fn entries(&mut self, entries: Vec<Entry>) -> RvDirEntries {
        let stats: Vec<RvStat> = entries.iter().map(|entry| raw_stat(&entry.stat)).collect();
        let names: Vec<String> = entries.into_iter().map(|entry| entry.name).collect();
        // Each name's bytes sit in its own allocation, so moving `names`
        // into `self` leaves the pointers below valid.
        let entries = names
            .iter()
            .zip(stats)
            .map(|(name, stat)| RvDirEntry {
                name: rv_str(name),
                stat,
            })
            .collect();
        *self = Lent {
            _names: names,
            entries,
            ..Lent::default()
        };
        RvDirEntries {
            ptr: self.entries.as_ptr(),
            len: self.entries.len(),
        }
    }

    /// Keeps the text of `error` and returns the failure as the contract
    /// hands it back.
    fn error(&mut self, error: &VfsError) -> RvFileError {
        let (kind, text) = describe(error);
        *self = Lent {
            text,
            ..Lent::default()
        };
        RvFileError {
            kind,
            reserved: 0,
            text: rv_str(&self.text),
        }
    }
}

/// `stat` as the contract hands it back. A kind of node the contract
/// does not name is `RV_FILE_TYPE_OTHER`.
fn raw_stat(stat: &Stat) -> RvStat {
    RvStat {
        file_type: match stat.file_type {
            FileType::File => RV_FILE_TYPE_FILE,
            FileType::Directory => RV_FILE_TYPE_DIRECTORY,
            FileType::Symlink => RV_FILE_TYPE_SYMLINK,
            _ => RV_FILE_TYPE_OTHER,
        },
        reserved: 0,
        len: stat.size,
    }
}

/// The contract's kind for `error`, and the text a model reads.
///
/// A missing path, a path that is not a directory, and a path that is one
/// keep the error's own sentence. A policy denial carries the policy's
/// own reason. A conflict names the path and not the two identities that
/// claimed it, which mean nothing outside the Harness. An I/O failure
/// carries the kind of the failure and the system's description. Of the
/// errors the contract has no kind for, a refused path carries the rule
/// it broke, and every other one its sentence without the leading label.
fn describe(error: &VfsError) -> (RvFileErrorKind, String) {
    match error {
        VfsError::NotFound { .. } => (RV_FILE_ERROR_NOT_FOUND, error.to_string()),
        VfsError::NotADirectory { .. } => (RV_FILE_ERROR_NOT_A_DIRECTORY, error.to_string()),
        VfsError::IsADirectory { .. } => (RV_FILE_ERROR_IS_A_DIRECTORY, error.to_string()),
        VfsError::PermissionDenied { reason, .. } | VfsError::ApprovalRequired { reason, .. } => {
            (RV_FILE_ERROR_DENIED, reason.clone())
        }
        VfsError::Conflict { path, .. } => (
            RV_FILE_ERROR_CONFLICT,
            format!("another tool call of the run changed {path}, or is changing it"),
        ),
        VfsError::Io { kind, message, .. } => (RV_FILE_ERROR_IO, format!("{kind}: {message}")),
        // The path is quoted in the sentence and may hold `": "` itself,
        // so the rule is taken from the error, not cut out of the text.
        VfsError::InvalidPath { reason, .. } => (RV_FILE_ERROR_OTHER, reason.to_string()),
        VfsError::NotUtf8 { path, .. } => (
            RV_FILE_ERROR_OTHER,
            format!("{path} holds a name or contents that are not UTF-8"),
        ),
        other => (RV_FILE_ERROR_OTHER, without_label(&other.to_string())),
    }
}

/// Runs one operation of a tool call's access, and turns a panic inside
/// it into a failure. The operation runs under a C function the Plugin
/// called, which a panic must not unwind through, and the policy and the
/// backend behind an access are the Host's own code.
fn contained<T>(operation: impl FnOnce() -> Result<T, VfsError>) -> Result<T, VfsError> {
    catch_unwind(AssertUnwindSafe(operation)).unwrap_or_else(|_| {
        Err(VfsError::Backend {
            message: "the file operation panicked".to_owned(),
        })
    })
}

/// `text` without its leading label: the text after the first `": "`, or
/// the whole text when it has none.
fn without_label(text: &str) -> String {
    text.split_once(": ")
        .map_or(text, |(_, rest)| rest)
        .to_owned()
}

/// Says what `path` names; returns RV_OK with `stat` filled, RV_ERR_FILE
/// with `error` filled, or RV_ERR_MISUSE.
pub(crate) unsafe extern "C" fn file_stat(
    ctx: *mut c_void,
    path: RvStr,
    stat: *mut RvStat,
    error: *mut RvFileError,
) -> RvStatus {
    answer_file(ctx, |access, lent| {
        let stat = STAT.check(stat)?;
        let error = STAT_ERROR.check(error)?;
        // SAFETY: the contract makes `path` valid for its length during
        // this call.
        let path = unsafe { PATH.read(path) }?;
        Ok(match contained(|| access.stat(&path)) {
            Ok(found) => {
                *lent = Lent::default();
                // SAFETY: `stat` is not NULL and is aligned, and the
                // contract makes it a struct the Plugin owns.
                unsafe { stat.write(raw_stat(&found)) };
                RV_OK
            }
            // SAFETY: as above, for `error`.
            Err(failure) => unsafe { fail(lent, error, &failure) },
        })
    })
}

/// Lists the directory at `path`; returns RV_OK with `entries` filled,
/// RV_ERR_FILE with `error` filled, or RV_ERR_MISUSE.
pub(crate) unsafe extern "C" fn file_list(
    ctx: *mut c_void,
    path: RvStr,
    entries: *mut RvDirEntries,
    error: *mut RvFileError,
) -> RvStatus {
    answer_file(ctx, |access, lent| {
        let entries = ENTRIES.check(entries)?;
        let error = LIST_ERROR.check(error)?;
        // SAFETY: the contract makes `path` valid for its length during
        // this call.
        let path = unsafe { PATH.read(path) }?;
        Ok(match contained(|| access.list(&path)) {
            Ok(listed) => {
                let listed = lent.entries(listed);
                // SAFETY: `entries` is not NULL and is aligned, and the
                // contract makes it a struct the Plugin owns.
                unsafe { entries.write(listed) };
                RV_OK
            }
            // SAFETY: as above, for `error`.
            Err(failure) => unsafe { fail(lent, error, &failure) },
        })
    })
}

/// Reads up to `len` bytes of the file at `path` from the byte `offset`
/// on; returns RV_OK with `bytes` filled, RV_ERR_FILE with `error` filled,
/// or RV_ERR_MISUSE.
pub(crate) unsafe extern "C" fn file_read(
    ctx: *mut c_void,
    path: RvStr,
    offset: u64,
    len: u64,
    bytes: *mut RvBytes,
    error: *mut RvFileError,
) -> RvStatus {
    answer_file(ctx, |access, lent| {
        let bytes = BYTES.check(bytes)?;
        let error = READ_ERROR.check(error)?;
        // SAFETY: the contract makes `path` valid for its length during
        // this call.
        let path = unsafe { PATH.read(path) }?;
        // No read returns more than memory can hold, and a backend that
        // slices a whole file refuses a length it cannot address.
        let len = usize::try_from(len).unwrap_or(usize::MAX);
        // A real file cannot be positioned past this offset, and no file
        // is that long, so the read finds the end and returns no bytes.
        let offset = offset.min(i64::MAX.unsigned_abs());
        let wanted = u64::try_from(len).unwrap_or(u64::MAX);
        Ok(
            match contained(|| access.read_range(&path, offset, wanted)) {
                Ok(mut read) => {
                    // The contract promises at most `len` bytes, whatever
                    // the backend returned.
                    read.truncate(len);
                    let read = lent.bytes(read);
                    // SAFETY: `bytes` is not NULL and is aligned, and the
                    // contract makes it a struct the Plugin owns.
                    unsafe { bytes.write(read) };
                    RV_OK
                }
                // SAFETY: as above, for `error`.
                Err(failure) => unsafe { fail(lent, error, &failure) },
            },
        )
    })
}

/// Hands `failure` back through `error`, with its text kept in `lent`,
/// and returns RV_ERR_FILE.
///
/// # Safety
/// `error` must be valid for a write of an `RvFileError`.
unsafe fn fail(lent: &mut Lent, error: *mut RvFileError, failure: &VfsError) -> RvStatus {
    let raw = lent.error(failure);
    // SAFETY: the caller makes `error` valid for the write.
    unsafe { error.write(raw) };
    RV_ERR_FILE
}
