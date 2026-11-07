//! The run surface a Host builds through the public API: a run over a
//! prompt string reports what it left at its declared output file, and
//! the output error a report carries.

use std::sync::Arc;

use harness::pack::{HostServices, PackRegistry, Packs};
use harness::record::MemoryRecorder;
use harness::vfs::{Origin, VfsRef};
use harness::{Harness, HostSnapshot, RunRequest};
use runvil::{ErrorCode, ErrorFact};

use crate::support::{Clock, Offline};

/// A prompt that reads its declared input and writes its declared output.
const SHOUTS: &str = concat!(
    "---\nname: shouts\ndescription: Shouts its input\nrunvil: 0\n",
    "input: { path: line.txt, description: The line to shout }\n",
    "output: { path: shout.txt, description: The shouted line }\n",
    "---\n\n# Shouts\n\n## Only\n\n```lua\n",
    "store.write('shout.txt', string.upper(store.read('line.txt')))\n",
    "```\n",
);

fn harness() -> Harness {
    Harness::new(
        Arc::new(MemoryRecorder::new()),
        Arc::new(Offline),
        Arc::new(Clock),
        Arc::new(Packs::new(PackRegistry::new(), HostServices::new())),
        HostServices::new(),
    )
}

fn request(source: &str, vfs: VfsRef) -> RunRequest {
    RunRequest {
        name: "shouts-1".to_owned(),
        source: source.to_owned(),
        file: "prompt".to_owned(),
        args: String::new(),
        input_text: Some("hello".to_owned()),
        vfs,
        host: HostSnapshot::default(),
    }
}

#[tokio::test]
async fn a_run_over_a_prompt_string_reports_its_output() {
    let vfs = VfsRef::default();
    let report = harness()
        .run(request(SHOUTS, vfs.clone()))
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
    let report = harness()
        .run(request(&failing, VfsRef::default()))
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
