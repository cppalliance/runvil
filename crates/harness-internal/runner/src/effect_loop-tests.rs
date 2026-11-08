//! The loop's one end that no Harness run reaches: a run left with
//! nothing to await, the fault a lost effect would leave, ends its send
//! queue as it ends, so a message the loop accepted and never delivered
//! comes back unread and every later send is refused.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use runvil::cancel::CancelHandle;
use runvil::effect::{Round, ToolCallOrigin};
use runvil::model::{
    Completion, CompletionOptions, Message, ModelBinding, ModelCatalog, ToolSchema,
};
use runvil::timestamp::Timestamp;
use runvil::vfs::Access;
use runvil::{Error, ErrorCode, Prompt, Run, RunParams, RunSeed, Step};
use runvil::{ToolCatalog, ToolId, ToolInfo, ToolOutput};
use serde_json::{Value, json};

use super::{STALLED, drive};
use crate::harness::RunControl;
use crate::performers::{BoxFuture, InferenceBroker, Timer, ToolPerformer};
use crate::recorder::{MemoryRecorder, Record, RecorderFuture, RunId, RunMeta, RunRecorder};
use crate::setup::RunSetup;

/// The fixed seed the run and its recorded meta take.
const SEED: RunSeed = RunSeed::from_bytes([7; 32]);

/// A run of one section that calls the tool `tests/runner/wait`, stepped
/// once with its tool call kept from the loop: the run waits on an effect
/// the loop never sees.
fn lost_run() -> Run {
    let source = "---\nname: lost\ndescription: d\nrunvil: 0\n---\n\n# Lost\n\n## Only\n\n\
        ```lua\nreturn tools.call('tests/runner/wait')\n```\n";
    let (prompt, _parse_events) = Prompt::parse(&|_: &str| Ok(source.to_owned()), "prompt");
    let wait = ToolInfo::new(
        ToolId::parse("tests/runner/wait").expect("the wait tool id is valid"),
        "Wait until the test answers.",
        json!({ "type": "object", "properties": {} }),
    );
    let params = RunParams::new(SEED, Timestamp::UNIX_EPOCH)
        .with_tools(ToolCatalog::new([wait]).expect("the one-tool catalog is valid"));
    let mut run = Run::new(Arc::new(prompt.expect("the prompt parses")), "", params);
    let Step::Pending { effects, .. } = run.step() else {
        panic!("the run parks on its tool call");
    };
    assert_eq!(effects.len(), 1, "the one tool call the loop never sees");
    run
}

/// Records into `inner`, and sends `text` through `control` as it takes
/// its first record: a Host's message arriving while the loop records.
struct SendsOnFirstAppend {
    inner: MemoryRecorder,
    control: RunControl,
    text: &'static str,
    sent: AtomicBool,
}

impl RunRecorder for SendsOnFirstAppend {
    fn begin_run(&self, meta: RunMeta) -> RecorderFuture<'_, RunId> {
        self.inner.begin_run(meta)
    }

    fn append(&self, run: RunId, record: Record) -> RecorderFuture<'_, ()> {
        if !self.sent.swap(true, Ordering::SeqCst) {
            assert!(
                self.control.send(self.text.to_owned()),
                "the live run accepts {}",
                self.text
            );
        }
        self.inner.append(run, record)
    }

    fn end_run(&self, run: RunId, result: Result<String, Error>) -> RecorderFuture<'_, ()> {
        self.inner.end_run(run, result)
    }
}

/// A performer for every kind the loop never sees the lost run issue;
/// reaching one is the test's failure.
struct Unused;

impl InferenceBroker for Unused {
    fn models(&self) -> BoxFuture<Result<ModelCatalog, Error>> {
        unreachable!("the effect loop never lists models")
    }

    fn chat(
        &self,
        _binding: ModelBinding,
        _messages: Vec<Message>,
        _tools: Vec<ToolSchema>,
        _options: CompletionOptions,
        _round: Round,
    ) -> BoxFuture<Result<Box<Completion>, Error>> {
        unreachable!("the lost run issues no Chat effect")
    }
}

impl ToolPerformer for Unused {
    fn call(
        &self,
        _tool: ToolId,
        _alias: String,
        _access: Arc<Access>,
        _origin: ToolCallOrigin,
        _args: Value,
    ) -> BoxFuture<Result<ToolOutput, Error>> {
        unreachable!("the loop never sees the lost run's tool call")
    }
}

impl Timer for Unused {
    fn sleep(&self, _duration: Duration) -> BoxFuture<()> {
        unreachable!("the lost run issues no Timer effect")
    }
}

#[tokio::test]
async fn a_run_left_with_nothing_to_await_reports_the_message_it_never_delivered_and_refuses_later_ones()
 {
    let control = RunControl::new(CancelHandle::new());
    let recorder = Arc::new(SendsOnFirstAppend {
        inner: MemoryRecorder::new(),
        control: control.clone(),
        text: "second",
        sent: AtomicBool::new(false),
    });
    let meta = RunMeta {
        seed: SEED,
        flags: 0,
        started_at: 0,
    };
    let run_id = recorder.begin_run(meta).await.unwrap();
    assert!(control.send("first".to_owned()));

    // The loop delivers `first` before its first step, and `second`
    // arrives as the loop records that delivery. The step then finds
    // nothing to await, so the loop never delivers `second`.
    let (result, unread) = drive(
        lost_run(),
        &RunSetup::new(recorder.clone(), Arc::new(Unused)),
        Arc::new(Unused),
        Arc::new(Unused),
        run_id,
        &control,
    )
    .await;

    let error = result.as_ref().expect_err("a stall ends the run");
    assert_eq!(error.code, ErrorCode::Internal);
    assert_eq!(error.located_message(), STALLED);
    assert_eq!(recorder.inner.result(run_id), Some(result.clone()));
    assert_eq!(
        unread,
        ["second"],
        "the message the loop accepted and never delivered"
    );
    assert!(
        !control.send("late".to_owned()),
        "the ended run refuses the message"
    );
}
