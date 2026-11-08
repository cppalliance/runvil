//! Run preparation through `Harness::run`: a prompt whose requirements
//! the environment cannot meet is prepared, and its run's first step ends
//! it with the Engine's own notice and no record before its end; a
//! recorder that refuses the run's first record fails the run naming it;
//! each run's begin draws a fresh seed and reads its start from the
//! Host's timer, both handed to the recorder; the setup's limits reach
//! the prepared run; each run makes its own instance of each of its Packs
//! and drops it by the time `Harness::run` returns; and the run's tool
//! performer sends a `ToolCall` effect to the Pack its id names. The
//! run's optional input broker - one of its own services, which a Pack's
//! factory takes and the fixture ask Pack holds - sits in the `input`
//! child module, a Pack's factory taking a service from the run's
//! services sits in the `host_services` child module, a
//! Pack's prelude reaching the prepared run sits in the `prelude` child
//! module, the prompt's declared input and output files sit in the
//! `files` child module, the wait for each Pack to be ready sits in
//! the `ready` child module, and the built-in `runvil/user`, which only a
//! run with a user holds, sits in the `user` child module.

use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use harness_runner::performers::{BoxFuture, Timer};
use harness_runner::recorder::{
    MemoryRecorder, Record, RecordKind, RecorderFuture, RunId, RunMeta, RunRecorder,
};
use harness_runner::{Harness, Packs, RunReport, RunSetup};
use runvil::make_tool_error;
use runvil::model::{Completion, CompletionResult, ToolCall};
use runvil::timestamp::Timestamp;
use runvil::{Error, ErrorCode, ErrorFact, RunLimits};
use runvil_plugin::{
    HostServices, Pack, PackFactory, PackFuture, PackId, PackInfo, ToolContext, ToolId, ToolInfo,
    ToolOutput,
};
use serde_json::{Value, json};

use crate::harness::launch;
use crate::scripted::{MODEL, ScriptedBroker};
use crate::support::{Make, Unused, host_over, parsed, registry};

#[path = "prepare-files.rs"]
mod files;
#[path = "prepare-host-services.rs"]
mod host_services;
#[path = "prepare-input.rs"]
mod input;
#[path = "prepare-prelude.rs"]
mod prelude;
#[path = "prepare-ready.rs"]
mod ready;
#[path = "prepare-user.rs"]
mod user;

/// A prompt declaring `web` as a required Pack that no
/// Host here installs.
const NEEDS_WEB: &str = "---\nname: needs-web\ndescription: d\nrunvil: 0\n\
    uses:\n  - web\n---\n\n# Title\n\n## Only\n\nDone.\n";

/// A Pack-free prompt whose one section returns a constant.
const PLAIN: &str = "---\nname: plain\ndescription: d\nrunvil: 0\n---\n\n\
    # Title\n\n## Only\n\n```lua\nreturn 'plain'\n```\n";

/// A prompt declaring the fixture Pack and calling its tool once by id.
const CALLS_ECHO: &str = "---\nname: calls-echo\ndescription: d\nrunvil: 0\n\
    uses:\n  - tests/tools\n---\n\n\
    # Title\n\n## Only\n\n```lua\nreturn tools.call('tools/echo', { value = 'hi' })\n```\n";

/// An empty in-memory recorder.
fn recorder() -> Arc<MemoryRecorder> {
    Arc::new(MemoryRecorder::new())
}

/// Asserts the recorder holds no record for `run_id`: a run that failed
/// preparation, or that its first step refused, records nothing before
/// its end.
#[track_caller]
fn assert_no_records(recorder: &MemoryRecorder, run_id: RunId) {
    let records = recorder.records(run_id);
    assert!(
        records.is_empty(),
        "a run that never started records nothing: {records:?}"
    );
}

/// A Host's Packs with none but the built-in.
pub(crate) fn bare() -> Packs {
    Packs::new(registry(&[]))
}

/// A Host's Packs: `pack`, an info beside its factory, beside the
/// built-in.
pub(crate) fn building(pack: (PackInfo, Make)) -> Packs {
    Packs::new(registry(&[pack]))
}

/// The setup of a run recorded through `recorder` and served by the
/// scripted broker. It selects no model, so the run binds the broker's
/// one model, and it holds no run services, no input, and a fresh memory
/// store.
pub(crate) fn setup_over(recorder: &Arc<MemoryRecorder>) -> RunSetup {
    RunSetup::new(recorder.clone(), Arc::new(ScriptedBroker::replying()))
}

/// Runs `source` with `setup` on a Host over `packs` and a timer no test
/// here reaches, and returns the run's report.
pub(crate) async fn report_of(source: &str, packs: Packs, setup: RunSetup) -> RunReport {
    launch(packs, Unused, source, setup)
        .run()
        .await
        .expect("the run is reported")
}

/// The run a preparation failure names in its `Run` fact.
pub(crate) fn run_of(error: &Error) -> RunId {
    RunId::from_raw(
        error
            .facts
            .get(&ErrorFact::Run)
            .and_then(Value::as_i64)
            .expect("a preparation failure names its run"),
    )
}

/// The fixture Pack `tools`: one tool, `tools/echo`, echoing its
/// `value` argument as trusted text.
const TOOLS: (PackInfo, Make) = echo_pack("tests/tools", None);

/// A Pack with the FQN `fqn` that offers `<name>/echo`, with `prelude`,
/// beside its factory.
pub(crate) const fn echo_pack(
    fqn: &'static str,
    prelude: Option<&'static str>,
) -> (PackInfo, Make) {
    let info = PackInfo::new(fqn);
    let info = match prelude {
        Some(prelude) => info.prelude(prelude),
        None => info,
    };
    (info, new_echo)
}

/// A Pack offering the one echo tool.
struct Echo {
    tools: Vec<ToolInfo>,
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "the PackFactory signature fixes the return type"
)]
fn new_echo(name: &PackId, _run: &HostServices) -> Result<Arc<dyn Pack>, Error> {
    let echo = ToolInfo::new(
        ToolId::parse(&format!("{name}/echo")).unwrap(),
        "Echo the value argument.",
        json!({"type": "object", "properties": {"value": {"type": "string"}}}),
    );
    Ok(Arc::new(Echo { tools: vec![echo] }))
}

impl Pack for Echo {
    fn tools(&self) -> Vec<ToolInfo> {
        self.tools.clone()
    }

    fn call<'a>(
        &'a self,
        _cx: ToolContext<'a>,
        args: Value,
    ) -> PackFuture<'a, Result<ToolOutput, Error>> {
        Box::pin(async move {
            let value = args
                .get("value")
                .and_then(Value::as_str)
                .ok_or_else(|| make_tool_error(ErrorCode::ToolOther, "echo: missing `value`"))?;
            Ok(ToolOutput::trusted(value.to_owned()))
        })
    }
}

/// The wall clock now, UTC milliseconds since the Unix epoch, the unit of
/// `RunMeta::started_at`.
fn unix_millis_now() -> i64 {
    let elapsed = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    i64::try_from(elapsed.as_millis()).unwrap()
}

/// The final text of a completed run.
fn completed(result: Result<String, Error>) -> String {
    result.expect("the run completes")
}

#[tokio::test]
async fn an_unmet_requirement_ends_the_prepared_run_as_failed_with_the_engines_notice() {
    let recorder = recorder();
    let (run_id, result) = prepare_and_drive(NEEDS_WEB, bare(), setup_over(&recorder)).await;

    assert_eq!(
        recorder.result(run_id),
        Some(result.clone()),
        "the recorder holds the refusal as the run's failure"
    );
    assert_eq!(
        refusal(result),
        "the environment cannot satisfy this prompt:\n\
         - missing required Pack: web (at prompt:5)",
        "the failure is the engine's notice, verbatim, on the `uses:` line"
    );
    assert_no_records(&recorder, run_id);
}

/// A recorder that begins runs in memory and refuses every append with a
/// store error of its own.
pub(crate) struct RefusingAppends(pub(crate) MemoryRecorder);

impl RunRecorder for RefusingAppends {
    fn begin_run(&self, meta: RunMeta) -> RecorderFuture<'_, RunId> {
        self.0.begin_run(meta)
    }

    fn append(&self, _run: RunId, _record: Record) -> RecorderFuture<'_, ()> {
        let code = ErrorCode::Io;
        Box::pin(async move { Err(Error::new(code, "the disk is full", code.prose())) })
    }

    fn end_run(&self, run: RunId, result: Result<String, Error>) -> RecorderFuture<'_, ()> {
        self.0.end_run(run, result)
    }
}

#[tokio::test]
async fn a_refused_first_record_fails_the_run_as_a_recorder_failure_naming_the_run() {
    let refusing = Arc::new(RefusingAppends(MemoryRecorder::new()));
    let setup = RunSetup::new(refusing.clone(), Arc::new(ScriptedBroker::replying()));
    let error = launch(bare(), Unused, PLAIN, setup)
        .run()
        .await
        .expect_err("the run's first record is refused");
    assert_eq!(error.code, ErrorCode::RecorderFailed);
    assert_eq!(
        error.message,
        "the run could not be recorded: the run recorder failed: the disk is full"
    );
    assert_eq!(run_of(&error), RunId::from_raw(1));
    assert_eq!(
        refusing.0.result(RunId::from_raw(1)),
        None,
        "a refused run is not recorded to its end"
    );
}

#[tokio::test]
async fn two_runs_draw_different_seeds_and_both_begin_at_the_recorder_at_the_wall_clock() {
    let recorder = recorder();
    let before = unix_millis_now();
    let first = report_of(PLAIN, bare(), setup_over(&recorder)).await;
    let second = report_of(PLAIN, bare(), setup_over(&recorder)).await;
    let after = unix_millis_now();

    assert_ne!(
        first.run_id, second.run_id,
        "each run begins its own record"
    );
    let first_meta = recorder
        .meta(first.run_id)
        .expect("the recorder began the run");
    let second_meta = recorder
        .meta(second.run_id)
        .expect("the recorder began the run");
    assert_ne!(
        first_meta.seed, second_meta.seed,
        "each run draws its own seed"
    );
    assert!(
        first_meta.started_at <= second_meta.started_at,
        "each run reads the clock as it begins: {first_meta:?} then {second_meta:?}"
    );
    for meta in [&first_meta, &second_meta] {
        assert!(
            (before..=after).contains(&meta.started_at),
            "the start is the wall clock at the begin: {} not in {before}..={after}",
            meta.started_at
        );
    }
}

/// A Host's clock that always reads one instant; no test here reaches
/// its sleep.
struct PinnedClock(Timestamp);

impl Timer for PinnedClock {
    fn sleep(&self, _duration: Duration) -> BoxFuture<()> {
        unreachable!("no test here issues a Timer effect")
    }

    fn now(&self) -> Timestamp {
        self.0
    }
}

#[tokio::test]
async fn a_pinned_timer_stamps_the_runs_start() {
    let recorder = recorder();
    let pinned = Timestamp::from_unix_millis(1_700_000_000_000);
    let report = launch(bare(), PinnedClock(pinned), PLAIN, setup_over(&recorder))
        .run()
        .await
        .expect("the run is reported");
    assert_eq!(completed(report.result), "plain");
    let meta = recorder
        .meta(report.run_id)
        .expect("the recorder began the run");
    assert_eq!(
        meta.started_at,
        pinned.unix_millis(),
        "the run starts at the Host's clock"
    );
}

/// A prompt declaring the fixture Pack whose one section loops over a
/// model that calls the echo tool every round, and returns how the loop
/// ended.
const LOOPS_ON_ECHO: &str = "---\nname: loops\ndescription: d\nrunvil: 0\n\
    uses:\n  - tests/tools\n---\n\n\
    # Title\n\n## Only\n\n```lua\n\
    tools.offer('tools/echo')\nmessages:user('go')\n\
    local ok, err = pcall(models.loop)\nreturn tostring(ok) .. '|' .. err.kind\n```\n";

#[tokio::test]
async fn the_setups_limits_reach_the_run() {
    let calls = Arc::new(AtomicUsize::new(0));
    let broker = ScriptedBroker::new(move |_round, _messages| {
        let call = calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            let echo = ToolCall::from_parts(
                format!("call_{call}"),
                "tools_echo",
                json!({ "value": "hi" }),
            )?;
            Completion::from_result(CompletionResult::ToolCalls(vec![echo]), MODEL).map(Box::new)
        })
    });
    let rounds = broker.rounds();
    let mut limits = RunLimits::default();
    limits.tool_iterations_ceiling = NonZeroU32::new(2).expect("2 is non-zero");
    let setup = RunSetup {
        limits,
        ..RunSetup::new(recorder(), Arc::new(broker))
    };

    let report = report_of(LOOPS_ON_ECHO, building(TOOLS), setup).await;
    assert_eq!(completed(report.result), "false|tool_loop_exhausted");
    assert_eq!(
        rounds.lock().unwrap().len(),
        2,
        "the setup's ceiling ends the loop after two rounds"
    );
}

#[tokio::test]
async fn a_prepared_run_drives_to_its_end_and_the_recorder_holds_its_result() {
    let recorder = recorder();
    let (run_id, result) = prepare_and_drive(PLAIN, bare(), setup_over(&recorder)).await;
    assert_eq!(completed(result.clone()), "plain");
    assert_eq!(
        recorder.result(run_id),
        Some(result),
        "the loop ends the run the Harness began"
    );
}

/// Runs `source` with `setup` on a Host over `packs`, returning the run's
/// id at the recorder beside its result.
pub(crate) async fn prepare_and_drive(
    source: &str,
    packs: Packs,
    setup: RunSetup,
) -> (RunId, Result<String, Error>) {
    let report = report_of(source, packs, setup).await;
    (report.run_id, report.result)
}

/// Runs `source` on a Host over `packs` and returns the run's result.
pub(crate) async fn drive_over(source: &str, packs: Packs) -> Result<String, Error> {
    prepare_and_drive(source, packs, setup_over(&recorder()))
        .await
        .1
}

/// The located notice of a run its first step refused, after asserting
/// that the run ended with a `RequirementsUnmet` failure.
#[track_caller]
pub(crate) fn refusal(result: Result<String, Error>) -> String {
    match result {
        Err(error) if error.code == ErrorCode::RequirementsUnmet => error.located_message(),
        other => panic!("the run's first step refuses it: {other:?}"),
    }
}

/// How many [`Counted`] instances a factory made, and how many dropped.
#[derive(Default)]
struct Counts {
    made: AtomicUsize,
    dropped: AtomicUsize,
}

/// An echo Pack instance that counts its drop in the [`Counts`] it
/// shares with its factory.
struct Counted {
    echo: Arc<dyn Pack>,
    counts: Arc<Counts>,
}

impl Drop for Counted {
    fn drop(&mut self) {
        self.counts.dropped.fetch_add(1, Ordering::SeqCst);
    }
}

impl Pack for Counted {
    fn tools(&self) -> Vec<ToolInfo> {
        self.echo.tools()
    }

    fn call<'a>(
        &'a self,
        cx: ToolContext<'a>,
        args: Value,
    ) -> PackFuture<'a, Result<ToolOutput, Error>> {
        self.echo.call(cx, args)
    }
}

/// The factory of a [`Counted`] Pack, counting each instance it makes in
/// `counts`.
fn counting(counts: Arc<Counts>) -> impl PackFactory {
    move |name: &PackId, run: &HostServices| -> Result<Arc<dyn Pack>, Error> {
        counts.made.fetch_add(1, Ordering::SeqCst);
        Ok(Arc::new(Counted {
            echo: new_echo(name, run)?,
            counts: Arc::clone(&counts),
        }))
    }
}

#[tokio::test]
async fn each_run_makes_its_own_instance_and_drops_it_by_the_time_harness_run_returns() {
    let counts = Arc::new(Counts::default());
    let mut packs = registry(&[]);
    packs
        .insert(PackInfo::new("tests/tools"), counting(Arc::clone(&counts)))
        .unwrap();
    let host = host_over(Packs::new(packs), Unused);
    assert_eq!(
        counts.made.load(Ordering::SeqCst),
        0,
        "installing the Pack makes no instance"
    );
    for runs in 1..=2 {
        let report = Harness::new(
            Arc::clone(&host),
            parsed(CALLS_ECHO),
            setup_over(&recorder()),
        )
        .run()
        .await
        .expect("the run is reported");
        assert_eq!(completed(report.result), "hi");
        assert_eq!(
            counts.made.load(Ordering::SeqCst),
            runs,
            "each run makes its own instance"
        );
        assert_eq!(
            counts.dropped.load(Ordering::SeqCst),
            runs,
            "the run's instance is dropped by the time Harness::run returns"
        );
    }
}

#[tokio::test]
async fn the_tool_performer_sends_the_effect_to_the_pack_its_id_names() {
    assert_eq!(
        completed(drive_over(CALLS_ECHO, building(TOOLS)).await),
        "hi",
        "the ToolCall effect reached the echo tool"
    );
}

#[tokio::test]
async fn a_script_reaches_an_undeclared_runvil_packs_tool_by_full_id_without_its_prelude() {
    let undeclared = "---\nname: undeclared\ndescription: d\nrunvil: 0\n---\n\n\
        # Title\n\n## Only\n\n```lua\n\
        return tools.call('extra/echo', { value = 'hi' }) .. '|' .. type(extra)\n```\n";
    let extra = echo_pack("runvil/extra", Some("extra = {}\n"));
    assert_eq!(
        completed(drive_over(undeclared, building(extra)).await),
        "hi|nil",
        "the undeclared Pack's tool is in the catalog, and its prelude is not installed"
    );
}

#[tokio::test]
async fn an_undeclared_third_party_pack_is_not_in_the_run() {
    let undeclared = "---\nname: undeclared\ndescription: d\nrunvil: 0\n---\n\n\
        # Title\n\n## Only\n\n```lua\n\
        local ok = pcall(tools.call, 'tools/echo', { value = 'hi' })\n\
        return tostring(ok) .. '|' .. tostring(tools.packs.get('tools'))\n```\n";
    assert_eq!(
        completed(drive_over(undeclared, building(TOOLS)).await),
        "false|nil",
        "tests/tools reaches only a run that declares it"
    );
}

#[tokio::test]
async fn a_bare_entry_another_vendor_answers_is_refused_naming_its_fqn() {
    let declares_bare = "---\nname: bare\ndescription: d\nrunvil: 0\n\
        uses:\n  - tools\n---\n\n# Title\n\n## Only\n\nDone.\n";
    assert_eq!(
        refusal(drive_over(declares_bare, building(TOOLS)).await),
        "the environment cannot satisfy this prompt:\n\
         - tools is unavailable: tools means runvil/tools, which is not installed; \
         write tests/tools to use that Pack (at prompt:5)",
        "a bare id means the runvil Pack, which the Host lacks"
    );
}

#[tokio::test]
async fn the_packs_split_by_declaration_and_a_script_calls_an_extras_tool_object() {
    let offering = "---\nname: offering\ndescription: d\nrunvil: 0\n\
        uses:\n  - tests/tools\n---\n\n# Title\n\n## Only\n\n```lua\n\
        local declared = tools.packs.get('tools')\n\
        local extras = tools.packs.extras()\n\
        local extra = extras[1].tools[1]\n\
        return tostring(declared.required) .. ':' .. declared.tools[1].id .. '|' .. \
          extras[1].name .. ':' .. extra.id .. '|' .. tools.call(extra, { value = 'hi' })\n```\n";
    let extra = echo_pack("runvil/extra", None);
    let host = Packs::new(registry(&[TOOLS, extra]));
    assert_eq!(
        completed(drive_over(offering, host).await),
        "true:tools/echo|extra:extra/echo|hi",
        "the declared Pack is required, the undeclared one an extra, and its tool's \
         object reaches its Pack"
    );
}
