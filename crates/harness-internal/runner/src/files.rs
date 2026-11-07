//! The Harness's half of a prompt's declared store files: the launch's input
//! text staged at the frontmatter's `input:` path before a run, and the
//! `output:` path read after it.
//!
//! Both go through the handle's store view ([`VfsRef::acquire_store`]),
//! so the store's strict path rules apply to the declared paths, which
//! the parser takes as written, and the handle's policy and op sink see
//! each operation like any other. Both are synchronous, like the VFS, and
//! the Harness runs them inline in the run's future.

use runvil::vfs::{Origin, VfsError, VfsOp, VfsOutcome, VfsRef, make_store_error, perform_vfs_op};
use runvil::{Error, ErrorCode, ErrorFact};

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
            return Err(make_reasoned_error(
                ErrorCode::RequirementsUnmet,
                "input_undeclared",
                "the launch supplied input text, but the prompt declares no `input:` file",
            ));
        }
        (Some(path), _) => path,
    };
    let store = |source: VfsError| {
        let mut error = make_store_error(&source);
        error.message = format!("the input file `{path}` could not be staged: {source}");
        error.facts.insert(ErrorFact::Path, path.into());
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
                let mut error = make_reasoned_error(
                    ErrorCode::RequirementsUnmet,
                    "input_missing",
                    format!(
                        "the prompt declares the input file `{path}`, but the launch supplied no \
                         input text and the store has no such file"
                    ),
                );
                error.facts.insert(ErrorFact::Path, path.into());
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
/// [`make_not_completed_error`], and a prompt with no `output:` file with `NoOutput`
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
        return Err(make_not_completed_error());
    }
    let path = path.ok_or_else(|| {
        make_reasoned_error(
            ErrorCode::NoOutput,
            "undeclared",
            "the prompt declares no `output:` file",
        )
    })?;
    read_output(vfs, path).map_err(|error| {
        let mut converted = make_store_error(&error);
        converted.message = if matches!(error, VfsError::NotFound { .. }) {
            format!("the run completed without writing its declared output file `{path}`")
        } else {
            format!("the output file `{path}` could not be read: {error}")
        };
        converted.facts.insert(ErrorFact::Path, path.into());
        converted
    })
}

/// The output failure of a run that failed or was canceled, possibly
/// before it began: `NoOutput` with the `Reason` fact `make_not_completed_error`.
pub(crate) fn make_not_completed_error() -> Error {
    make_reasoned_error(
        ErrorCode::NoOutput,
        "not_completed",
        "the run did not complete",
    )
}

/// A failure under `code` with `message`, the `Reason` fact `reason`, and
/// the code's default prose.
fn make_reasoned_error(code: ErrorCode, reason: &str, message: impl Into<String>) -> Error {
    let mut error = Error::new(code, message, code.prose());
    error.facts.insert(ErrorFact::Reason, reason.into());
    error
}

#[cfg(test)]
mod tests {
    //! The report's output: a completed run's declared file, and the failure
    //! that replaces it when the run did not complete, the prompt declares no
    //! output, the run left no file, or the store refuses the read.

    use runvil::vfs::{Origin, VfsRef};
    use runvil::{Error, ErrorCode, ErrorFact};
    use serde_json::json;

    use super::report_output;

    /// The result of a completed run.
    const COMPLETED: Result<String, Error> = Ok(String::new());

    /// A default handle whose store holds `contents` at `path`.
    fn seeded(path: &str, contents: &str) -> VfsRef {
        let vfs = VfsRef::default();
        vfs.acquire_store(Origin::new("files test"))
            .expect("the default handle declares a store")
            .write(path, contents.as_bytes())
            .expect("the seed is written");
        vfs
    }

    #[test]
    fn a_completed_run_reports_its_declared_output_file() {
        let vfs = seeded("report.md", "done");
        assert_eq!(
            report_output(&vfs, Some("report.md"), &COMPLETED),
            Ok("done".to_owned())
        );
    }

    #[test]
    fn a_run_that_did_not_complete_reports_no_output() {
        let failed = Error::new(ErrorCode::Internal, "", ErrorCode::Internal.prose());
        for result in [Err(Error::canceled()), Err(failed)] {
            let error = report_output(&VfsRef::default(), Some("report.md"), &result)
                .expect_err("a run that did not complete has no output");
            assert_eq!(error.code, ErrorCode::NoOutput);
            assert_eq!(error.message, "the run did not complete");
            assert_eq!(
                error.facts.get(&ErrorFact::Reason),
                Some(&json!("not_completed"))
            );
        }
    }

    #[test]
    fn a_prompt_without_an_output_declaration_reports_no_output() {
        let error = report_output(&VfsRef::default(), None, &COMPLETED)
            .expect_err("an undeclared output has no text");
        assert_eq!(error.code, ErrorCode::NoOutput);
        assert_eq!(error.message, "the prompt declares no `output:` file");
        assert_eq!(error.prose, ErrorCode::NoOutput.prose());
        assert_eq!(
            error.facts.get(&ErrorFact::Reason),
            Some(&json!("undeclared"))
        );
    }

    #[test]
    fn a_completed_run_without_its_output_file_reports_not_found() {
        let error = report_output(&VfsRef::default(), Some("report.md"), &COMPLETED)
            .expect_err("the run wrote no output file");
        assert_eq!(error.code, ErrorCode::NotFound);
        assert_eq!(
            error.message,
            "the run completed without writing its declared output file `report.md`"
        );
        assert_eq!(error.facts.get(&ErrorFact::Path), Some(&json!("report.md")));
    }

    #[test]
    fn a_refused_read_reports_the_stores_failure_with_its_cause_in_the_message() {
        let vfs = seeded("out/report.md", "done");
        let error = report_output(&vfs, Some("out"), &COMPLETED)
            .expect_err("a directory is not an output file");
        assert_eq!(error.code, ErrorCode::IsADirectory);
        assert!(
            error
                .message
                .starts_with("the output file `out` could not be read: "),
            "{error:?}"
        );
        assert_eq!(error.prose, ErrorCode::IsADirectory.prose());
        assert_eq!(error.facts.get(&ErrorFact::Path), Some(&json!("out")));
    }
}
