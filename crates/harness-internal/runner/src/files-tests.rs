//! The report's output: a completed run's declared file, and the failure
//! that replaces it when the run did not complete, the prompt declares no
//! output, the run left no file, or the store refuses the read.

use runvil::error::{Error, ErrorCode, FactKey};
use runvil::vfs::{Origin, VfsRef};
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
            error.facts.get(&FactKey::Reason),
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
        error.facts.get(&FactKey::Reason),
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
    assert_eq!(error.facts.get(&FactKey::Path), Some(&json!("report.md")));
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
    assert_eq!(error.facts.get(&FactKey::Path), Some(&json!("out")));
}
