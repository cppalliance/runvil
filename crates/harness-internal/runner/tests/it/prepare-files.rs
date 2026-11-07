//! The prompt's declared store files: the launch's input text is staged
//! at the frontmatter's `input:` path before the run, through the store
//! view wherever the handle mounts the store; a Host-seeded store
//! satisfies the declaration; text with no declared file and a declared
//! file with neither text nor a seeded copy each refuse the run and end it
//! with a `RequirementsUnmet` failure, and a store that refuses the write
//! ends it with the store's refusal; and the report carries what a
//! completed run left at the declared `output:` path.

use std::sync::Arc;

use harness_runner::files::read_output;
use harness_runner::recorder::MemoryRecorder;
use harness_runner::{RunReport, RunSetup};
use runvil::vfs::{MemoryBackend, Mode, ModePolicy, Origin, VfsRef};
use runvil::{Error, ErrorCode, ErrorFact};
use serde_json::json;

use super::{PLAIN, assert_no_records, bare, completed, recorder, report_of, run_of, setup_over};

/// A prompt declaring `paper.md` as its input and `report.md` as its
/// output, whose one section writes the output from the input.
const FILES: &str = "---\nname: files\ndescription: d\nrunvil: 0\n\
    input:\n  path: paper.md\n  description: The paper\n\
    output:\n  path: report.md\n  description: The report\n---\n\n\
    # Title\n\n## Only\n\n```lua\n\
    store.write('report.md', 'seen: ' .. store.read('paper.md'))\nreturn 'done'\n```\n";

/// Runs `source` over `vfs` with `input_text` and returns the recorder
/// beside the run's report.
async fn run_over(
    source: &str,
    vfs: &VfsRef,
    input_text: Option<&str>,
) -> (Arc<MemoryRecorder>, RunReport) {
    let recorder = recorder();
    let setup = RunSetup {
        vfs: vfs.clone(),
        input_text: input_text.map(str::to_owned),
        ..setup_over(&recorder)
    };
    let report = report_of(source, bare(), setup).await;
    (recorder, report)
}

/// Checks that `report` is of a run its input refused, whose record ended
/// with the refusal as its result and holds nothing before that end.
/// Returns the refusal.
fn refused(recorder: &MemoryRecorder, report: RunReport) -> Error {
    let error = report.result.expect_err("the input refuses the run");
    let run_id = run_of(&error);
    assert_eq!(run_id, report.run_id, "the refusal names its run");
    assert_eq!(
        recorder.result(run_id),
        Some(Err(error.clone())),
        "the recorder holds the refusal as the run's result"
    );
    assert_no_records(recorder, run_id);
    error
}

#[tokio::test]
async fn the_input_text_is_staged_at_the_declared_path_and_the_output_comes_back() {
    let vfs = VfsRef::default();
    let (_recorder, report) = run_over(FILES, &vfs, Some("# Paper")).await;
    assert_eq!(completed(report.result), "done");
    assert_eq!(report.output, Ok("seen: # Paper".to_owned()));
    assert_eq!(read_output(&vfs, "report.md").unwrap(), "seen: # Paper");
}

#[tokio::test]
async fn a_store_mounted_away_from_the_root_is_staged_by_its_logical_path() {
    let vfs = VfsRef::builder()
        .mount("/", MemoryBackend::new())
        .store("/store", MemoryBackend::new())
        .build();
    let (_recorder, report) = run_over(FILES, &vfs, Some("# Paper")).await;
    completed(report.result);
    let access = vfs.acquire(Origin::new("prepare files test")).unwrap();
    assert_eq!(access.read("/store/paper.md").unwrap(), b"# Paper");
    assert!(
        !access.exists("/paper.md").unwrap(),
        "the base never saw it"
    );
}

#[tokio::test]
async fn a_host_seeded_store_satisfies_the_declared_input() {
    let vfs = VfsRef::default();
    vfs.acquire_store(Origin::new("prepare files test"))
        .unwrap()
        .write("paper.md", b"# Seeded")
        .unwrap();
    let (_recorder, report) = run_over(FILES, &vfs, None).await;
    assert_eq!(report.output, Ok("seen: # Seeded".to_owned()));
}

#[tokio::test]
async fn input_text_for_a_prompt_that_declares_no_input_is_refused() {
    let (recorder, report) = run_over(PLAIN, &VfsRef::default(), Some("# Paper")).await;
    let source = refused(&recorder, report);
    assert_eq!(source.code, ErrorCode::RequirementsUnmet);
    assert_eq!(
        source.message,
        "the launch supplied input text, but the prompt declares no `input:` file"
    );
    assert_eq!(source.prose, ErrorCode::RequirementsUnmet.prose());
    assert_eq!(
        source.facts.get(&ErrorFact::Reason),
        Some(&json!("input_undeclared"))
    );
}

#[tokio::test]
async fn a_declared_input_neither_supplied_nor_in_the_store_is_refused() {
    let (recorder, report) = run_over(FILES, &VfsRef::default(), None).await;
    let source = refused(&recorder, report);
    assert_eq!(source.code, ErrorCode::RequirementsUnmet);
    assert_eq!(
        source.facts.get(&ErrorFact::Reason),
        Some(&json!("input_missing"))
    );
    assert_eq!(
        source.facts.get(&ErrorFact::Path),
        Some(&json!("paper.md")),
        "the refusal names the missing file"
    );
}

#[tokio::test]
async fn a_store_that_refuses_the_staging_write_refuses_the_run() {
    let vfs = VfsRef::builder()
        .store("/", MemoryBackend::new())
        .policy(ModePolicy::new(Mode::Ask))
        .build();
    let (recorder, report) = run_over(FILES, &vfs, Some("# Paper")).await;
    let source = refused(&recorder, report);
    assert_eq!(
        source.code,
        ErrorCode::ApprovalRequired,
        "the refusal is the store's"
    );
    assert!(
        source
            .message
            .starts_with("the input file `paper.md` could not be staged: "),
        "the message holds the store's refusal: {source:?}"
    );
    assert_eq!(source.facts.get(&ErrorFact::Path), Some(&json!("paper.md")));
}

#[tokio::test]
async fn a_prompt_without_an_output_declaration_reports_no_output() {
    let (_recorder, report) = run_over(PLAIN, &VfsRef::default(), None).await;
    completed(report.result);
    let error = report.output.expect_err("an undeclared output has no text");
    assert_eq!(error.code, ErrorCode::NoOutput);
    assert_eq!(
        error.facts.get(&ErrorFact::Reason),
        Some(&json!("undeclared"))
    );
}
