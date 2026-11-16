//! Sends messages to a prepared run and records success and cancellation.

use std::sync::Arc;

use runvil::harness::MemoryRecorder;
use runvil::{Error, ErrorCode, Host};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Error> {
    let history = Arc::new(MemoryRecorder::new());
    let prompts = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/prompts");
    let host = Host::new(prompts).with_recorder(history.clone());

    let run = host.run("messages.md").interactive().prepare()?;
    let control = run.control();
    assert!(control.send("Ada"));
    assert!(control.send("build a small Host"));
    let report = run.run().await?;
    assert_eq!(history.result(report.run_id), Some(report.result.clone()));
    let answer = report.result?;
    assert_eq!(answer, "Ada wants to build a small Host.");
    assert!(!history.records(report.run_id).is_empty());
    println!("run {}: {answer}", report.run_id);

    // Cancellation is an execution result inside the report; awaiting the
    // run itself still succeeds when parsing and recording succeed.
    let run = host.run("messages.md").interactive().prepare()?;
    run.control().cancel();
    let report = run.run().await?;
    assert_eq!(history.result(report.run_id), Some(report.result.clone()));
    assert!(matches!(&report.result, Err(error) if error.code == ErrorCode::Canceled));
    println!("run {}: canceled and recorded", report.run_id);
    Ok(())
}
