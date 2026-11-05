//! The Harness's half of a prompt's declared store files: the launch's input
//! text staged at the frontmatter's `input:` path before a run, and the
//! `output:` path read after it.
//!
//! Both go through the handle's store view ([`VfsRef::acquire_store`]),
//! so the store's strict path rules apply to the declared paths, which
//! the parser takes as written, and the handle's policy and op sink see
//! each operation like any other. Both are synchronous, like the VFS, and
//! the Harness runs them inline in the run's future.

use runvil::error::{Error, ErrorCode, FactKey};
use runvil::vfs::{Origin, VfsError, VfsOp, VfsOutcome, VfsRef, perform_vfs_op, store_error};

#[cfg(test)]
#[path = "files-tests.rs"]
mod tests;

/// Puts the prompt's declared input in place in `vfs`'s store: writes
/// `text` at `declared`, overwriting any file there, or, when the launch
/// supplied no text, checks that the store already holds the file.
///
/// # Errors
/// Returns `RequirementsUnmet` with the `Reason` fact `input_undeclared`
/// for text the prompt declares no file for, `RequirementsUnmet` with the
/// `Reason` fact `input_missing` for a declared file that is neither
/// supplied nor present, and the store failure's code when the store
/// refuses the write or the check. The last two carry the declared path
/// as the `Path` fact. The prose is the code's default.
pub fn stage_input(
    vfs: &VfsRef,
    declared: Option<&str>,
    text: Option<String>,
) -> Result<(), Error> {
    let path = match (declared, &text) {
        (None, None) => return Ok(()),
        (None, Some(_)) => {
            return Err(reasoned(
                ErrorCode::RequirementsUnmet,
                "input_undeclared",
                "the launch supplied input text, but the prompt declares no `input:` file",
            ));
        }
        (Some(path), _) => path,
    };
    let store = |source: VfsError| {
        let mut error = store_error(&source);
        error.message = format!("the input file `{path}` could not be staged: {source}");
        error.facts.insert(FactKey::Path, path.into());
        error
    };
    let view = vfs
        .acquire_store(Origin::new(format!("input: {path}")))
        .map_err(store)?;
    let path = path.to_owned();
    match text {
        Some(contents) => perform_vfs_op(&view, VfsOp::Write { path, contents })
            .map(drop)
            .map_err(store),
        None => match perform_vfs_op(&view, VfsOp::Exists { path: path.clone() }) {
            Ok(VfsOutcome::Bool(true)) => Ok(()),
            Ok(_) => {
                let mut error = reasoned(
                    ErrorCode::RequirementsUnmet,
                    "input_missing",
                    format!(
                        "the prompt declares the input file `{path}`, but the launch supplied no \
                         input text and the store has no such file"
                    ),
                );
                error.facts.insert(FactKey::Path, path.into());
                Err(error)
            }
            Err(source) => Err(store(source)),
        },
    }
}

/// Reads the prompt's declared output file at `path` from `vfs`'s store.
///
/// # Errors
/// Returns the store's failure, [`VfsError::NotFound`] when the run never
/// wrote the file.
pub fn read_output(vfs: &VfsRef, path: &str) -> Result<String, VfsError> {
    let view = vfs.acquire_store(Origin::new(format!("output: {path}")))?;
    let read = VfsOp::Read {
        path: path.to_owned(),
        start: None,
        end: None,
    };
    match perform_vfs_op(&view, read)? {
        VfsOutcome::Text(text) => Ok(text),
        other => Err(VfsError::Backend {
            message: format!("a whole-file store read answered {other:?} instead of text"),
        }),
    }
}

/// The output a run's report carries: what a completed run left at its
/// declared `output:` path in `vfs`'s store, or why the report omits it.
///
/// A run whose `result` is an error did not complete and fails with
/// [`not_completed`], and a prompt with no `output:` file with `NoOutput`
/// and the `Reason` fact `undeclared`. A completed run that left no file
/// fails with `NotFound`, and a refused read with the store failure's
/// code. Both carry the declared path as the `Path` fact. The prose is the
/// code's default.
pub(crate) fn report_output(
    vfs: &VfsRef,
    path: Option<&str>,
    result: &Result<String, Error>,
) -> Result<String, Error> {
    if result.is_err() {
        return Err(not_completed());
    }
    let path = path.ok_or_else(|| {
        reasoned(
            ErrorCode::NoOutput,
            "undeclared",
            "the prompt declares no `output:` file",
        )
    })?;
    read_output(vfs, path).map_err(|error| {
        let mut converted = store_error(&error);
        converted.message = if matches!(error, VfsError::NotFound { .. }) {
            format!("the run completed without writing its declared output file `{path}`")
        } else {
            format!("the output file `{path}` could not be read: {error}")
        };
        converted.facts.insert(FactKey::Path, path.into());
        converted
    })
}

/// The output failure of a run that failed or was canceled, possibly
/// before it began: `NoOutput` with the `Reason` fact `not_completed`.
pub(crate) fn not_completed() -> Error {
    reasoned(
        ErrorCode::NoOutput,
        "not_completed",
        "the run did not complete",
    )
}

/// A failure under `code` with `message`, the `Reason` fact `reason`, and
/// the code's default prose.
fn reasoned(code: ErrorCode, reason: &str, message: impl Into<String>) -> Error {
    let mut error = Error::new(code, message, code.prose());
    error.facts.insert(FactKey::Reason, reason.into());
    error
}
