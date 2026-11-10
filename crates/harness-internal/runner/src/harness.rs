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
//! the messages it sends wait in a queue any thread can fill, and the
//! run's effect loop is their one waiter.

use std::fmt;
use std::pin::pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll};

use futures_util::future::{Either, select};
use futures_util::task::AtomicWaker;
use runvil_engine::cancel::CancelHandle;
use runvil_engine::model::ModelInfo;
use runvil_engine::{Error, ErrorCode, ErrorFact, Prompt};

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
    /// Ends the run's send queue when it drops: with the Harness, with the
    /// future `run` returns, polled or not, or as `run` returns, a refusal
    /// included.
    ended: EndOnDrop,
}

impl fmt::Debug for Harness {
    /// Shows the Host, the prompt's file, the setup, and the control.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Harness")
            .field("host", &self.host)
            .field("prompt", &self.prompt.file())
            .field("setup", &self.setup)
            .field("control", &self.control)
            .finish_non_exhaustive()
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
    /// fact `not_completed` or `undeclared`, `NotFound` for a completed
    /// run that left no file, or the store's failure, each with the
    /// declared path as the `Path` fact when there is one.
    pub output: Result<String, runvil_engine::Error>,
    /// Messages sent to the main chain that the run
    /// never read, oldest first.
    pub unread: Vec<String>,
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
    /// `setup.limits`. It makes its own instance of each Pack of
    /// `host.packs` that the prompt picks, and its tool calls go to those
    /// instances, which drop when the run ends; when `setup.has_user` is
    /// false, it holds `runvil/user` unavailable instead of making it. A
    /// run has at most `setup.max_tool_calls_in_flight` tool calls in
    /// flight; the rest wait and start oldest first. All of this work
    /// happens in [`Harness::run`].
    #[must_use]
    pub fn new(host: Arc<Host>, prompt: Prompt, setup: RunSetup) -> Harness {
        let control = RunControl::new(CancelHandle::new());
        Harness {
            host,
            prompt,
            setup,
            ended: EndOnDrop(Arc::clone(control.send_queue())),
            control,
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
    /// setup's broker, puts the prompt's declared input in place, makes
    /// the run's Packs and waits for them to be ready, drives the run, and
    /// reads the declared output file if the run completed. The run's
    /// Pack instances drop before it returns. The returned future is
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
    /// an `Internal` error. A run whose main chain waits on its inbox with
    /// nothing else to run is not stalled: it waits for a
    /// [`RunControl::send`] or a cancel.
    ///
    /// The report's `unread` holds the messages sent to the main chain
    /// that the run never read, oldest first: the ones still in its inbox
    /// when it ended, then the ones the Harness accepted and never
    /// delivered, which for a run that ended before it started is every
    /// message sent to it.
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
            ended: _ended,
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
                    unread: control.send_queue().end(),
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
                    unread: control.send_queue().end(),
                });
            }
        };

        let (result, unread) = drive(
            run,
            &setup,
            Arc::clone(&host.timer),
            Arc::new(packs),
            run_id,
            &control,
        )
        .await;
        let result = match result {
            Err(refusal) if is_refusal(&refusal) => return Err(recorded(refusal)),
            result => result,
        };
        let output = report_output(&setup.vfs, output_path.as_deref(), &result);
        Ok(RunReport {
            run_id,
            result,
            output,
            unread,
        })
    }
}

/// Ends a run's [`SendQueue`] when dropped, discarding what it still
/// holds.
struct EndOnDrop(Arc<SendQueue>);

impl Drop for EndOnDrop {
    fn drop(&mut self) {
        self.0.end();
    }
}

/// A handle that sends a run's main chain a message, drops the run's
/// in-flight round, or cancels the run, from outside the future that
/// drives it.
///
/// [`send`](Self::send) hands the main chain a message and
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
    sends: Arc<SendQueue>,
}

impl RunControl {
    /// The control over a run whose Engine cancel flag is `cancel`, with
    /// no stop raised and no message sent.
    pub(crate) fn new(cancel: CancelHandle) -> Self {
        Self {
            cancel,
            stop: Arc::default(),
            sends: Arc::default(),
        }
    }

    /// Sends `text` to the run's main-chain inbox and returns true, or
    /// returns false, dropping the text, once the run has ended or its
    /// Harness, or the future `Harness::run` returned, has been dropped.
    /// Text sent before the run starts is delivered before its first step.
    /// True means accepted, not read: text the run never reads comes back
    /// on `RunReport::unread`.
    ///
    /// Once the run has started, the loop that drives it delivers each
    /// message the next time it waits, oldest first, and it handles a stop
    /// raised before a send first. So a Host that stops and then sends
    /// always gets the stop: the round in flight is dropped, and the
    /// message waits for the run's next read of its inbox instead of
    /// steering that round.
    #[must_use]
    pub fn send(&self, text: String) -> bool {
        self.sends.push(text)
    }

    /// Drops the effects in flight, except tool calls that survive stops,
    /// and lets the run go on.
    ///
    /// A call survives a stop when its tool's info sets
    /// `survives_stop`; for a tool only the model sees, that is the info
    /// its section's render gave it. A chain waiting on its inbox has
    /// nothing in flight, so a stop never reaches its wait.
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
    /// waits for an answer, before each step, and before it starts a
    /// step's effects. It drops the effects the stop reaches, if any, then
    /// lowers the stop.
    ///
    /// A message that arrives while a model round has no answer withdraws
    /// the round, and the run reissues it with the message appended. A
    /// stop the loop first sees after the step that reissued the round,
    /// before the reissued round starts, drops that round too, unstarted:
    /// it is the request the stop meant.
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
    pub(crate) fn cancel_handle(&self) -> &CancelHandle {
        &self.cancel
    }

    /// The run's stop signal.
    pub(crate) fn stop_signal(&self) -> &Arc<StopSignal> {
        &self.stop
    }

    /// The messages sent to the run that its effect loop has not
    /// delivered yet.
    pub(crate) fn send_queue(&self) -> &Arc<SendQueue> {
        &self.sends
    }
}

impl fmt::Debug for RunControl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RunControl")
            .field("canceled", &self.cancel.is_canceled())
            .field("stop_raised", &self.stop.raised.load(Ordering::SeqCst))
            .field("sends", &self.sends)
            .finish()
    }
}

/// The messages a Host sent through [`RunControl::send`] that the run's
/// effect loop has not delivered yet, oldest first, and whether the run
/// has ended. Any thread can send, and the effect loop is the one taker.
#[derive(Default)]
pub(crate) struct SendQueue {
    sends: Mutex<Sends>,
    waker: AtomicWaker,
}

/// What a [`SendQueue`] guards.
#[derive(Default)]
struct Sends {
    /// The messages accepted and not yet delivered, oldest first.
    queued: Vec<String>,
    /// The run has ended, so every send is refused.
    ended: bool,
}

impl SendQueue {
    /// Queues `text` and wakes the loop waiting on the queue, or refuses it
    /// once the run has ended.
    fn push(&self, text: String) -> bool {
        {
            let mut sends = self.lock();
            if sends.ended {
                return false;
            }
            sends.queued.push(text);
        }
        self.waker.wake();
        true
    }

    /// Takes every queued message, oldest first, without waiting.
    pub(crate) fn take(&self) -> Vec<String> {
        std::mem::take(&mut self.lock().queued)
    }

    /// Ready with every queued message, oldest first, or registers `cx` to
    /// be woken by the next send. The queue is read after the
    /// registration, so a message sent between the two is never lost.
    pub(crate) fn poll_sent(&self, cx: &mut Context<'_>) -> Poll<Vec<String>> {
        self.waker.register(cx.waker());
        let sent = self.take();
        if sent.is_empty() {
            Poll::Pending
        } else {
            Poll::Ready(sent)
        }
    }

    /// Ends the queue as the run ends, so every later send is refused, and
    /// returns the messages it accepted and never delivered, oldest first.
    pub(crate) fn end(&self) -> Vec<String> {
        let mut sends = self.lock();
        sends.ended = true;
        std::mem::take(&mut sends.queued)
    }

    fn lock(&self) -> MutexGuard<'_, Sends> {
        // Every writer leaves the queue whole between its pushes and takes,
        // so a poisoned lock still holds a usable value.
        self.sends.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl fmt::Debug for SendQueue {
    /// Shows how many messages wait and whether the run has ended, never
    /// their text.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sends = self.lock();
        f.debug_struct("SendQueue")
            .field("queued", &sends.queued.len())
            .field("ended", &sends.ended)
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
