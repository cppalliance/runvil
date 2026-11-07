//! The per-run Harness: one object per run of a parsed prompt, which
//! drives that one run as a single future.
//!
//! A launcher builds a [`Harness`] from the Host it shares behind an
//! `Arc`, the prompt `Host::parse` returned, and the run's own
//! [`RunSetup`], takes the run's [`RunControl`], and awaits
//! [`Harness::run`] on whatever executor it likes. The run begins its
//! record, resolves the run's model through the setup's broker, prepares
//! the prompt, drives the run through the effect loop, and reads the
//! declared output file of a completed run. Every performer future is
//! polled inside that one future, and every VFS operation runs inline in
//! it, so the Harness starts no task and names no runtime. The stop and
//! the cancel a [`RunControl`] raises are plain flags any thread can set,
//! and the run's effect loop is their one waiter.

use std::fmt;
use std::pin::pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};

use futures_util::future::{Either, select};
use futures_util::task::AtomicWaker;
use runvil::cancel::CancelHandle;
use runvil::model::ModelInfo;
use runvil::{Error, ErrorCode, ErrorFact, Prompt};

use crate::effect_loop::drive;
use crate::environment::current_model;
use crate::files::{make_not_completed_error, report_output};
use crate::host::Host;
use crate::performers::InferenceBroker;
use crate::prepare::{begin, prepare};
use crate::recorder::{RunId, is_refusal};
use crate::setup::RunSetup;

/// Drives one run of a prompt for a Host.
///
/// It holds what the run needs: the Host it shares with every other run,
/// the parsed prompt, and the run's own setup. [`Harness::run`] consumes
/// it.
pub struct Harness {
    host: Arc<Host>,
    prompt: Prompt,
    setup: RunSetup,
    control: RunControl,
}

impl fmt::Debug for Harness {
    /// Shows the Host, the prompt's file, the setup, and the control.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Harness")
            .field("host", &self.host)
            .field("prompt", &self.prompt.file())
            .field("setup", &self.setup)
            .field("control", &self.control)
            .finish()
    }
}

/// How one run ended, and what it left at its declared output file.
///
/// The recorder holds the same result under the same run id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunReport {
    /// The id the recorder issued for the run.
    pub run_id: RunId,
    /// How the run ended: its final text, or the error that ended it. A
    /// cancel is the error whose code is `canceled`.
    pub result: Result<String, Error>,
    /// The text a completed run left at its declared `output:` file, or
    /// the reason the report omits that text: `NoOutput` with the `Reason`
    /// fact `make_not_completed_error` or `undeclared`, `NotFound` for a completed
    /// run that left no file, or the store's failure, each with the
    /// declared path as the `Path` fact when there is one.
    pub output: Result<String, runvil::Error>,
}

/// The failure that stops a run before `Harness::run` can record its
/// result: the recorder refused a write for run `run`, or refused to
/// begin it when `run` is `None`.
///
/// It is `RecorderFailed`, with the refusal after `the run could not be
/// recorded: ` in its message, the refusal's facts, and the run's `Run`
/// fact when the recorder issued one. Every other way a run ends is a
/// recorded result in a [`RunReport`].
fn unrecorded(run: Option<RunId>, refusal: &Error) -> Error {
    let code = ErrorCode::RecorderFailed;
    let mut error =
        Error::new(code, "the run could not be recorded", code.prose()).with_cause(refusal);
    error.facts.clone_from(&refusal.facts);
    if let Some(run) = run {
        error.facts.insert(ErrorFact::Run, run.get().into());
    }
    error
}

impl Harness {
    /// A Harness for one run of `prompt` on `host`, with `setup` as the
    /// run's own inputs.
    ///
    /// The run is recorded through `setup.recorder`. It resolves its model
    /// and gets the model's replies through `setup.broker`, and it reads
    /// its start from `host.timer` and sleeps through it. It runs under
    /// `setup.limits`. Its tool calls go to the Packs of `host.packs` that
    /// the prompt picks, and each call is lent `setup.services`. A run has
    /// at most `setup.max_tool_calls_in_flight` tool calls in flight; the
    /// rest wait and start oldest first. All of this work happens in
    /// [`Harness::run`].
    #[must_use]
    pub fn new(host: Arc<Host>, prompt: Prompt, setup: RunSetup) -> Harness {
        Harness {
            host,
            prompt,
            setup,
            control: RunControl::new(CancelHandle::new()),
        }
    }

    /// Returns the control that steers this Harness's run from outside its
    /// future.
    ///
    /// Take it before calling [`Harness::run`], which consumes the Harness.
    #[must_use]
    pub fn control(&self) -> RunControl {
        self.control.clone()
    }

    /// Runs the prompt to its end and reports how it ended; the only error
    /// is `RecorderFailed`.
    ///
    /// It begins the run's record, resolves the run's model through the
    /// setup's broker, puts the prompt's declared input in place, waits
    /// for the Host's Packs to be ready, drives the run, and reads the
    /// declared output file if the run completed. The returned future is
    /// `Send`. Its only runtime needs are those of the Host's performers
    /// and Packs.
    ///
    /// Every run gets a record and ends with its result, which the report
    /// carries beside the run's id: the final text, or the error that
    /// ended the run, where a cancel is the error whose code is
    /// `canceled`. The record begins before anything else, so a run that
    /// ends before it starts is recorded too, and it holds the run's own
    /// records only, never the prompt's parse. A cancel raised before
    /// calling `run`, or while the broker lists its models, ends the run
    /// canceled. A model list the broker fails to fetch ends it with the
    /// broker's failure, and a list that omits the selected model with a
    /// `RequirementsUnmet` failure. A declared input file the Harness
    /// fails to put in place ends it with the input's failure, and an
    /// environment that falls short of the prompt's requirements, which
    /// the run's first step finds, with a `RequirementsUnmet` failure. An
    /// environment that falls short after a cancel is raised, as when the
    /// cancel cuts short the wait for a Pack that is still starting, ends
    /// the run canceled instead. A run that is pending with nothing to
    /// await, which means the Harness lost track of an effect, ends with
    /// an `Internal` error.
    ///
    /// # Errors
    /// Returns `RecorderFailed` when the recorder refuses a write. That is
    /// the only error: the run is then not recorded to its end. Its
    /// message holds the refusal after `the run could not be recorded: `,
    /// and its `Run` fact is the run the recorder issued, absent when the
    /// recorder refused to begin the run.
    pub async fn run(self) -> Result<RunReport, Error> {
        Box::pin(self.run_to_end()).await
    }

    async fn run_to_end(self) -> Result<RunReport, Error> {
        let Harness {
            host,
            prompt,
            setup,
            control,
        } = self;
        let cancel = control.cancel_handle();

        let (run_id, params) = begin(&setup, host.timer.now())
            .await
            .map_err(|refusal| unrecorded(None, &refusal))?;
        let recorded = |refusal: Error| unrecorded(Some(run_id), &refusal);

        let selected = setup.selected_model.as_deref();
        let model = match resolve_model(selected, &*setup.broker, cancel).await {
            Ok(model) => model,
            Err(error) => {
                setup
                    .recorder
                    .end_run(run_id, Err(error.clone()))
                    .await
                    .map_err(recorded)?;
                return Ok(RunReport {
                    run_id,
                    result: Err(error),
                    output: Err(make_not_completed_error()),
                });
            }
        };

        let output_path = prompt
            .frontmatter()
            .output()
            .map(|decl| decl.path().to_owned());
        let prepared = prepare(&host, prompt, &setup, run_id, params, model, cancel).await;
        let (run, packs) = match prepared {
            Ok(prepared) => prepared,
            Err(refusal) if is_refusal(&refusal) => return Err(recorded(refusal)),
            Err(error) => {
                return Ok(RunReport {
                    run_id,
                    result: Err(error),
                    output: Err(make_not_completed_error()),
                });
            }
        };

        let result = match drive(
            run,
            &setup,
            Arc::clone(&host.timer),
            Arc::new(packs),
            run_id,
            cancel.clone(),
            Arc::clone(control.stop_signal()),
        )
        .await
        {
            Err(refusal) if is_refusal(&refusal) => return Err(recorded(refusal)),
            result => result,
        };
        let output = report_output(&setup.vfs, output_path.as_deref(), &result);
        Ok(RunReport {
            run_id,
            result,
            output,
        })
    }
}

/// A handle that drops a run's in-flight round or cancels the run, from
/// outside the future that drives it.
///
/// [`stop_round`](Self::stop_round) drops the work in flight, and the run
/// goes on. [`cancel`](Self::cancel) ends the run.
///
/// Cloning is cheap, and every clone controls the same run. Take one from
/// [`Harness::control`](crate::Harness::control) before you call
/// [`Harness::run`](crate::Harness::run), because `run` consumes the
/// Harness.
#[derive(Clone)]
pub struct RunControl {
    cancel: CancelHandle,
    stop: Arc<StopSignal>,
}

impl RunControl {
    /// The control over a run whose Engine cancel flag is `cancel`.
    fn new(cancel: CancelHandle) -> Self {
        Self {
            cancel,
            stop: Arc::default(),
        }
    }

    /// Drops the effects in flight, except tool calls that survive stops,
    /// and lets the run go on.
    ///
    /// A call survives a stop when its tool's info sets
    /// `survives_stop`, as the question to the operator does; for a tool
    /// only the model sees, that is the info its section's render gave it.
    /// Every other effect in flight is aborted and answered `Dropped`. The
    /// run's cancel flag stays clear.
    /// A `pcall` around a dropped call catches its canceled error.
    /// Otherwise, the error ends the run canceled.
    ///
    /// A stop also drops the tool calls waiting for a slot, unstarted,
    /// except those that survive stops, which start once a slot frees.
    ///
    /// A stop reaches only the effects in flight or waiting when the loop
    /// that drives the run sees it. The loop looks for a stop while it
    /// waits for an answer and before it starts a step's effects. It drops
    /// the effects the stop reaches, if any, then lowers the stop.
    pub fn stop_round(&self) {
        self.stop.raise();
    }

    /// Cancels the run.
    ///
    /// Every effect in flight, tool calls that survive stops included, is
    /// answered `Dropped`, and the run ends canceled. Every tool call
    /// waiting for a slot is answered `Dropped` too, unstarted. A cancel
    /// raised before the run begins still gives the run a record, which
    /// ends canceled before the run starts. Calling `cancel` more
    /// than once is the same as calling it once.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// The run's Engine cancel flag.
    fn cancel_handle(&self) -> &CancelHandle {
        &self.cancel
    }

    /// The run's stop signal.
    fn stop_signal(&self) -> &Arc<StopSignal> {
        &self.stop
    }
}

impl fmt::Debug for RunControl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RunControl")
            .field("canceled", &self.cancel.is_canceled())
            .field("stop_raised", &self.stop.raised.load(Ordering::SeqCst))
            .finish()
    }
}

/// A stop raised from any thread and lowered by the run's effect loop once
/// it has dropped what the stop reached.
#[derive(Default)]
pub(crate) struct StopSignal {
    raised: AtomicBool,
    waker: AtomicWaker,
}

impl StopSignal {
    /// Raises the stop and wakes the loop waiting on it.
    fn raise(&self) {
        self.raised.store(true, Ordering::SeqCst);
        self.waker.wake();
    }

    /// Whether a stop is raised.
    pub(crate) fn is_raised(&self) -> bool {
        self.raised.load(Ordering::SeqCst)
    }

    /// Ready while a stop is raised, or registers `cx` to be woken by the
    /// next one. The flag is checked again after the registration, so a
    /// stop raised between the two is never lost.
    pub(crate) fn poll_raised(&self, cx: &mut Context<'_>) -> Poll<()> {
        if self.is_raised() {
            return Poll::Ready(());
        }
        self.waker.register(cx.waker());
        if self.is_raised() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }

    /// Lowers the stop. A stop raised while the loop was dropping what an
    /// earlier one reached is lowered with it, since nothing it could
    /// reach is left in flight.
    pub(crate) fn lower(&self) {
        self.raised.store(false, Ordering::SeqCst);
    }
}

/// Resolves the run's model, the one `selected` names, through `broker`,
/// or returns the error that ends the run instead: the cancel when
/// `cancel` fires first, and the lookup's failure when the broker's list
/// cannot be fetched or lacks the selection.
async fn resolve_model(
    selected: Option<&str>,
    broker: &dyn InferenceBroker,
    cancel: &CancelHandle,
) -> Result<Option<ModelInfo>, Error> {
    let resolving = pin!(current_model(selected, broker));
    // The cancel arm is polled first, so a cancel raised before the run,
    // or before a listing that is ready at once, wins.
    match select(cancel.canceled(), resolving).await {
        Either::Left(((), _)) => Err(Error::canceled()),
        Either::Right((resolved, _)) => resolved,
    }
}
