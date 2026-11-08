//! Readiness at preparation: the Harness waits for the `ready` of each of
//! the run's Packs before the run's snapshot, asking each one before any
//! answers, so a Pack still starting has its tools in the run once it is
//! ready, a Pack whose `ready` fails is unavailable to that run and
//! offering it raises its reason, an undeclared third-party Pack that
//! never gets ready holds no run up, a built Pack with no tools still
//! reaches the prompt's Lua, a cancel during the wait ends the run as
//! canceled in both the report and the record, even when the run's first
//! step would refuse it, a cancel that lands while a refusal is recorded
//! leaves the report holding what the record holds, and a Pack that keeps
//! the default `ready` serves as before.

use super::*;

use std::sync::OnceLock;
use std::time::Duration;

use futures_util::FutureExt as _;
use harness_runner::RunControl;
use runvil_plugin::PackFactory;
use tokio::sync::{Notify, watch};

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

/// The factory of a [`Starting`] Pack over `startup`, which the test
/// drives.
fn starting(startup: Arc<Startup>) -> impl PackFactory {
    move |name: &PackId, run: &HostServices| -> Result<Arc<dyn Pack>, Error> {
        Ok(Arc::new(Starting {
            echo: new_echo(name, run)?,
            startup: Arc::clone(&startup),
        }))
    }
}

impl Pack for Starting {
    fn tools(&self) -> Vec<ToolInfo> {
        match *self.startup.phase.borrow() {
            Phase::Starting => Vec::new(),
            Phase::Ready | Phase::Failed(_) => self.echo.tools(),
        }
    }

    fn ready(&self) -> PackFuture<'_, Result<(), Error>> {
        let mut phase = self.startup.phase.clone();
        Box::pin(async move {
            self.startup.asked.notify_one();
            let settled = *phase
                .wait_for(|phase| !matches!(phase, Phase::Starting))
                .await
                .map_err(|_closed| {
                    make_tool_error(
                        ErrorCode::ToolOther,
                        "the test dropped the fixture's startup",
                    )
                })?;
            match settled {
                Phase::Failed(reason) => Err(make_tool_error(ErrorCode::ToolOther, reason)),
                Phase::Starting | Phase::Ready => Ok(()),
            }
        })
    }

    fn call<'a>(
        &'a self,
        cx: ToolContext<'a>,
        args: Value,
    ) -> PackFuture<'a, Result<ToolOutput, Error>> {
        self.echo.call(cx, args)
    }
}

/// A Host that built a [`Starting`] Pack for each of `fqns`, in order,
/// every startup standing at `phase`, beside each Pack's handle in `fqns`
/// order.
fn starting_host(fqns: &[&'static str], phase: Phase) -> (Packs, Vec<StartupHandle>) {
    let mut packs = registry(&[]);
    let mut handles = Vec::new();
    for &fqn in fqns {
        let (moves, watched) = watch::channel(phase);
        let startup = Arc::new(Startup {
            phase: watched,
            asked: Notify::new(),
        });
        packs
            .insert(PackInfo::new(fqn), starting(Arc::clone(&startup)))
            .unwrap();
        handles.push((moves, startup));
    }
    (Packs::new(packs), handles)
}

/// A Host that built [`SLOW`], whose startup stands at `phase`, beside
/// the sender that moves it and the startup the Pack shares.
fn slow_host(phase: Phase) -> (Packs, watch::Sender<Phase>, Arc<Startup>) {
    let (host, mut handles) = starting_host(&[SLOW], phase);
    let (moves, startup) = handles.pop().unwrap();
    (host, moves, startup)
}

/// A recorder over `inner` that raises the run's cancel as a run is
/// ended, before the end reaches `inner`: a cancel that lands after the
/// result is chosen and before it is recorded.
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

    fn end_run(&self, run: RunId, result: Result<String, Error>) -> RecorderFuture<'_, ()> {
        if let Some(control) = self.control.get() {
            control.cancel();
        }
        self.inner.end_run(run, result)
    }
}

#[tokio::test]
async fn a_pack_whose_ready_resolves_late_has_its_tools_in_the_run() {
    let (host, moves, startup) = slow_host(Phase::Starting);
    let recorder = recorder();
    let (report, ()) = tokio::time::timeout(PATIENCE, async {
        tokio::join!(report_of(CALLS_SLOW, host, setup_over(&recorder)), async {
            startup.asked.notified().await;
            moves.send(Phase::Ready).unwrap();
        })
    })
    .await
    .expect("the Harness waits on the Pack's ready");
    assert_eq!(
        completed(report.result),
        "hi",
        "the snapshot after the wait offers the Pack's tool"
    );
}

#[tokio::test]
async fn every_starting_pack_is_asked_to_be_ready_before_any_of_them_answers() {
    let (host, handles) = starting_host(&[SLOW, SLOWER], Phase::Starting);
    let recorder = recorder();
    let (report, ()) = tokio::time::timeout(PATIENCE, async {
        tokio::join!(
            report_of(COUNTS_OFFERED, host, setup_over(&recorder)),
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
    assert_eq!(
        completed(report.result),
        "2 offered",
        "both Packs' tools reach the run once both are ready"
    );
}

#[tokio::test]
async fn a_declared_pack_whose_ready_fails_refuses_the_run_naming_its_reason() {
    let reason = "the fixture server refused the handshake";
    let (host, _moves, _startup) = slow_host(Phase::Failed(reason));
    let recorder = recorder();
    let (run_id, result) = prepare_and_drive(DECLARES_SLOW, host, setup_over(&recorder)).await;
    assert_eq!(
        recorder.result(run_id).as_ref(),
        Some(&result),
        "the recorder holds the refusal as the run's failure"
    );
    let notice = refusal(result);
    assert!(
        notice.contains(&format!("- slow is unavailable: {reason}")),
        "the notice names the Pack and its ready failure: {notice}"
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
    let message = drive_over(offers_slow, host)
        .await
        .expect_err("offering an unavailable Pack fails the run")
        .located_message();
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
    let (_moves, watched) = watch::channel(Phase::Starting);
    let startup = Arc::new(Startup {
        phase: watched,
        asked: Notify::new(),
    });
    let mut packs = registry(&[]);
    packs
        .insert(PackInfo::new("tests/stuck"), starting(startup))
        .unwrap();
    let host = Packs::new(packs);
    let result = tokio::time::timeout(PATIENCE, drive_over(COUNTS_OFFERED, host))
        .await
        .expect("no run waits on a Pack it does not hold");
    assert_eq!(completed(result), "0 offered");
}

#[tokio::test]
async fn an_undeclared_builtin_with_no_tools_has_an_object_the_prompt_can_offer() {
    let reads_stub = "---\nname: reads-stub\ndescription: d\nrunvil: 0\n---\n\n\
        # Title\n\n## Only\n\n```lua\n\
        local stub = tools.packs.get('subagents')\n\
        tools.offer('subagents')\n\
        return stub.name .. ':' .. #stub.tools\n```\n";
    let host = Packs::new(registry(&[]));
    assert_eq!(
        completed(drive_over(reads_stub, host).await),
        "subagents:0",
        "every Host builds the stub and every run holds it, so Lua sees it and offers it"
    );
}

#[tokio::test]
async fn a_cancel_while_a_declared_pack_is_still_starting_ends_the_run_canceled_in_report_and_record()
 {
    let (host, _moves, startup) = slow_host(Phase::Starting);
    let recorder = recorder();
    let harness = launch(host, Unused, CALLS_SLOW, setup_over(&recorder));
    let control = harness.control();
    let (report, ()) = tokio::time::timeout(PATIENCE, async {
        tokio::join!(harness.run(), async {
            startup.asked.notified().await;
            control.cancel();
        })
    })
    .await
    .expect("the cancel ends the wait");

    let report = report.expect("a canceled run is reported");
    crate::support::assert_canceled(&report.result);
    crate::support::assert_not_completed(&report.output);
    assert_eq!(
        recorder.result(report.run_id),
        Some(report.result),
        "the recorder holds the cancel, not a refusal of the unready Pack"
    );
}

#[tokio::test]
async fn a_refusal_after_a_cancel_cut_the_wait_short_ends_the_run_canceled_with_no_run_events() {
    let (host, _moves, startup) = slow_host(Phase::Starting);
    let recorder = recorder();
    let declares_slow_and_web = "---\nname: slow-and-web\ndescription: d\nrunvil: 0\n\
        uses:\n  - slow\n  - web\n---\n\n# Title\n\n## Only\n\nDone.\n";
    let harness = launch(host, Unused, declares_slow_and_web, setup_over(&recorder));
    let control = harness.control();
    let (report, ()) = tokio::time::timeout(PATIENCE, async {
        tokio::join!(harness.run(), async {
            startup.asked.notified().await;
            control.cancel();
        })
    })
    .await
    .expect("the cancel ends the wait");

    let report = report.expect("a canceled run is reported");
    assert!(
        report.result.as_ref().is_err_and(Error::is_canceled),
        "the missing web Pack would refuse the run, but the cancel came first: {:?}",
        report.result
    );
    let run_id = report.run_id;
    assert_eq!(recorder.result(run_id), Some(report.result));
    assert_no_records(&recorder, run_id);
}

#[tokio::test]
async fn a_cancel_that_lands_while_a_refusal_is_recorded_leaves_the_report_holding_the_recorded_failure()
 {
    let inner = recorder();
    let recorder = Arc::new(CancelsAtEnd {
        inner: inner.clone(),
        control: OnceLock::new(),
    });
    let setup = RunSetup::new(recorder.clone(), Arc::new(ScriptedBroker::replying()));
    let harness = launch(bare(), Unused, NEEDS_WEB, setup);
    recorder.control.set(harness.control()).unwrap();

    let report = tokio::time::timeout(PATIENCE, harness.run())
        .await
        .expect("a refused run ends at once")
        .expect("a refused run is reported");
    let run_id = report.run_id;
    assert_eq!(
        report.result.as_ref().map_err(|error| error.code),
        Err(ErrorCode::RequirementsUnmet),
        "the refusal was decided before the cancel landed"
    );
    assert_eq!(
        inner.result(run_id),
        Some(report.result),
        "the recorder holds the report's result"
    );
}

#[tokio::test]
async fn a_pack_that_keeps_the_default_ready_is_ready_at_once_and_serves_the_run() {
    let echo = new_echo(&PackId::parse("tools").unwrap(), &HostServices::new()).unwrap();
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
