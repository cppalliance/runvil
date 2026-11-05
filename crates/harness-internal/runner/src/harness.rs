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
//! it, so the Harness starts no task and names no runtime.

use std::fmt;
use std::pin::pin;
use std::sync::Arc;

use futures_util::future::{Either, select};
use runvil::cancel::CancelHandle;
use runvil::error::{Error, ErrorCode, FactKey};
use runvil::model::ModelInfo;
use runvil::vfs::VfsRef;
use runvil_plugin::HostServices;

use crate::effect_loop::drive;
use crate::environment::{HostSnapshot, current_model};
use crate::files::{not_completed, report_output};
use crate::host::HostContext;
use crate::performers::{InferenceBroker, Timer};
use crate::prepare::{Services, begin, prepare};
use crate::recorder::{RunId, RunRecorder, is_refusal};

#[path = "harness-control.rs"]
mod control;

pub use control::RunControl;
pub(crate) use control::StopSignal;

/// Drives one run of a prompt for a Host.
///
/// It holds what the Host supplies for the run: a recorder, an inference
/// broker, a timer, the Host's built Packs, and the run's own
/// services. [`Harness::run`] consumes it.
pub struct Harness {
    recorder: Arc<dyn RunRecorder>,
    broker: Arc<dyn InferenceBroker>,
    timer: Arc<dyn Timer>,
    host: Arc<HostContext>,
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
    /// The run's name. It becomes the `execution` field of every event and
    /// the `name` field of the run metadata.
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
    /// fact `not_completed` or `undeclared`, `NotFound` for a completed
    /// run that left no file, or the store's failure, each with the
    /// declared path as the `Path` fact when there is one.
    pub output: Result<String, runvil::error::Error>,
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
        error.facts.insert(FactKey::Run, run.get().into());
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
        host: Arc<HostContext>,
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
                    output: Err(not_completed()),
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
                    output: Err(not_completed()),
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
