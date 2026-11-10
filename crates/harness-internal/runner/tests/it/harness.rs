//! The per-run Harness: `run` begins the run's record, resolves the run's
//! model through the setup's broker, prepares the parsed prompt, drives
//! the run as one future, and reads the declared output; the record holds
//! the run's own events and no parse event; the `ui()` snapshot serves the
//! setup's selection and first workspace root; a run that fails after it
//! starts is a recorded failed run, never an error of `run`; a Vfs effect
//! is answered inline as it is issued; and one run completes under a
//! thread-parking executor with no runtime behind it. How a stop and a
//! cancel reach the effects in flight sits in the `stop` child module, how
//! a stop reaches the tool calls waiting for a slot and how a task's early
//! end drops its stop-surviving call while the run goes on in the
//! `stop_cap` child module, how what a stop reaches depends on when the
//! loop sees it in the `stop_timing` child module, the messages a Host
//! sends the main chain in the `inbox` child module, and the runs that end
//! before they start and the one error `run` returns in the `errors` child
//! module.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, Thread, ThreadId};
use std::time::Duration;

use harness_runner::performers::{InferenceBroker, Timer};
use harness_runner::recorder::{MemoryRecorder, Record, RecordKind, RunId, RunRecorder};
use harness_runner::{Harness, Packs, RunControl, RunReport, RunSetup};
use runvil_engine::vfs::VfsRef;
use runvil_engine::{Error, ErrorCode};
use runvil_plugin::PackRegistry;
use serde_json::{Value, json};

use crate::scripted::{Kept, MODEL, ScriptedBroker};
use crate::support::{HookedBackend, PendingTimer, assert_not_completed, host_over, parsed};

#[path = "harness-errors.rs"]
mod errors;
#[path = "harness-inbox.rs"]
mod inbox;
#[path = "harness-stop.rs"]
mod stop;
#[path = "harness-stop-cap.rs"]
mod stop_cap;
#[path = "harness-stop-timing.rs"]
mod stop_timing;

/// How long a test waits for a run before it fails.
const PATIENCE: Duration = Duration::from_secs(10);

/// The setup of a run recorded through `recorder` and served by `broker`,
/// with [`MODEL`] selected, no arguments, no input text, and a fresh
/// memory store.
pub(crate) fn setup(
    recorder: Arc<dyn RunRecorder>,
    broker: impl InferenceBroker + 'static,
) -> RunSetup {
    RunSetup {
        selected_model: Some(MODEL.to_owned()),
        ..RunSetup::new(recorder, Arc::new(broker))
    }
}

/// A Harness running `source` with `setup` on a Host over `packs` that
/// sleeps through `timer`.
pub(crate) fn launch(
    packs: Packs,
    timer: impl Timer + 'static,
    source: &str,
    setup: RunSetup,
) -> Harness {
    Harness::new(host_over(packs, timer), parsed(source), setup)
}

/// A Host's Packs with none but the built-ins.
pub(crate) fn no_packs() -> Packs {
    Packs::new(PackRegistry::new())
}

/// A Harness running `source` over `recorder` and `broker` with no Packs
/// and a timer no test here reaches.
fn plain_harness(recorder: &Arc<MemoryRecorder>, broker: ScriptedBroker, source: &str) -> Harness {
    launch(
        no_packs(),
        PendingTimer::default(),
        source,
        setup(recorder.clone(), broker),
    )
}

/// Yields until `condition` holds, or fails naming `what`. Every
/// performer here is in memory, so the run beside the caller reaches each
/// state within a few polls.
pub(crate) async fn until(what: &str, condition: impl Fn() -> bool) {
    for _ in 0..10_000 {
        if condition() {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("{what}");
}

/// Runs `harness` beside `steer`, which is handed the run's control, and
/// returns the run's result.
pub(crate) async fn run_beside<F>(
    harness: Harness,
    steer: impl FnOnce(RunControl) -> F,
) -> Result<RunReport, Error>
where
    F: Future<Output = ()>,
{
    let control = harness.control();
    let (result, ()) = tokio::time::timeout(PATIENCE, async {
        tokio::join!(harness.run(), steer(control))
    })
    .await
    .expect("the run ends in time");
    result
}

/// The answer payloads of the effects whose record payload is keyed by
/// `kind` (`Chat`, `ToolCall`, `Timer`, `Vfs`) and whose body passes
/// `filter`, in effect order.
fn answers_where(records: &[Record], kind: &str, filter: impl Fn(&Value) -> bool) -> Vec<Value> {
    records
        .iter()
        .filter(|record| record.kind == RecordKind::Effect)
        .filter_map(|effect| {
            let body = effect.payload.get(kind)?;
            filter(body).then_some(effect.effect_id)
        })
        .map(|id| {
            records
                .iter()
                .find(|record| record.kind == RecordKind::Answer && record.effect_id == id)
                .expect("every effect is answered")
                .payload
                .clone()
        })
        .collect()
}

/// The answer payloads of every effect of `kind`, in effect order.
fn answers_to(records: &[Record], kind: &str) -> Vec<Value> {
    answers_where(records, kind, |_| true)
}

/// Whether a `ToolCall` effect body calls `tool`.
fn calls(tool: &'static str) -> impl Fn(&Value) -> bool {
    move |body| body["tool"] == tool
}

/// The run a test here drives: the first one a fresh recorder begins.
const FIRST_RUN: RunId = RunId::from_raw(1);

/// Whether `record` is an event of `kind`.
fn is_event(record: &Record, kind: &str) -> bool {
    record.kind == RecordKind::Event && record.payload["kind"] == kind
}

/// How many times `records` show the chain of the task `task` parked on
/// its empty inbox.
fn inbox_waits(records: &[Record], task: &str) -> usize {
    records
        .iter()
        .filter(|record| record.task_id == task && is_event(record, "inbox_waiting"))
        .count()
}

/// How many times `records` show the main chain parked on its empty
/// inbox.
fn main_waits(records: &[Record]) -> usize {
    inbox_waits(records, "0")
}

/// A prompt declaring `paper.md` as its input and `report.md` as its
/// output, whose one section writes the output from the input.
const FILES: &str = "---\nname: files\ndescription: d\nrunvil: 0\n\
    input:\n  path: paper.md\n  description: The paper\n\
    output:\n  path: report.md\n  description: The report\n---\n\n\
    # Title\n\n## Only\n\n```lua\n\
    store.write('report.md', 'seen: ' .. store.read('paper.md'))\nreturn 'done'\n```\n";

/// A prompt whose one section writes a store file and then raises.
const RAISES: &str = "---\nname: raises\ndescription: d\nrunvil: 0\n---\n\n\
    # Title\n\n## Only\n\n```lua\nstore.write('a.md', '1')\nerror('the section gives up')\n```\n";

/// A prompt that writes two store files and reads them back.
const WRITES_TWICE: &str = "---\nname: writes\ndescription: d\nrunvil: 0\n---\n\n\
    # Title\n\n## Only\n\n```lua\n\
    store.write('a.md', '1')\nstore.write('b.md', '2')\n\
    return store.read('a.md') .. store.read('b.md')\n```\n";

/// A prompt that writes a store file, infers once through its one role,
/// and returns the reply beside the file.
const INFERS_AND_STORES: &str = "---\nname: infers\ndescription: d\nrunvil: 0\n\
    ---\n\n\
    # Title\n\n## Only\n\n```lua\n\
    store.write('note.md', 'kept')\n\
    return models.infer('asked') .. '|' .. store.read('note.md')\n```\n";

/// A prompt that returns the `ui()` snapshot's two fields.
const READS_UI: &str = "---\nname: reads-ui\ndescription: d\nrunvil: 0\n---\n\n\
    # Title\n\n## Only\n\n```lua\n\
    return tostring(ui().selected_model) .. '|' .. tostring(ui().workspace_root)\n```\n";

#[tokio::test]
async fn a_staged_input_and_a_declared_output_round_trip_beside_the_recorded_result() {
    let recorder = Arc::new(MemoryRecorder::new());
    let harness = launch(
        no_packs(),
        PendingTimer::default(),
        FILES,
        RunSetup {
            input_text: Some("# Paper".to_owned()),
            ..setup(recorder.clone(), ScriptedBroker::replying())
        },
    );
    let report = harness.run().await.expect("the run is reported");

    assert_eq!(report.result, Ok("done".to_owned()));
    assert_eq!(report.output, Ok("seen: # Paper".to_owned()));
    let run_id = report.run_id;
    assert_eq!(
        recorder.result(run_id),
        Some(report.result.clone()),
        "the recorder ends the run with the reported result"
    );

    let records = recorder.records(run_id);
    let kinds: Vec<RecordKind> = records.iter().map(|record| record.kind).collect();
    assert_eq!(
        kinds.last(),
        Some(&RecordKind::Event),
        "the run's end is an event"
    );
    let effects: Vec<usize> = kinds
        .iter()
        .enumerate()
        .filter(|(_, kind)| **kind == RecordKind::Effect)
        .map(|(position, _)| position)
        .collect();
    assert_eq!(effects.len(), 2, "the read and the write");
    for position in effects {
        assert_eq!(
            kinds[position + 1],
            RecordKind::Answer,
            "a serial run's answer follows its effect"
        );
    }
}

#[tokio::test]
async fn a_runs_record_starts_with_its_own_events_and_holds_no_parse_event() {
    let recorder = Arc::new(MemoryRecorder::new());
    let report = plain_harness(&recorder, ScriptedBroker::replying(), WRITES_TWICE)
        .run()
        .await
        .expect("the run is reported");
    assert_eq!(report.result, Ok("12".to_owned()));

    let records = recorder.records(report.run_id);
    let first = records.first().expect("the run recorded its events");
    assert_eq!(first.kind, RecordKind::Event);
    assert_eq!(
        first.payload["kind"], "run_started",
        "the run's own start leads"
    );
    assert_eq!(
        (first.task_id.as_str(), first.task_seq),
        ("0", 0),
        "the root task counts from zero, with no parse events before it"
    );
    let parse_events: Vec<&Value> = records
        .iter()
        .filter(|record| record.kind == RecordKind::Event)
        .map(|record| &record.payload["kind"])
        .filter(|kind| kind.as_str().is_some_and(|kind| kind.starts_with("parse_")))
        .collect();
    assert!(
        parse_events.is_empty(),
        "the record holds no parse event: {parse_events:?}"
    );
}

#[tokio::test]
async fn the_ui_snapshot_serves_the_selected_model_and_the_first_workspace_root() {
    let recorder = Arc::new(MemoryRecorder::new());
    let first = PathBuf::from("/w/one");
    let harness = launch(
        no_packs(),
        PendingTimer::default(),
        READS_UI,
        RunSetup {
            workspace_roots: vec![first.clone(), PathBuf::from("/w/two")],
            ..setup(recorder.clone(), ScriptedBroker::replying())
        },
    );
    let report = harness.run().await.expect("the run is reported");
    assert_eq!(
        report.result,
        Ok(format!("{MODEL}|{}", first.display())),
        "the selection and the first root"
    );

    let harness = launch(
        no_packs(),
        PendingTimer::default(),
        READS_UI,
        RunSetup::new(recorder.clone(), Arc::new(ScriptedBroker::replying())),
    );
    let report = harness.run().await.expect("the run is reported");
    assert_eq!(
        report.result,
        Ok("nil|nil".to_owned()),
        "no selection and no roots read as nil"
    );
}

#[tokio::test]
async fn a_run_that_fails_after_it_starts_is_a_recorded_failed_run_not_an_error() {
    let recorder = Arc::new(MemoryRecorder::new());
    let report = plain_harness(&recorder, ScriptedBroker::replying(), RAISES)
        .run()
        .await
        .expect("a run's own failure is its result, not an error");

    let error = report.result.as_ref().expect_err("the section raised");
    assert_eq!(
        error.code,
        ErrorCode::LuaRuntime,
        "the run's own code, not a refusal"
    );
    assert!(
        error.located_message().contains("the section gives up"),
        "{error:?}"
    );
    let run_id = report.run_id;
    assert_eq!(recorder.result(run_id), Some(report.result.clone()));
    assert_not_completed(&report.output);
    assert_eq!(
        answers_to(&recorder.records(run_id), "Vfs"),
        [json!({ "Vfs": { "Ok": "Unit" } })],
        "the run starts and performs its write before it raises"
    );
}

#[tokio::test]
async fn a_vfs_effect_is_answered_inline_on_the_thread_that_polls_the_run() {
    let ran_on: Kept<ThreadId> = Kept::default();
    let seen = Arc::clone(&ran_on);
    let vfs = VfsRef::builder()
        .store(
            "/",
            HookedBackend::new(move || seen.lock().unwrap().push(thread::current().id())),
        )
        .build();
    let recorder = Arc::new(MemoryRecorder::new());
    let harness = launch(
        no_packs(),
        PendingTimer::default(),
        WRITES_TWICE,
        RunSetup {
            vfs,
            ..setup(recorder.clone(), ScriptedBroker::replying())
        },
    );
    let report = harness.run().await.expect("the run is reported");

    assert_eq!(report.result, Ok("12".to_owned()));
    assert_eq!(
        *ran_on.lock().unwrap(),
        vec![thread::current().id(); 2],
        "both writes ran inline, on the thread that polls the run"
    );
    let records = recorder.records(report.run_id);
    for (position, effect) in records.iter().enumerate() {
        if effect.kind == RecordKind::Effect {
            let answer = &records[position + 1];
            assert_eq!(answer.kind, RecordKind::Answer, "nothing comes between");
            assert_eq!(answer.effect_id, effect.effect_id, "the answer is its own");
        }
    }
}

/// Wakes the thread parked in [`block_on`].
struct Unpark(Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

/// A std-only executor: polls `future` on this thread and parks between
/// polls until its waker unparks the thread.
fn block_on<F: Future + Unpin>(mut future: F) -> F::Output {
    let waker = Waker::from(Arc::new(Unpark(thread::current())));
    let mut cx = Context::from_waker(&waker);
    loop {
        if let Poll::Ready(output) = Pin::new(&mut future).poll(&mut cx) {
            return output;
        }
        thread::park();
    }
}

/// Compiles only for a `Send` value.
fn assert_send<T: Send>(_: &T) {}

#[test]
fn one_run_completes_on_the_tests_own_thread_under_a_thread_parking_block_on() {
    let recorder = Arc::new(MemoryRecorder::new());
    let broker = ScriptedBroker::replying();
    let rounds = broker.rounds();
    let run = plain_harness(&recorder, broker, INFERS_AND_STORES).run();
    assert_send(&run);

    let report = block_on(Box::pin(run)).expect("the run is reported with no runtime");
    assert_eq!(report.result, Ok("re: asked|kept".to_owned()));
    assert_eq!(rounds.lock().unwrap().len(), 1, "the one infer");
}
