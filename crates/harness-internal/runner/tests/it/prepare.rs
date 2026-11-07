//! Run preparation: a prompt whose requirements the environment cannot
//! meet is prepared, and its run's first step ends it with the Engine's
//! own notice; a prompt that does not parse fails preparation with the
//! parse failure, which its run ends with; each leaves only its parse
//! events at the recorder; each run's begin draws a fresh seed and start,
//! both handed to the recorder under the run's name; and the prepared tool
//! performer sends a `ToolCall` effect to the Pack its id names. The
//! run's optional input broker - one of its own services, lent to every
//! tool call and behind the fixture ask Pack - sits in the
//! `input` child module, the run's services meeting a Pack's needs sit in
//! the `host_services` child module, a Pack's prelude reaching the
//! prepared run sits in the `prelude` child module, the prompt's declared
//! input and output files sit in the `files` child module, and the wait
//! for each Pack to be ready sits in the `ready` child module.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use harness_runner::HostContext;
use harness_runner::effect_loop::drive_run;
use harness_runner::prepare::{Prepared, Services, begin, prepare};
use harness_runner::recorder::{
    MemoryRecorder, Record, RecordKind, RecorderFuture, RunId, RunMeta, RunRecorder,
};
use runvil::cancel::CancelHandle;
use runvil::event::Event;
use runvil::tools::tool_error;
use runvil::{Error, ErrorCode, ErrorFact};
use runvil_plugin::{
    HostServices, Pack, PackFuture, PackId, PackInfo, ToolContext, ToolId, ToolInfo, ToolOutput,
};
use serde_json::{Value, json};

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

/// A prompt with unclosed frontmatter, so it does not parse.
const UNCLOSED: &str = "---\nname: unclosed\ndescription: d\nrunvil: 0\n\n# Title\n";

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

/// The events the recorder holds for `run_id`, after asserting they are
/// only events and open with the parse. A run that failed preparation, or
/// that its first step refused, records nothing after its parse events.
fn recorded_parse_events(recorder: &MemoryRecorder, run_id: RunId) -> Vec<Event> {
    let records = recorder.records(run_id);
    assert!(
        records
            .iter()
            .all(|record| record.kind == RecordKind::Event),
        "a run that never started records only events"
    );
    let events: Vec<Event> = records
        .into_iter()
        .map(|record| serde_json::from_value(record.payload).unwrap())
        .collect();
    assert!(
        matches!(events.first(), Some(Event::ParseStarted { .. })),
        "the events open with the parse: {events:?}"
    );
    events
}

/// A Host with no Packs but the built-in.
pub(crate) fn bare() -> HostContext {
    HostContext::new(registry(&[]), HostServices::new())
}

/// A Host that built `info` beside the built-in.
pub(crate) fn building(info: PackInfo) -> HostContext {
    HostContext::new(registry(&[info]), HostServices::new())
}

/// The preparation services over `recorder` and `host`, with no run
/// services and the performers no test here reaches.
pub(crate) fn services(recorder: &Arc<MemoryRecorder>, host: HostContext) -> Services {
    Services {
        host: Arc::new(host),
        services: HostServices::new(),
        vfs: runvil::vfs::VfsRef::default(),
        input_text: None,
        cancel: CancelHandle::new(),
        recorder: recorder.clone(),
        broker: Arc::new(Unused),
        timer: Arc::new(Unused),
        name: "session-1".to_owned(),
        file: "prompt".to_owned(),
        model: None,
        ui: None,
    }
}

/// Begins a run of `source` under the name `services` holds at the
/// recorder it holds, then prepares the run with no arguments.
pub(crate) async fn begin_and_prepare(source: &str, services: Services) -> Result<Prepared, Error> {
    let begun = begin(&*services.recorder, &services.name, source)
        .await
        .expect("the memory recorder begins the run");
    prepare(source, "", begun, services).await
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
                .ok_or_else(|| tool_error(ErrorCode::ToolOther, "echo: missing `value`"))?;
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
    let (run_id, result) = prepare_and_drive(NEEDS_WEB, services(&recorder, bare())).await;

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
    let events = recorded_parse_events(&recorder, run_id);
    assert!(
        matches!(events.last(), Some(Event::ParseSucceeded { .. })),
        "the parse events, then the end, with no run event between: {events:?}"
    );
}

#[tokio::test]
async fn a_prompt_that_does_not_parse_fails_preparation_and_its_run_ends_with_the_parse_failure() {
    let recorder = recorder();
    let error = begin_and_prepare(UNCLOSED, services(&recorder, bare()))
        .await
        .expect_err("a prompt without a closed frontmatter does not parse");

    assert_eq!(
        error.code,
        ErrorCode::FrontmatterInvalid,
        "the failure is the parse failure"
    );
    assert_eq!(
        error.located_message(),
        "frontmatter was not closed with --- (line 1, column 1)",
        "the parse failure keeps its location"
    );
    let run_id = run_of(&error);
    assert_eq!(
        recorder.result(run_id),
        Some(Err(error)),
        "the recorder holds the very failure preparation returned"
    );
    let events = recorded_parse_events(&recorder, run_id);
    assert!(
        matches!(events.last(), Some(Event::ParseFailed { .. })),
        "the events end with the failed parse: {events:?}"
    );
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
async fn a_refused_parse_event_fails_preparation_as_a_recorder_failure_naming_the_run() {
    let mut services = services(&recorder(), bare());
    services.recorder = Arc::new(RefusingAppends(MemoryRecorder::new()));
    let error = begin_and_prepare(PLAIN, services)
        .await
        .expect_err("the first parse event is refused");
    assert_eq!(error.code, ErrorCode::RecorderFailed);
    assert_eq!(error.message, "the run recorder failed: the disk is full");
    assert_eq!(run_of(&error), RunId::from_raw(1));
}

#[tokio::test]
async fn two_prepared_runs_draw_different_seeds_and_both_begin_at_the_recorder() {
    let recorder = recorder();
    let before = unix_millis_now();
    let first = begin_and_prepare(PLAIN, services(&recorder, bare()))
        .await
        .unwrap();
    let second = begin_and_prepare(PLAIN, services(&recorder, bare()))
        .await
        .unwrap();
    let after = unix_millis_now();

    assert_ne!(
        first.run_id, second.run_id,
        "each preparation begins its own run"
    );
    let first_meta = recorder
        .meta(first.run_id)
        .expect("the recorder began the run");
    let second_meta = recorder
        .meta(second.run_id)
        .expect("the recorder began the run");
    assert_ne!(
        first_meta.seed, second_meta.seed,
        "each preparation draws its own seed"
    );
    assert!(
        first_meta.started_at <= second_meta.started_at,
        "each preparation reads the clock as it begins its run: {first_meta:?} then {second_meta:?}"
    );
    for prepared in [&first, &second] {
        let meta = recorder
            .meta(prepared.run_id)
            .expect("the recorder began the run");
        assert!(
            (before..=after).contains(&meta.started_at),
            "the start is the wall clock at preparation: {} not in {before}..={after}",
            meta.started_at
        );
        assert_eq!(meta.name, "session-1", "the metadata holds the run's name");
        assert!(
            meta.prompt_hash.starts_with("sha256:"),
            "the prompt hash names its algorithm: {}",
            meta.prompt_hash
        );
        assert_eq!(
            recorder.result(prepared.run_id),
            None,
            "a prepared run stays open for the loop"
        );
    }
    assert_eq!(
        recorder.meta(first.run_id).unwrap().prompt_hash,
        recorder.meta(second.run_id).unwrap().prompt_hash,
        "the same prompt text hashes the same"
    );
}

#[tokio::test]
async fn a_prepared_run_drives_to_its_end_under_its_own_performers() {
    let recorder = recorder();
    let Prepared {
        run,
        run_id,
        performers,
        ..
    } = begin_and_prepare(PLAIN, services(&recorder, bare()))
        .await
        .unwrap();
    let result = drive_run(
        run,
        performers,
        recorder.clone(),
        run_id,
        CancelHandle::new(),
    )
    .await;
    assert_eq!(completed(result.clone()), "plain");
    assert_eq!(
        recorder.result(run_id),
        Some(result),
        "the loop ends the run preparation began"
    );
}

/// Prepares `source` with `services` and drives the run to its end,
/// returning the run's id at the recorder beside its result.
pub(crate) async fn prepare_and_drive(
    source: &str,
    services: Services,
) -> (RunId, Result<String, Error>) {
    let recorder = Arc::clone(&services.recorder);
    let prepared = begin_and_prepare(source, services).await.unwrap();
    let result = drive_run(
        prepared.run,
        prepared.performers,
        recorder,
        prepared.run_id,
        CancelHandle::new(),
    )
    .await;
    (prepared.run_id, result)
}

/// Prepares `source` over `host` and drives the run to its end.
pub(crate) async fn drive_over(source: &str, host: HostContext) -> Result<String, Error> {
    prepare_and_drive(source, services(&recorder(), host))
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
    let host = HostContext::new(registry(&[TOOLS, extra]), HostServices::new());
    assert_eq!(
        completed(drive_over(offering, host).await),
        "true:tools/echo|extra:extra/echo|hi",
        "the declared Pack is required, the undeclared one an extra, and its tool's \
         object reaches its Pack"
    );
}
