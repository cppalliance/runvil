//! Fixtures shared by the runner's suites: a one-section prompt, a run
//! over it, fake performers that answer by script, a Pack registry
//! holding a suite's Packs, and a Host over them.

use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::Poll;
use std::time::Duration;

use harness_runner::performers::{InferenceBroker, Timer, ToolPerformer};
use harness_runner::recorder::{
    MemoryRecorder, Record, RunId, RunMeta, RunRecorder, make_recorder_error,
};
use harness_runner::{Host, Packs, RunSetup};
use runvil_engine::cancel::CancelHandle;
use runvil_engine::effect::{Round, ToolCallOrigin};
use runvil_engine::model::{
    Completion, CompletionOptions, Message, ModelBinding, ModelCatalog, ToolSchema,
};
use runvil_engine::timestamp::Timestamp;
use runvil_engine::vfs::{
    Access, AcquireContext, Entry, ExecId, MemoryBackend, Stat, Vfs, VfsAccess, VfsError, VfsPath,
    VfsRef,
};
use runvil_engine::{BoxFuture, Error, ErrorCode, ErrorFact};
use runvil_engine::{Prompt, Run, RunParams, RunSeed};
use runvil_engine::{ToolCatalog, ToolId, ToolInfo, ToolOutput};
use runvil_plugin::{Pack, PackId, PackInfo, PackRegistry};
use serde_json::{Value, json};

/// The fixed seed every fixture run and recorded meta takes.
pub(crate) const TEST_SEED: RunSeed = RunSeed::from_bytes([7; 32]);

/// Asserts a report's `output` is the failure of a run that did not
/// complete.
pub(crate) fn assert_not_completed(output: &Result<String, Error>) {
    let Err(error) = output else {
        panic!("a run that did not complete reports no output: {output:?}");
    };
    assert_eq!(error.code, ErrorCode::NoOutput);
    assert_eq!(
        error.facts.get(&ErrorFact::Reason),
        Some(&json!("not_completed"))
    );
}

/// Asserts a run's `result` is a cancel: the error whose code is
/// `canceled`.
#[track_caller]
pub(crate) fn assert_canceled(result: &Result<String, Error>) {
    assert!(
        result.as_ref().is_err_and(Error::is_canceled),
        "the run ends canceled: {result:?}"
    );
}

/// The factory of a fixture Pack that shares nothing: it makes the Pack
/// under the Pack id it receives.
pub(crate) type Make = fn(&PackId) -> Result<Arc<dyn Pack>, Error>;

/// A registry holding the built-ins and each of `packs`, an info beside
/// its factory, which a suite builds its Host over.
pub(crate) fn registry(packs: &[(PackInfo, Make)]) -> PackRegistry {
    let mut registry = PackRegistry::new();
    for (info, make) in packs {
        registry.insert(*info, *make).unwrap();
    }
    registry
}

/// A Host over `packs` that sleeps through `timer`. Its finder serves no
/// prompt: a suite parses its own with [`parsed`], so a prompt the Host's
/// Packs cannot serve still reaches the run.
pub(crate) fn host_over(packs: Packs, timer: impl Timer + 'static) -> Arc<Host> {
    Arc::new(Host {
        packs,
        prompts: Arc::new(|name: &str| -> Result<String, Error> {
            let code = ErrorCode::NotFound;
            Err(Error::new(
                code,
                format!("no prompt named {name}"),
                code.prose(),
            ))
        }),
        timer: Arc::new(timer),
    })
}

/// `source` parsed as the prompt named `prompt`.
pub(crate) fn parsed(source: &str) -> Prompt {
    let (prompt, _parse_events) = Prompt::parse(&|_: &str| Ok(source.to_owned()), "prompt");
    prompt.expect("the suite's prompt parses")
}

/// A section body that calls the fixture wait tool by its full id and
/// returns its answer: the section parks until the test's tool performer
/// answers.
pub(crate) const WAITS: &str = "return tools.call('tests/runner/wait')";

/// The catalog every fixture run is prepared against: the one wait tool
/// [`WAITS`] calls, so a section reaches it by full id without binding an
/// alias.
fn catalog() -> ToolCatalog {
    let wait = ToolInfo::new(
        ToolId::parse("tests/runner/wait").expect("the wait tool id is valid"),
        "Wait until the test's tool performer answers.",
        json!({ "type": "object", "properties": {} }),
    );
    ToolCatalog::new([wait]).expect("the one-tool catalog is valid")
}

/// A prompt whose sections are `sections`, each a section's name and the
/// Lua it runs as its only block, in order.
fn prompt(sections: &[(&str, &str)]) -> Arc<Prompt> {
    let mut source = String::from(
        "---\nname: runner-test\ndescription: a runner fixture\nrunvil: 0\n---\n\n# Fixture\n",
    );
    for (name, lua) in sections {
        write!(source, "\n## {name}\n\n```lua\n{lua}\n```\n").expect("a String takes any write");
    }
    let (prompt, _parse_events) = Prompt::parse(&|_: &str| Ok(source.clone()), "prompt");
    Arc::new(prompt.expect("the fixture prompt parses"))
}

/// A Pack-free run over `prompt` with a fixed seed and start,
/// prepared against the fixture [`catalog`], over the run's filesystem
/// `vfs`, under the cancel flag `cancel`.
fn prepared(prompt: Arc<Prompt>, vfs: VfsRef, cancel: &CancelHandle) -> Run {
    let ctx = RunParams::new(TEST_SEED, Timestamp::UNIX_EPOCH)
        .with_vfs(vfs)
        .with_tools(catalog())
        .with_cancel(cancel.clone());
    Run::new(prompt, "", ctx)
}

/// A Pack-free run over `lua` with a fixed seed and start, over a
/// fresh memory store.
pub(crate) fn run(lua: &str) -> Run {
    run_over(lua, VfsRef::default())
}

/// A Pack-free run over `lua` whose `store` table operates on
/// `vfs`: the test holds the handle, so it can read what the run wrote or
/// mount a backend of its own.
pub(crate) fn run_over(lua: &str, vfs: VfsRef) -> Run {
    run_of_sections(&[("Only", lua)], vfs)
}

/// A Pack-free run over `sections`, each a section's name and the Lua it
/// runs, whose `store` table operates on `vfs`.
pub(crate) fn run_of_sections(sections: &[(&str, &str)], vfs: VfsRef) -> Run {
    prepared(prompt(sections), vfs, &CancelHandle::new())
}

/// [`run`] under the cancel flag `cancel`, which the test also hands the
/// loop.
pub(crate) fn run_with_cancel(lua: &str, cancel: &CancelHandle) -> Run {
    prepared(prompt(&[("Only", lua)]), VfsRef::default(), cancel)
}

/// A Pack-free run over two sections: `## Main` runs `main`, and
/// `## Child` runs `child` when the main spawns it as a task.
pub(crate) fn run_with_child(main: &str, child: &str) -> Run {
    run_with_child_and_cancel(main, child, &CancelHandle::new())
}

/// [`run_with_child`] under the cancel flag `cancel`, which the test also
/// hands the loop.
pub(crate) fn run_with_child_and_cancel(main: &str, child: &str, cancel: &CancelHandle) -> Run {
    let sections = [("Main", main), ("Child", child)];
    prepared(prompt(&sections), VfsRef::default(), cancel)
}

/// A main section that parks on a 30-second timer beside its child: the
/// child's own tool call and the timer are two effects out at once.
pub(crate) const TIMED_MAIN: &str = "local t = tasks.spawn('## Child')\n\
     local _first, _ok, result = tasks.join_any({ t }, { timeout = 30 })\n\
     return result";

/// A performer for every kind that no test here expects to be issued;
/// reaching one is the test's failure.
pub(crate) struct Unused;

impl InferenceBroker for Unused {
    fn models(&self) -> BoxFuture<'static, Result<ModelCatalog, Error>> {
        unreachable!("the effect loop never lists models")
    }

    fn chat(
        &self,
        _binding: ModelBinding,
        _messages: Vec<Message>,
        _tools: Vec<ToolSchema>,
        _options: CompletionOptions,
        _round: Round,
    ) -> BoxFuture<'static, Result<Box<Completion>, Error>> {
        unreachable!("no test issues a Chat effect")
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
    ) -> BoxFuture<'static, Result<ToolOutput, Error>> {
        unreachable!("this test issues no ToolCall effect")
    }
}

impl Timer for Unused {
    fn sleep(&self, _duration: Duration) -> BoxFuture<'static, ()> {
        unreachable!("no test issues a Timer effect")
    }
}

/// The setup of a run the effect loop drives alone, recorded through
/// `recorder`, whose broker no test here reaches; a test that issues
/// `Chat` effects sets its own broker.
pub(crate) fn loop_setup(recorder: Arc<dyn RunRecorder>) -> RunSetup {
    RunSetup::new(recorder, Arc::new(Unused))
}

/// Answers every tool call with the same trusted text.
pub(crate) struct TextTool(pub(crate) &'static str);

impl ToolPerformer for TextTool {
    fn call(
        &self,
        _tool: ToolId,
        _alias: String,
        _access: Arc<Access>,
        _origin: ToolCallOrigin,
        _args: Value,
    ) -> BoxFuture<'static, Result<ToolOutput, Error>> {
        let text = self.0;
        Box::pin(async move { Ok(ToolOutput::trusted(text)) })
    }
}

/// Stays pending forever: the tool call that never returns.
pub(crate) struct PendingTool;

impl ToolPerformer for PendingTool {
    fn call(
        &self,
        _tool: ToolId,
        _alias: String,
        _access: Arc<Access>,
        _origin: ToolCallOrigin,
        _args: Value,
    ) -> BoxFuture<'static, Result<ToolOutput, Error>> {
        Box::pin(std::future::pending())
    }
}

/// Panics instead of answering: a performer the Harness lost to a bug. The
/// call panics on its first poll.
pub(crate) struct PanickingTool;

impl ToolPerformer for PanickingTool {
    fn call(
        &self,
        _tool: ToolId,
        _alias: String,
        _access: Arc<Access>,
        _origin: ToolCallOrigin,
        _args: Value,
    ) -> BoxFuture<'static, Result<ToolOutput, Error>> {
        Box::pin(std::future::poll_fn(
            |_cx| -> Poll<Result<ToolOutput, Error>> {
                panic!("the tool performer panics instead of answering")
            },
        ))
    }
}

/// Ends the run at the recorder before answering, so the loop's next
/// write is refused: the recorder failing under a live run.
pub(crate) struct ClosingTool {
    pub(crate) recorder: Arc<MemoryRecorder>,
    pub(crate) run_id: RunId,
}

impl ToolPerformer for ClosingTool {
    fn call(
        &self,
        _tool: ToolId,
        _alias: String,
        _access: Arc<Access>,
        _origin: ToolCallOrigin,
        _args: Value,
    ) -> BoxFuture<'static, Result<ToolOutput, Error>> {
        let recorder = Arc::clone(&self.recorder);
        let run_id = self.run_id;
        Box::pin(async move {
            recorder
                .end_run(run_id, Err(Error::canceled()))
                .await
                .expect("the open run ends");
            Ok(ToolOutput::trusted("late"))
        })
    }
}

/// A recorder that refuses one chosen call and keeps the rest in memory:
/// the recorder failing under a live run. Calls count from one across
/// `begin_run`, `append`, and `end_run` in the order they reach it, and a
/// refused call records nothing. A test that drives the loop alone begins
/// its run on `inner()`, which counts nothing, so the count starts at the
/// loop's first write.
pub(crate) struct FailingRecorder {
    inner: MemoryRecorder,
    fail_on: usize,
    calls: AtomicUsize,
    begun: Mutex<Vec<RunId>>,
}

impl FailingRecorder {
    /// A recorder that refuses its `call`th call, counting from one.
    pub(crate) fn failing_on(call: usize) -> Self {
        Self {
            inner: MemoryRecorder::new(),
            fail_on: call,
            calls: AtomicUsize::new(0),
            begun: Mutex::new(Vec::new()),
        }
    }

    /// The runs this recorder issued through its own `begin_run`, in
    /// order.
    pub(crate) fn begun(&self) -> Vec<RunId> {
        self.begun.lock().unwrap().clone()
    }

    /// A recorder that refuses nothing, for counting the calls of a run.
    pub(crate) fn never_failing() -> Self {
        Self::failing_on(usize::MAX)
    }

    /// The calls that have reached the recorder, refused one included.
    pub(crate) fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    /// The memory recorder behind the wrapper, which holds what was kept.
    pub(crate) fn inner(&self) -> &MemoryRecorder {
        &self.inner
    }

    fn count(&self) -> Result<(), runvil_engine::Error> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if call == self.fail_on {
            return Err(make_recorder_error(format!(
                "the fixture recorder refuses call {call}"
            )));
        }
        Ok(())
    }
}

impl RunRecorder for FailingRecorder {
    fn begin_run(&self, meta: RunMeta) -> BoxFuture<'_, Result<RunId, Error>> {
        Box::pin(async move {
            self.count()?;
            let run = self.inner.begin_run(meta).await?;
            self.begun.lock().unwrap().push(run);
            Ok(run)
        })
    }

    fn append(&self, run: RunId, record: Record) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async move {
            self.count()?;
            self.inner.append(run, record).await
        })
    }

    fn end_run(
        &self,
        run: RunId,
        result: Result<String, Error>,
    ) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async move {
            self.count()?;
            self.inner.end_run(run, result).await
        })
    }
}

/// Raises its flag when dropped: how a test sees a future torn down.
pub(crate) struct RaiseOnDrop(pub(crate) Arc<AtomicBool>);

impl Drop for RaiseOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// Never fires, and raises `dropped` when its sleep is torn down: the
/// timer a cancel or an abort must reach.
#[derive(Default)]
pub(crate) struct PendingTimer {
    pub(crate) dropped: Arc<AtomicBool>,
}

impl Timer for PendingTimer {
    fn sleep(&self, _duration: Duration) -> BoxFuture<'static, ()> {
        let raise = RaiseOnDrop(Arc::clone(&self.dropped));
        Box::pin(async move {
            let _raise = raise;
            std::future::pending::<()>().await;
        })
    }
}

/// Parks every tool call until the test adds a permit to `gate`, then
/// answers it: a tool call that stays out for exactly as long as the test
/// says, with no timer involved.
pub(crate) struct GatedTool {
    pub(crate) gate: Arc<tokio::sync::Semaphore>,
}

impl ToolPerformer for GatedTool {
    fn call(
        &self,
        _tool: ToolId,
        _alias: String,
        _access: Arc<Access>,
        _origin: ToolCallOrigin,
        _args: Value,
    ) -> BoxFuture<'static, Result<ToolOutput, Error>> {
        let gate = Arc::clone(&self.gate);
        Box::pin(async move {
            gate.acquire()
                .await
                .expect("the gate semaphore is never closed")
                .forget();
            Ok(ToolOutput::trusted("opened"))
        })
    }
}

/// A [`MemoryBackend`] that runs `on_write` as each write starts and
/// delegates every operation to the memory backend: a test mounts it as
/// the run's store, so a store operation reaches it through the run's
/// real store view, and the hook sees where the operation runs or makes
/// the backend panic in the one chosen operation.
pub(crate) struct HookedBackend {
    inner: MemoryBackend,
    on_write: Arc<dyn Fn() + Send + Sync>,
}

impl HookedBackend {
    pub(crate) fn new(on_write: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            inner: MemoryBackend::new(),
            on_write: Arc::new(on_write),
        }
    }
}

impl Vfs for HookedBackend {
    fn acquire(&mut self, cx: &AcquireContext) -> Result<Box<dyn VfsAccess>, VfsError> {
        Ok(Box::new(HookedAccess {
            inner: self.inner.acquire(cx)?,
            on_write: Arc::clone(&self.on_write),
        }))
    }

    fn release(&mut self, id: ExecId) -> Result<(), VfsError> {
        self.inner.release(id)
    }
}

/// One session with a [`HookedBackend`].
struct HookedAccess {
    inner: Box<dyn VfsAccess>,
    on_write: Arc<dyn Fn() + Send + Sync>,
}

impl VfsAccess for HookedAccess {
    fn read(&self, path: &VfsPath) -> Result<Vec<u8>, VfsError> {
        self.inner.read(path)
    }

    fn write(&mut self, path: &VfsPath, contents: &[u8]) -> Result<(), VfsError> {
        (self.on_write)();
        self.inner.write(path, contents)
    }

    fn append(&mut self, path: &VfsPath, contents: &[u8]) -> Result<(), VfsError> {
        self.inner.append(path, contents)
    }

    fn remove(&mut self, path: &VfsPath, recursive: bool) -> Result<(), VfsError> {
        self.inner.remove(path, recursive)
    }

    fn exists(&self, path: &VfsPath) -> Result<bool, VfsError> {
        self.inner.exists(path)
    }

    fn glob(&self, pattern: &str) -> Result<Vec<String>, VfsError> {
        self.inner.glob(pattern)
    }

    fn list(&self, path: &VfsPath) -> Result<Vec<Entry>, VfsError> {
        self.inner.list(path)
    }

    fn stat(&self, path: &VfsPath) -> Result<Stat, VfsError> {
        self.inner.stat(path)
    }

    fn mkdir(&mut self, path: &VfsPath, recursive: bool) -> Result<(), VfsError> {
        self.inner.mkdir(path, recursive)
    }

    fn rename(&mut self, from: &VfsPath, to: &VfsPath) -> Result<(), VfsError> {
        self.inner.rename(from, to)
    }

    fn copy(&mut self, from: &VfsPath, to: &VfsPath) -> Result<(), VfsError> {
        self.inner.copy(from, to)
    }
}
