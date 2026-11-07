//! The run surface a Host builds through the public API: a run over a
//! prompt string reports what it left at its declared output file, and
//! the output error a report carries.

use std::sync::Arc;

use harness::record::MemoryRecorder;
use harness::vfs::{Origin, VfsRef};
use harness::{Harness, RunSetup};
use runvil::{ErrorCode, ErrorFact};

use crate::support::{Offline, launch, no_packs};

/// A prompt that reads its declared input and writes its declared output.
const SHOUTS: &str = concat!(
    "---\nname: shouts\ndescription: Shouts its input\nrunvil: 0\n",
    "input: { path: line.txt, description: The line to shout }\n",
    "output: { path: shout.txt, description: The shouted line }\n",
    "---\n\n# Shouts\n\n## Only\n\n```lua\n",
    "store.write('shout.txt', string.upper(store.read('line.txt')))\n",
    "```\n",
);

/// A Harness running `source` over `vfs` with `hello` as its input.
fn harness(source: &str, vfs: VfsRef) -> Harness {
    let setup = RunSetup {
        vfs,
        input_text: Some("hello".to_owned()),
        ..RunSetup::new(Arc::new(MemoryRecorder::new()), Arc::new(Offline))
    };
    launch(no_packs(), source, setup)
}

#[tokio::test]
async fn a_run_over_a_prompt_string_reports_its_output() {
    let vfs = VfsRef::default();
    let report = harness(SHOUTS, vfs.clone())
        .run()
        .await
        .expect("the run is reported");
    assert!(
        report.result.is_ok(),
        "the run completes: {:?}",
        report.result
    );
    assert_eq!(
        report.output,
        Ok("HELLO".to_owned()),
        "the report carries what the run left at its declared output file"
    );
    assert_eq!(
        vfs.acquire_store(Origin::new("suite check"))
            .expect("the default handle declares a store")
            .read_string("shout.txt")
            .expect("the output stays in the Host's store"),
        "HELLO"
    );
}

#[tokio::test]
async fn a_run_that_does_not_complete_reports_no_output() {
    let failing = SHOUTS.replace("store.write", "error('no shout') store.write");
    let report = harness(&failing, VfsRef::default())
        .run()
        .await
        .expect("the run is reported");
    assert_eq!(
        report.result.as_ref().map_err(|error| error.code),
        Err(ErrorCode::LuaRuntime),
        "the raised error ends the run"
    );
    let error = report.output.expect_err("a failed run reports no output");
    assert_eq!(error.code, ErrorCode::NoOutput);
    assert_eq!(
        error.facts.get(&ErrorFact::Reason),
        Some(&serde_json::json!("not_completed"))
    );
}
