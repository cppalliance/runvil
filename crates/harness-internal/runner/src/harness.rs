//! The per-run Harness: one object per run over everything the Host
//! supplies, which drives that one run as a single future.
//!
//! A Host builds a [`Harness`] from its recorder, inference broker, timer,
//! built Packs, and the run's services, takes the run's [`RunControl`], and
//! awaits [`Harness::run`] on whatever executor it likes. The run begins
//! its record, resolves the launch model through the broker, prepares the
//! request's source, drives the run through the effect loop, and reads the
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
use runvil::vfs::VfsRef;
use runvil::{Error, ErrorCode, ErrorFact};
use runvil_plugin::HostServices;

use crate::effect_loop::drive;
use crate::environment::{HostSnapshot, current_model};
use crate::files::{make_not_completed_error, report_output};
use crate::packs::Packs;
use crate::performers::{InferenceBroker, Timer};
use crate::prepare::{Services, begin, prepare};
use crate::recorder::{RunId, RunRecorder, is_refusal};

/// Drives one run of a prompt for a Host.
///
/// It holds what the Host supplies for the run: a recorder, an inference
/// broker, a timer, the Host's built Packs, and the run's own
/// services. [`Harness::run`] consumes it.
pub struct Harness {
    recorder: Arc<dyn RunRecorder>,
    broker: Arc<dyn InferenceBroker>,
    timer: Arc<dyn Timer>,
    host: Arc<Packs>,
    services: HostServices,
    control: RunControl,
}

impl fmt::Debug for Harness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Harness")
            .field("host", &self.host)
            .field("services", &self.services)
            .field("control", &self.control)
            .finish_non_exhaustive()
    }
}

/// The inputs to one run: the prompt's source and arguments, its declared
/// input text, the filesystem it works in, and the Host state it reads.
#[derive(Debug, Clone)]
pub struct RunRequest {
    /// The run's name. It becomes the `name` field of the run metadata.
    pub name: String,
    /// The prompt's Markdown source. The Harness reads the prompt only
    /// from this text.
    pub source: String,
    /// The file the source came from, as the Host names it, such as
    /// `agents/research.md`, or `prompt` when it came from no file. The
    /// run's failures name it, so a reader can open the file.
    pub file: String,
    /// The run's argument text.
    pub args: String,
    /// Text written to the prompt's declared `input:` file before the run
    /// starts.
    pub input_text: Option<String>,
    /// The run's whole filesystem: the store that holds the prompt's
    /// declared files, plus any real directories mounted into it.
    pub vfs: VfsRef,
    /// The Host state the run reads: its selected model and the workspace
    /// roots it has granted. The run resolves its model from the
    /// selection, and the prompt's `ui()` global serves this state.
    pub host: HostSnapshot,
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
    /// Creates a Harness for one run.
    ///
    /// The Harness records the run through `recorder`. It resolves the
    /// run's model and gets the model's replies through `broker`. It sleeps
    /// through `timer`. Its tool calls go to the Packs of `host` that the
    /// run's prompt picks, and each call is lent `services`, the run's
    /// own, such as its input broker. A run has at most 10 tool calls in
    /// flight; the rest wait and start oldest first. All of this work
    /// happens in [`Harness::run`].
    #[must_use]
    pub fn new(
        recorder: Arc<dyn RunRecorder>,
        broker: Arc<dyn InferenceBroker>,
        timer: Arc<dyn Timer>,
        host: Arc<Packs>,
        services: HostServices,
    ) -> Harness {
        Harness {
            recorder,
            broker,
            timer,
            host,
            services,
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

    /// Runs `request` to its end and reports how it ended.
    ///
    /// It begins the run's record, resolves the run's model through the
    /// inference broker, prepares the prompt from its source, waits for
    /// the Host's Packs to be ready, drives the run, and reads the declared
    /// output file if the run completed. The returned future is `Send`.
    /// Its only runtime needs are those of the Host's performers and Packs.
    ///
    /// Every run gets a record and ends with its result, which the report
    /// carries beside the run's id: the final text, or the error that
    /// ended the run, where a cancel is the error whose code is
    /// `canceled`. The record begins before anything else, so a run that
    /// ends before it starts is recorded too. A cancel raised before
    /// calling `run`, or while the broker lists its models, ends the run
    /// canceled. A model list the broker fails to fetch ends it with the
    /// broker's failure, and a list that omits the selected model with a
    /// `RequirementsUnmet` failure. A source that fails to parse ends it
    /// with the parse failure, a declared input file the Harness fails to
    /// put in place with the input's failure, and an environment that
    /// falls short of the prompt's requirements, which the run's first
    /// step finds, with a `RequirementsUnmet` failure. An environment that
    /// falls short after a cancel is raised, as when the cancel cuts short
    /// the wait for a Pack that is still starting, ends the run canceled
    /// instead. A run that is pending with nothing to await,
    /// which means the Harness lost track of an effect, ends with an
    /// `Internal` error.
    ///
    /// # Errors
    /// Returns `RecorderFailed` when the recorder refuses a write. That is
    /// the only error: the run is then not recorded to its end. Its
    /// message holds the refusal after `the run could not be recorded: `,
    /// and its `Run` fact is the run the recorder issued, absent when the
    /// recorder refused to begin the run.
    pub async fn run(self, request: RunRequest) -> Result<RunReport, Error> {
        Box::pin(self.run_to_end(request)).await
    }

    async fn run_to_end(self, request: RunRequest) -> Result<RunReport, Error> {
        let Harness {
            recorder,
            broker,
            timer,
            host: packs,
            services,
            control,
        } = self;
        let RunRequest {
            name,
            source,
            file,
            args,
            input_text,
            vfs,
            host,
        } = request;

        let begun = begin(&*recorder, &name, &source)
            .await
            .map_err(|refusal| unrecorded(None, &refusal))?;
        let run_id = begun.run_id;
        let recorded = |refusal: Error| unrecorded(Some(run_id), &refusal);

        let model = match resolve_model(&host, &*broker, control.cancel_handle()).await {
            Ok(model) => model,
            Err(error) => {
                recorder
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

        let services = Services {
            host: packs,
            services,
            vfs: vfs.clone(),
            input_text,
            cancel: control.cancel_handle().clone(),
            recorder: Arc::clone(&recorder),
            broker,
            timer,
            name,
            file,
            model,
            ui: Some(host.ui()),
        };
        let prepared = match prepare(&source, &args, begun, services).await {
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
            prepared.run,
            prepared.performers,
            recorder,
            run_id,
            control.cancel_handle().clone(),
            Arc::clone(control.stop_signal()),
        )
        .await
        {
            Err(refusal) if is_refusal(&refusal) => return Err(recorded(refusal)),
            result => result,
        };
        let output = report_output(&vfs, prepared.output_path.as_deref(), &result);
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
    /// ends canceled before the prompt is parsed. Calling `cancel` more
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

/// Resolves the Host's current model through `broker`, or returns the
/// error that ends the run instead: the cancel when `cancel` fires first,
/// and the lookup's failure when the broker's list cannot be fetched or
/// lacks the selection.
async fn resolve_model(
    host: &HostSnapshot,
    broker: &dyn InferenceBroker,
    cancel: &CancelHandle,
) -> Result<Option<ModelInfo>, Error> {
    let resolving = pin!(current_model(host, broker));
    // The cancel arm is polled first, so a cancel raised before the run,
    // or before a listing that is ready at once, wins.
    match select(cancel.canceled(), resolving).await {
        Either::Left(((), _)) => Err(Error::canceled()),
        Either::Right((resolved, _)) => resolved,
    }
}
