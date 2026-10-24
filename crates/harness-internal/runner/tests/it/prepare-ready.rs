//! Readiness at preparation: the Harness waits for the `ready` of each of
//! the run's Packs before the run's snapshot, asking each one before any
//! answers, so a Pack still starting has its tools in the run once it is
//! ready, a Pack whose `ready` fails is unavailable to that run and
//! offering it raises its reason, an undeclared third-party Pack that
//! never gets ready holds no run up, a built Pack with no tools still
//! reaches the prompt's Lua, a cancel during the wait ends the run as
//! cancelled in both the report and the record, a cancel that lands while
//! a refusal is recorded leaves the report holding what the record holds,
//! and a Pack that keeps the default `ready` serves as before.

use super::*;

use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::Duration;

use futures_util::FutureExt as _;
use harness_runner::environment::HostSnapshot;
use harness_runner::files::OutputError;
use harness_runner::recorder::{Record, RecorderFuture, RunMeta, RunRecorder};
use harness_runner::{Harness, RunControl, RunRequest};
use runvil::vfs::VfsRef;
use runvil_plugin::ServiceKey;
use tokio::sync::{Notify, watch};

use crate::scripted::{MODEL, ScriptedBroker};

/// How long a test waits for a run before it fails.
const PATIENCE: Duration = Duration::from_secs(10);

/// Where a [`Starting`] Pack's startup stands; the test moves it.
#[derive(Clone, Copy)]
enum Phase {
    Starting,
    Ready,
    Failed(&'static str),
}

/// The startup a test drives for one [`Starting`] Pack.
struct Startup {
    phase: watch::Receiver<Phase>,
    /// Notified when the Harness first polls the Pack's `ready`.
    asked: Notify,
}

/// One [`Starting`] Pack's sender that moves its startup, beside the
/// startup it shares.
type StartupHandle = (watch::Sender<Phase>, Arc<Startup>);

/// Each [`Starting`] Pack's startup by its Pack id, shared with the
/// Packs as a Host-wide service.
const STARTUPS: ServiceKey<BTreeMap<String, Arc<Startup>>> = ServiceKey::new("tests/startups");

/// The fixture Pack `slow`: the echo tool, offered once its startup
/// leaves [`Phase::Starting`], ready or failed, so only the Harness keeps
/// a failed one's tool out of a run. It is a `runvil` Pack, so every run
/// holds it, as every run holds [`SLOWER`].
const SLOW: &str = "runvil/slow";

/// A second [`Starting`] Pack beside [`SLOW`].
const SLOWER: &str = "runvil/slower";

/// A prompt declaring `slow` and calling its tool once by id.
const CALLS_SLOW: &str = "---\nname: calls-slow\ndescription: d\nrunvil: 0\n\
    uses:\n  - slow\n---\n\n\
    # Title\n\n## Only\n\n```lua\nreturn tools.call('slow/echo', { value = 'hi' })\n```\n";

/// A prompt declaring `slow` and calling nothing.
const DECLARES_SLOW: &str = "---\nname: declares-slow\ndescription: d\nrunvil: 0\n\
    uses:\n  - slow\n---\n\n# Title\n\n## Only\n\n```lua\nreturn 'ran'\n```\n";

/// A Pack-free prompt returning how many undeclared tools the run
/// offers.
const COUNTS_OFFERED: &str = "---\nname: counts\ndescription: d\nrunvil: 0\n---\n\n\
    # Title\n\n## Only\n\n```lua\n\
    local count = 0\n\
    for _, pack in ipairs(tools.packs.extras()) do count = count + #pack.tools end\n\
    return count .. ' offered'\n```\n";

/// A Pack whose tools wait on the test's [`Startup`].
struct Starting {
    echo: Arc<dyn Pack>,
    startup: Arc<Startup>,
}

fn construct_starting(
    name: &PackId,
    config: Value,
    services: &HostServices,
) -> Result<Arc<dyn Pack>, ToolError> {
    let startup = services
        .get(&STARTUPS)
        .and_then(|startups| startups.get(&name.to_string()).cloned())
        .ok_or_else(|| {
            ToolError::message(format!(
                "{name} needs its startup in tests/startups, and this host provides none"
            ))
        })?;
    Ok(Arc::new(Starting {
        echo: construct_echo(name, config, services)?,
        startup,
    }))
}

impl Pack for Starting {
    fn tools(&self) -> Vec<ToolInfo> {
        match *self.startup.phase.borrow() {
            Phase::Starting => Vec::new(),
            Phase::Ready | Phase::Failed(_) => self.echo.tools(),
        }
    }

    fn ready(&self) -> PackFuture<'_, Result<(), ToolError>> {
        let mut phase = self.startup.phase.clone();
        Box::pin(async move {
            self.startup.asked.notify_one();
            let settled = *phase
                .wait_for(|phase| !matches!(phase, Phase::Starting))
                .await
                .map_err(|_closed| ToolError::message("the test dropped the fixture's startup"))?;
            match settled {
                Phase::Failed(reason) => Err(ToolError::message(reason)),
                Phase::Starting | Phase::Ready => Ok(()),
            }
        })
    }

    fn call<'a>(
        &'a self,
        cx: ToolContext<'a>,
        args: Value,
    ) -> PackFuture<'a, Result<ToolOutput, ToolError>> {
        self.echo.call(cx, args)
    }
}

/// A Host that built a [`Starting`] Pack for each of `fqns`, in order,
/// every startup standing at `phase`, beside each Pack's handle in `fqns`
/// order.
fn starting_host(fqns: &[&'static str], phase: Phase) -> (HostContext, Vec<StartupHandle>) {
    let mut startups = BTreeMap::new();
    let mut handles = Vec::new();
    let mut infos = Vec::new();
    for fqn in fqns {
        let (moves, watched) = watch::channel(phase);
        let startup = Arc::new(Startup {
            phase: watched,
            asked: Notify::new(),
        });
        let (_vendor, id) = fqn.split_once('/').unwrap();
        startups.insert(id.to_owned(), Arc::clone(&startup));
        handles.push((moves, startup));
        infos.push(PackInfo::new(fqn, construct_starting));
    }
    let mut wide = HostServices::new();
    wide.provide(&STARTUPS, Arc::new(startups)).unwrap();
    (HostContext::new(registry(&infos), wide), handles)
}

/// A Host that built [`SLOW`], whose startup stands at `phase`, beside
/// the sender that moves it and the startup the Pack shares.
fn slow_host(phase: Phase) -> (HostContext, watch::Sender<Phase>, Arc<Startup>) {
    let (host, mut handles) = starting_host(&[SLOW], phase);
    let (moves, startup) = handles.pop().unwrap();
    (host, moves, startup)
}

/// A request to run `source` under the scripted broker's model.
fn request(source: &str) -> RunRequest {
    RunRequest {
        name: "session-1".to_owned(),
        source: source.to_owned(),
        args: String::new(),
        input_text: None,
        vfs: VfsRef::default(),
        host: HostSnapshot {
            selected_model: Some(MODEL.to_owned()),
            ..HostSnapshot::default()
        },
    }
}

/// A recorder over `inner` that raises the run's cancel as a run is
/// ended, before the end reaches `inner`: a cancel that lands after the
/// outcome is chosen and before it is recorded.
struct CancelsAtEnd {
    inner: Arc<MemoryRecorder>,
    control: OnceLock<RunControl>,
}

impl RunRecorder for CancelsAtEnd {
    fn begin_run(&self, meta: RunMeta) -> RecorderFuture<'_, RunId> {
        self.inner.begin_run(meta)
    }

    fn append(&self, run: RunId, record: Record) -> RecorderFuture<'_, ()> {
        self.inner.append(run, record)
    }

    fn end_run(&self, run: RunId, outcome: RunOutcome) -> RecorderFuture<'_, ()> {
        if let Some(control) = self.control.get() {
            control.cancel();
        }
        self.inner.end_run(run, outcome)
    }
}

#[tokio::test]
async fn a_pack_whose_ready_resolves_late_has_its_tools_in_the_run() {
    let (host, moves, startup) = slow_host(Phase::Starting);
    let recorder = recorder();
    let (prepared, ()) = tokio::time::timeout(PATIENCE, async {
        tokio::join!(prepare(CALLS_SLOW, "", services(&recorder, host)), async {
            startup.asked.notified().await;
            moves.send(Phase::Ready).unwrap();
        })
    })
    .await
    .expect("the Harness waits on the Pack's ready");
    let prepared = prepared.expect("the snapshot after the wait offers the Pack's tool");
    let outcome = drive_run(
        prepared.run,
        prepared.performers,
        recorder.clone(),
        prepared.run_id,
        CancelHandle::new(),
    )
    .await
    .unwrap();
    assert_eq!(completed(outcome), "hi");
}

#[tokio::test]
async fn every_starting_pack_is_asked_to_be_ready_before_any_of_them_answers() {
    let (host, handles) = starting_host(&[SLOW, SLOWER], Phase::Starting);
    let recorder = recorder();
    let (prepared, ()) = tokio::time::timeout(PATIENCE, async {
        tokio::join!(
            prepare(COUNTS_OFFERED, "", services(&recorder, host)),
            async {
                for (_, startup) in &handles {
                    startup.asked.notified().await;
                }
                for (moves, _) in &handles {
                    moves.send(Phase::Ready).unwrap();
                }
            }
        )
    })
    .await
    .expect(
        "the Harness asks both Packs before either is ready, so neither waits behind the other",
    );
    let prepared = prepared.expect("a Pack-free prompt is prepared");
    let outcome = drive_run(
        prepared.run,
        prepared.performers,
        recorder.clone(),
        prepared.run_id,
        CancelHandle::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        completed(outcome),
        "2 offered",
        "both Packs' tools reach the run once both are ready"
    );
}

#[tokio::test]
async fn a_declared_pack_whose_ready_fails_refuses_the_run_naming_its_reason() {
    let reason = "the fixture server refused the handshake";
    let (host, _moves, _startup) = slow_host(Phase::Failed(reason));
    let recorder = recorder();
    let error = prepare(DECLARES_SLOW, "", services(&recorder, host))
        .await
        .expect_err("a declared Pack that will not be ready refuses the run");

    let PrepareError::Refused { run_id, error } = error else {
        panic!("the refusal is a requirements refusal: {error}");
    };
    assert!(
        error
            .to_string()
            .contains(&format!("- slow is unavailable: {reason}")),
        "the notice names the Pack and its ready failure: {error}"
    );
    assert!(
        matches!(
            recorder.outcome(run_id),
            Some(RunOutcome::Failed { kind, .. }) if kind == "RequirementsUnmet"
        ),
        "the recorder holds the refusal as the run's failure"
    );
}

#[tokio::test]
async fn an_undeclared_pack_whose_ready_fails_leaves_the_run_going_without_its_tools() {
    let (host, _moves, _startup) = slow_host(Phase::Failed("the fixture server is down"));
    assert_eq!(
        completed(drive_over(COUNTS_OFFERED, host).await),
        "0 offered",
        "the failed Pack's listed tool stays out of the run's offering"
    );
}

#[tokio::test]
async fn an_undeclared_pack_whose_ready_fails_raises_its_reason_when_the_prompt_offers_it() {
    let offers_slow = "---\nname: offers-slow\ndescription: d\nrunvil: 0\n---\n\n\
        # Title\n\n## Only\n\n```lua\ntools.offer('slow')\nreturn 'offered'\n```\n";
    let (host, _moves, _startup) = slow_host(Phase::Failed("the fixture server is down"));
    let outcome = drive_over(offers_slow, host).await;
    let RunOutcome::Failed { message, .. } = &outcome else {
        panic!("offering an unavailable Pack fails the run: {outcome:?}");
    };
    assert!(
        message.contains(
            "tools.offer: the slow Pack is unavailable in this run: \
             the fixture server is down"
        ),
        "the failure names the Pack and its ready failure: {message}"
    );
}

#[tokio::test]
async fn an_undeclared_third_party_pack_that_never_gets_ready_holds_no_run_up() {
    let mut startups = BTreeMap::new();
    let (_moves, watched) = watch::channel(Phase::Starting);
    let startup = Arc::new(Startup {
        phase: watched,
        asked: Notify::new(),
    });
    startups.insert("stuck".to_owned(), startup);
    let mut wide = HostServices::new();
    wide.provide(&STARTUPS, Arc::new(startups)).unwrap();
    let stuck = PackInfo::new("tests/stuck", construct_starting);
    let host = HostContext::new(registry(&[stuck]), wide);
    let outcome = tokio::time::timeout(PATIENCE, drive_over(COUNTS_OFFERED, host))
        .await
        .expect("no run waits on a Pack it does not hold");
    assert_eq!(completed(outcome), "0 offered");
}

#[tokio::test]
async fn an_undeclared_builtin_with_no_tools_has_an_object_the_prompt_can_offer() {
    let reads_stub = "---\nname: reads-stub\ndescription: d\nrunvil: 0\n---\n\n\
        # Title\n\n## Only\n\n```lua\n\
        local stub = tools.packs.get('subagents')\n\
        tools.offer('subagents')\n\
        return stub.name .. ':' .. #stub.tools\n```\n";
    let host = HostContext::new(registry(&[]), HostServices::new());
    assert_eq!(
        completed(drive_over(reads_stub, host).await),
        "subagents:0",
        "every Host builds the stub and every run holds it, so Lua sees it and offers it"
    );
}

#[tokio::test]
async fn a_cancel_while_a_declared_pack_is_still_starting_ends_the_run_cancelled_in_report_and_record()
 {
    let (host, _moves, startup) = slow_host(Phase::Starting);
    let recorder = recorder();
    let harness = Harness::new(
        recorder.clone(),
        Arc::new(ScriptedBroker::replying()),
        Arc::new(Unused),
        Arc::new(host),
        HostServices::new(),
    );
    let control = harness.control();
    let (report, ()) = tokio::time::timeout(PATIENCE, async {
        tokio::join!(harness.run(request(CALLS_SLOW)), async {
            startup.asked.notified().await;
            control.cancel();
        })
    })
    .await
    .expect("the cancel ends the wait");

    let report = report.expect("a cancelled run is reported");
    assert_eq!(report.outcome, RunOutcome::Cancelled);
    assert_eq!(report.output, Err(OutputError::NotCompleted));
    let run_id = report
        .run_id
        .expect("the run began at the recorder before the wait");
    assert_eq!(
        recorder.outcome(run_id),
        Some(RunOutcome::Cancelled),
        "the recorder holds the cancel, not a refusal of the unready Pack"
    );
}

#[tokio::test]
async fn a_cancel_that_lands_while_a_refusal_is_recorded_leaves_the_report_holding_the_recorded_failure()
 {
    let inner = recorder();
    let recorder = Arc::new(CancelsAtEnd {
        inner: inner.clone(),
        control: OnceLock::new(),
    });
    let harness = Harness::new(
        recorder.clone(),
        Arc::new(ScriptedBroker::replying()),
        Arc::new(Unused),
        Arc::new(bare()),
        HostServices::new(),
    );
    recorder.control.set(harness.control()).unwrap();

    let report = tokio::time::timeout(PATIENCE, harness.run(request(NEEDS_WEB)))
        .await
        .expect("a refused run ends at once")
        .expect("a refused run is reported");
    let run_id = report
        .run_id
        .expect("the run began at the recorder before the refusal");
    assert!(
        matches!(&report.outcome, RunOutcome::Failed { kind, .. } if kind == "RequirementsUnmet"),
        "the refusal was decided before the cancel landed: {:?}",
        report.outcome
    );
    assert_eq!(
        inner.outcome(run_id),
        Some(report.outcome),
        "the recorder holds the report's outcome"
    );
}

#[tokio::test]
async fn a_pack_that_keeps_the_default_ready_is_ready_at_once_and_serves_the_run() {
    let echo = construct_echo(
        &PackId::parse("tools").unwrap(),
        Value::Null,
        &HostServices::new(),
    )
    .unwrap();
    assert!(
        matches!(echo.ready().now_or_never(), Some(Ok(()))),
        "the default ready resolves Ok on its first poll"
    );
    assert_eq!(
        completed(drive_over(CALLS_ECHO, building(TOOLS)).await),
        "hi",
        "its tools reach the run as before"
    );
}
