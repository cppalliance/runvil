//! Named runs through the public API: defaults, ownership, results, and controls.

use std::future::{Future, IntoFuture};
use std::num::NonZeroU32;
use std::pin::pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use runvil::effect::Round;
use runvil::harness::{InferenceBroker, MemoryRecorder, Timer};
use runvil::model::{
    Completion, CompletionOptions, CompletionResult, Message, ModelBinding, ModelCatalog, ModelId,
    ModelInfo, ThinkingMode, ToolSchema,
};
use runvil::plugin::PackRegistry;
use runvil::{BoxFuture, Error, ErrorCode, ErrorFact, Harness, Host, RunSetup};

fn source(frontmatter: &str, lua: &str) -> String {
    format!(
        "---\nname: test\ndescription: A named run\nrunvil: 0\n{frontmatter}---\n\n\
         # Test\n\n## Only\n\n```lua\n{lua}\n```\n"
    )
}

fn app(frontmatter: &str, lua: &str) -> Host {
    let source = source(frontmatter, lua);
    Host::new(".").with_prompt_finder(move |_: &str| Ok(source.clone()))
}

struct Echo {
    catalog: ModelCatalog,
    calls: Arc<AtomicUsize>,
}

impl Echo {
    fn new(calls: Arc<AtomicUsize>) -> Result<Self, Error> {
        let catalog = ModelCatalog::new([ModelInfo::new(
            ModelId::gateway("my-model")?,
            "Echoes the last message",
            NonZeroU32::MIN.saturating_add(8191),
            ThinkingMode::Never,
        )])?;
        Ok(Self { catalog, calls })
    }
}

impl InferenceBroker for Echo {
    fn models(&self) -> BoxFuture<'static, Result<ModelCatalog, Error>> {
        let catalog = self.catalog.clone();
        Box::pin(async move { Ok(catalog) })
    }

    fn chat(
        &self,
        _binding: ModelBinding,
        messages: Vec<Message>,
        _tools: Vec<ToolSchema>,
        _options: CompletionOptions,
        _round: Round,
    ) -> BoxFuture<'static, Result<Box<Completion>, Error>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let asked = messages.last().map_or("", Message::content).to_owned();
        Box::pin(async move {
            Completion::from_result(CompletionResult::Text(asked), "my-model").map(Box::new)
        })
    }
}

#[tokio::test]
async fn the_front_door_accepts_owned_services_and_fluent_inputs() -> Result<(), Error> {
    let calls = Arc::new(AtomicUsize::new(0));
    let broker = Echo::new(calls.clone())?;
    let prompt = source(
        "input: { path: input.txt, description: The question }\n",
        "return models.infer(args .. ': ' .. store.read('input.txt'))",
    );
    let host = Host::new("./prompts")
        .with_broker(broker)
        .with_packs(PackRegistry::new())
        .with_prompt_finder(move |_: &str| Ok(prompt.clone()));

    let report = host
        .run("research.md")
        .args("Review")
        .input("Explain this code")
        .model("my-model")
        .await?;

    assert_eq!(report.result?, "Review: Explain this code");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn requests_share_services_and_get_separate_filesystems() -> Result<(), Error> {
    let host = app(
        "",
        "local old = store.exists('count.txt'); store.write('count.txt', args); \
         return old and 'shared' or args",
    );
    let first = host.run("first.md").args("first").text();
    let second = host.run("second.md").args("second").text();
    let (first, second) = tokio::join!(first, second);
    assert_eq!(first?, "first");
    assert_eq!(second?, "second");
    Ok(())
}

#[tokio::test]
async fn a_shared_broker_and_recorder_work_across_requests() -> Result<(), Error> {
    let broker = Arc::new(Echo::new(Arc::new(AtomicUsize::new(0)))?);
    let recorder = Arc::new(MemoryRecorder::new());
    let host = app("", "return models.infer(args)")
        .with_broker(broker)
        .with_recorder(recorder.clone());
    let first = host.run("first.md").args("one").await?;
    let second = host.run("second.md").args("two").await?;
    assert_ne!(first.run_id, second.run_id);
    assert_eq!(recorder.result(first.run_id), Some(first.result));
    assert_eq!(recorder.result(second.run_id), Some(second.result));
    Ok(())
}

#[tokio::test]
async fn explicit_setup_accepts_owned_services() -> Result<(), Error> {
    let host = app("", "return models.infer(args)");
    let prompt = host.parse("explicit.md")?;
    let setup = RunSetup {
        args: "owned services".to_owned(),
        ..RunSetup::new(
            MemoryRecorder::new(),
            Echo::new(Arc::new(AtomicUsize::new(0)))?,
        )
    };
    let report = Harness::new(host, prompt, setup).run().await?;
    assert_eq!(report.result?, "owned services");
    Ok(())
}

#[tokio::test]
async fn the_folder_quickstart_runs_with_default_services() -> Result<(), Error> {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/prompts");
    let host = Host::new(folder);
    assert_eq!(
        host.run("greet.md").input("Ada").output().await?,
        "Hello, Ada."
    );
    Ok(())
}

#[tokio::test]
async fn a_missing_prompt_names_the_requested_file_and_folder_before_recording() {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/prompts");
    let recorder = Arc::new(MemoryRecorder::new());
    let host = Host::new(&folder).with_recorder(recorder.clone());
    let error = host.run("missing.md").text().await.unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
    assert_eq!(
        error.facts.get(&ErrorFact::Name),
        Some(&"missing.md".into())
    );
    assert_eq!(
        error.facts.get(&ErrorFact::Path),
        Some(&folder.join("missing.md").to_string_lossy().as_ref().into())
    );
    assert!(
        error
            .message
            .starts_with("the prompt `missing.md` could not be read from")
    );
    assert_eq!(recorder.meta(runvil::harness::RunId::from_raw(1)), None);
}

#[tokio::test]
async fn an_invalid_model_selection_is_a_recorded_failure() {
    let recorder = Arc::new(MemoryRecorder::new());
    let host = app("", "return 'must not run'").with_recorder(recorder.clone());
    let error = host.run("invalid.md").model("").text().await.unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidValue);
    let run = runvil::harness::RunId::from_raw(1);
    assert_eq!(error.facts.get(&ErrorFact::Run), Some(&run.get().into()));
    assert!(recorder.records(run).is_empty());
    assert_eq!(recorder.result(run).unwrap().unwrap_err().code, error.code);
}

#[tokio::test]
async fn dropping_an_unstarted_request_does_not_read_or_record() {
    let reads = Arc::new(AtomicUsize::new(0));
    let recorder = Arc::new(MemoryRecorder::new());
    let host = Host::new(".")
        .with_recorder(recorder.clone())
        .with_prompt_finder({
            let reads = reads.clone();
            move |_: &str| {
                reads.fetch_add(1, Ordering::SeqCst);
                Ok(source("", "return 'done'"))
            }
        });
    drop(host.run("unused.md").args("unused"));
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(recorder.meta(runvil::harness::RunId::from_raw(1)), None);
}

#[tokio::test]
async fn parse_failure_precedes_recording() {
    let recorder = Arc::new(MemoryRecorder::new());
    let host = Host::new(".")
        .with_recorder(recorder.clone())
        .with_prompt_finder(|_: &str| Ok("not a prompt".to_owned()));
    assert!(host.run("invalid.md").await.is_err());
    assert_eq!(recorder.meta(runvil::harness::RunId::from_raw(1)), None);
}

#[tokio::test]
async fn text_propagates_execution_errors_with_the_recorded_run_id() {
    let recorder = Arc::new(MemoryRecorder::new());
    let host = app("", "error('failed on purpose')").with_recorder(recorder.clone());
    let error = host.run("bad.md").text().await.unwrap_err();
    assert_eq!(error.code, ErrorCode::LuaRuntime);
    let run = runvil::harness::RunId::from_raw(1);
    assert_eq!(error.facts.get(&ErrorFact::Run), Some(&run.get().into()));
    assert!(recorder.result(run).unwrap().is_err());
}

#[tokio::test]
async fn a_full_report_retains_execution_errors() -> Result<(), Error> {
    let host = app("", "error('failed on purpose')");
    let report = host.run("bad.md").await?;
    assert_eq!(report.result.unwrap_err().code, ErrorCode::LuaRuntime);
    Ok(())
}

#[tokio::test]
async fn output_reads_the_declared_file_and_prioritizes_execution_failure() -> Result<(), Error> {
    let host = app(
        "output: { path: reply.txt, description: The reply }\n",
        "store.write('reply.txt', args); return 'final text'",
    );
    assert_eq!(
        host.run("output.md").args("file text").output().await?,
        "file text"
    );
    let failed = app("", "error('failed on purpose')");
    assert_eq!(
        failed.run("bad.md").output().await.unwrap_err().code,
        ErrorCode::LuaRuntime
    );
    let no_output = app("", "return 'done'");
    assert_eq!(
        no_output.run("none.md").output().await.unwrap_err().code,
        ErrorCode::NoOutput
    );
    Ok(())
}

#[tokio::test]
async fn output_read_failures_carry_the_completed_runs_id() {
    let recorder = Arc::new(MemoryRecorder::new());
    let host = app(
        "output: { path: missing.txt, description: The missing output }\n",
        "return 'completed without writing the output'",
    )
    .with_recorder(recorder.clone());
    let error = host.run("missing-output.md").output().await.unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
    assert_eq!(
        error.facts.get(&ErrorFact::Path),
        Some(&"missing.txt".into())
    );
    let run = runvil::harness::RunId::from_raw(1);
    assert_eq!(error.facts.get(&ErrorFact::Run), Some(&run.get().into()));
    assert!(recorder.result(run).unwrap().is_ok());
}

#[tokio::test]
async fn a_built_request_can_be_canceled_before_execution() -> Result<(), Error> {
    let host = app("", "return 'never executed'");
    let run = host.run("cancel.md").prepare()?;
    let control = run.control();
    control.cancel();
    let report = run.run().await?;
    assert!(report.result.unwrap_err().is_canceled());
    assert!(!control.send("too late"));
    Ok(())
}

#[tokio::test]
async fn interactive_requests_receive_messages_through_the_control() -> Result<(), Error> {
    let host = app("uses:\n  - user\n", "return tasks.receive()");
    let run = host.run("chat.md").interactive().prepare()?;
    assert!(run.control().send("hello"));
    assert_eq!(run.run().await?.result?, "hello");
    Ok(())
}

#[tokio::test]
async fn advanced_configuration_preserves_the_hosts_defaults() -> Result<(), Error> {
    let recorder = Arc::new(MemoryRecorder::new());
    let host =
        app("", "store.write('reply.txt', args); return args").with_recorder(recorder.clone());
    let files = runvil::vfs::VfsRef::default();
    let report = host
        .run("configured.md")
        .configure(|setup| {
            setup.vfs = files.clone();
            setup.args = "configured".to_owned();
        })
        .await?;
    assert_eq!(report.result?, "configured");
    assert!(recorder.result(report.run_id).unwrap().is_ok());
    let store = files
        .acquire_store(runvil::vfs::Origin::new("inspect configured run"))
        .unwrap();
    assert_eq!(store.read("reply.txt").unwrap(), b"configured");
    Ok(())
}

struct Frozen;

impl Timer for Frozen {
    fn sleep(&self, _: Duration) -> BoxFuture<'static, ()> {
        Box::pin(async {})
    }

    fn now(&self) -> runvil::Timestamp {
        runvil::Timestamp::UNIX_EPOCH
    }
}

#[tokio::test]
async fn shared_custom_timers_keep_their_start_time() -> Result<(), Error> {
    let recorder = Arc::new(MemoryRecorder::new());
    let host = app("", "return args")
        .with_timer(Arc::new(Frozen))
        .with_recorder(recorder.clone());
    let report = host.run("time.md").await?;
    assert_eq!(
        recorder.meta(report.run_id).unwrap().started_at,
        runvil::Timestamp::UNIX_EPOCH
    );
    Ok(())
}

#[test]
fn request_futures_are_send_and_static() {
    fn require_send(_: impl Send + 'static) {}
    let host = app("", "return 'done'");
    require_send(host.run("thread.md").into_future());
    require_send(host.run("text.md").text());
    require_send(host.run("output.md").output());
    let mut request = pin!(host.run("now.md").into_future());
    // A model-free prompt completes on the caller's poll without a runtime.
    assert!(matches!(
        request
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop())),
        Poll::Ready(Ok(_))
    ));
}
