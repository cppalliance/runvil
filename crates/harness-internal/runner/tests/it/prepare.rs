//! Run preparation through `Harness::run`: a prompt whose requirements
//! the environment cannot meet is prepared, and its run's first step ends
//! it with the Engine's own notice and no record before its end; a
//! recorder that refuses the run's first record fails the run naming it;
//! each run's begin draws a fresh seed and start, both handed to the
//! recorder; and the run's tool performer sends a `ToolCall` effect to
//! the Pack its id names. The run's optional input broker - one of its
//! own services, lent to every tool call and behind the fixture ask Pack -
//! sits in the `input` child module, the run's services meeting a Pack's
//! needs sit in the `host_services` child module, a Pack's prelude
//! reaching the prepared run sits in the `prelude` child module, the
//! prompt's declared input and output files sit in the `files` child
//! module, and the wait for each Pack to be ready sits in the `ready`
//! child module.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use harness_runner::recorder::{
    MemoryRecorder, Record, RecordKind, RecorderFuture, RunId, RunMeta, RunRecorder,
};
use harness_runner::{Packs, RunReport, RunSetup};
use runvil::make_tool_error;
use runvil::{Error, ErrorCode, ErrorFact};
use runvil_plugin::{
    HostServices, Pack, PackFuture, PackId, PackInfo, ToolContext, ToolId, ToolInfo, ToolOutput,
};
use serde_json::{Value, json};

use crate::harness::launch;
use crate::scripted::ScriptedBroker;
use crate::support::{Unused, registry};

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
    Packs::new(registry(&[]), HostServices::new())
}

/// A Host's Packs: `info` beside the built-in.
pub(crate) fn building(info: PackInfo) -> Packs {
    Packs::new(registry(&[info]), HostServices::new())
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
const TOOLS: PackInfo = echo_pack("tests/tools", None);

/// A Pack with the FQN `fqn` that offers `<name>/echo`, with `prelude`.
pub(crate) const fn echo_pack(fqn: &'static str, prelude: Option<&'static str>) -> PackInfo {
    let info = PackInfo::new(fqn, construct_echo);
    match prelude {
        Some(prelude) => info.prelude(prelude),
        None => info,
    }
}

/// A Pack offering the one echo tool.
struct Echo {
    tools: Vec<ToolInfo>,
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "the PackInfo construct signature fixes the return type"
)]
fn construct_echo(
    name: &PackId,
    _config: Value,
    _services: &HostServices,
) -> Result<Arc<dyn Pack>, Error> {
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
    let host = Packs::new(registry(&[TOOLS, extra]), HostServices::new());
    assert_eq!(
        completed(drive_over(offering, host).await),
        "true:tools/echo|extra:extra/echo|hi",
        "the declared Pack is required, the undeclared one an extra, and its tool's \
         object reaches its Pack"
    );
}
