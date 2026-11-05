//! Run preparation: everything between a prompt's source text and a `Run`
//! the effect loop can drive.
//!
//! [`begin`] draws the inputs the Engine refuses to draw itself: the
//! run's seed from the OS CSPRNG and its `started_at` from the wall
//! clock, both handed to the recorder as the run begins, so the record
//! holds them verbatim. `Harness::run_to_end` begins the run before
//! anything else, so every run has a record. Then [`prepare`] takes the
//! steps that build the run: parse; put the prompt's declared `input:`
//! file in place in the store (`files::stage_input`); hand the run's
//! whole filesystem, real directories and the declared store, to the
//! params as given; wait for every built Pack to be ready, or for the
//! run's cancel; take the run's snapshot of the Host's Packs with the
//! run's own services, which yields the catalog, the roster, the
//! preludes, and the tool performer; set the first three and the Host's
//! model on the params; and build the `Run` beside its performers.
//!
//! A prompt that fails to parse, or an input file that cannot be put in
//! place, is a run that ended before it began: the recorder ends it as
//! failed with the cause as the message, so the record answers "why did
//! this run fail" for a run the loop never saw. `Harness::run_to_end`
//! reports that ended run, and its caller reads the events recorded so
//! far from the recorder. An environment that cannot satisfy the prompt
//! fails no preparation: the run is built, and its first step ends it
//! with the Engine's model-readable notice, or as canceled when the
//! run's cancel fired first, as when a cancel cuts the wait short while a
//! needed Pack is still starting. The effect loop records that end like
//! any other, and the record holds no run event after the parse events.
//!
//! The input staging is store work and runs inline, like every other VFS
//! operation of the run.

use std::fmt::{self, Write as _};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use runvil::cancel::CancelHandle;
use runvil::error::{Error, ErrorCode, FactKey};
use runvil::event::Event;
use runvil::model::{ModelInfo, ModelRole};
use runvil::prompt::FileDecl;
use runvil::timestamp::Timestamp;
use runvil::vfs::VfsRef;
use runvil::{Prompt, Run, RunParams, RunSeed};
use runvil_plugin::HostServices;
use sha2::{Digest as _, Sha256};

use crate::display_chain::display_chain;
use crate::files::{input_kind, stage_input};
use crate::host::HostContext;
use crate::performers::{InferenceBroker, Performers, Timer};
use crate::recorder::{
    Record, RecordKind, RunErrorKind, RunId, RunMeta, RunOutcome, RunRecorder, recorder_error,
};

/// What the caller owns and preparation borrows: the Host's built
/// Packs and the run's own services, the real directories, the run's
/// cancel flag, the recorder, the inference broker and the timer that
/// reach beyond the runner, and the run's name and prompt file.
pub struct Services {
    /// The Host's built Packs, from which the run picks its own and takes
    /// their snapshot.
    pub host: Arc<HostContext>,
    /// The run's own services, such as its input broker: every tool call
    /// of the run is lent them.
    pub services: HostServices,
    /// The run's whole filesystem: the real directories and the declared store,
    /// passed straight to the params' VFS.
    pub vfs: VfsRef,
    /// The text staged at the prompt's declared `input:` path before the
    /// run, when the launch supplied one.
    pub input_text: Option<String>,
    /// The run's cancel flag: handed to the params and polled by the
    /// Engine.
    pub cancel: CancelHandle,
    /// The recorder the run began at, which preparation and the loop
    /// write to.
    pub recorder: Arc<dyn RunRecorder>,
    /// Performs the run's `Chat` effects.
    pub broker: Arc<dyn InferenceBroker>,
    /// Performs the run's `Timer` effects.
    pub timer: Arc<dyn Timer>,
    /// The run's name, the one it began under: every event's
    /// `execution`.
    pub name: String,
    /// The file the prompt's source came from, which the parse records.
    pub file: String,
    /// The Host's default model, when one is selected: the run fills all
    /// five roles with it, and its first step checks each declared role's
    /// requirements against it.
    pub model: Option<ModelInfo>,
    /// The Host snapshot the `ui()` global serves, when the run has
    /// one.
    pub ui: Option<serde_json::Value>,
}

impl fmt::Debug for Services {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Services")
            .field("host", &self.host)
            .field("services", &self.services)
            .field("name", &self.name)
            .field("model", &self.model)
            .field("ui", &self.ui)
            .finish_non_exhaustive()
    }
}

/// A run begun at its recorder, with the seed and start its record holds.
#[derive(Debug, Clone, Copy)]
pub struct Begun {
    /// The run's id at the recorder, begun and still open.
    pub run_id: RunId,
    /// The seed the Harness drew for the run.
    pub seed: RunSeed,
    /// When the run started, as the Harness read the wall clock.
    pub started_at: Timestamp,
}

/// A run ready for the effect loop, whose seed, start, and parse events are
/// already at the recorder.
#[derive(Debug)]
pub struct Prepared {
    /// The run, built over the prepared params.
    pub run: Run,
    /// The run's id at the recorder, begun and still open, for
    /// `Harness::run_to_end` to drive; the run's events, its parse events
    /// first, are read from the recorder under this id.
    pub run_id: RunId,
    /// The performers for the run: the caller's inference broker and timer
    /// beside the runner's own tool performer, the run's snapshot of the
    /// Host's Packs.
    pub performers: Performers,
    /// The prompt's declared `output:` path, which the caller reads with
    /// [`read_output`](crate::files::read_output) once the run completes.
    pub output_path: Option<String>,
}

/// The failed outcome a [`prepare`] failure ended its run with, or, for a
/// `RecorderFailed` refusal, which ends nothing, the refusal.
pub(crate) fn ended(error: Error) -> Result<RunOutcome, Error> {
    if error.code == ErrorCode::RecorderFailed {
        return Err(error);
    }
    Ok(failed(ended_kind(&error), &error))
}

/// The kind a [`prepare`] failure ends its run under: the input's kind for
/// an unmet or refused input, and `Parse` for a prompt that does not parse.
fn ended_kind(error: &Error) -> RunErrorKind {
    if error.code == ErrorCode::RequirementsUnmet || error.code.number() / 100 == 6 {
        input_kind(error)
    } else {
        RunErrorKind::Parse
    }
}

/// `error` with run `run`'s id as its `Run` fact.
fn of_run(run: RunId, mut error: Error) -> Error {
    error.facts.insert(FactKey::Run, run.get().into());
    error
}

/// A recorder's refusal of a write for run `run`: `RecorderFailed` with
/// the run's `Run` fact, the refusal built with [`recorder_error`] when
/// the recorder returned another code.
fn refused(run: RunId, error: Error) -> Error {
    let error = if error.code == ErrorCode::RecorderFailed {
        error
    } else {
        recorder_error(error)
    };
    of_run(run, error)
}

/// Begins one run named `name` of the prompt `source` at `recorder`: draws
/// the run's seed and start and hands both to the recorder with the
/// prompt's hash.
///
/// # Errors
/// Returns the recorder's refusal to begin the run.
pub async fn begin(recorder: &dyn RunRecorder, name: &str, source: &str) -> Result<Begun, Error> {
    let seed = RunSeed::from_bytes(rand::random());
    let started_at = now_timestamp();
    let run_id = recorder
        .begin_run(RunMeta {
            name: name.to_owned(),
            prompt_hash: prompt_hash(source),
            seed,
            flags: 0,
            started_at: started_at.unix_millis(),
        })
        .await?;
    Ok(Begun {
        run_id,
        seed,
        started_at,
    })
}

/// Prepares the prompt `source` for the run `begun` with `args`: parses
/// the prompt, puts its declared input file in place, waits for every
/// built Pack to be ready or for the run's cancel, takes the run's
/// snapshot of the Host's Packs, sets the snapshot's catalog, roster, and
/// declared preludes and the Host's model on the params, and builds the
/// `Run` and its performers.
///
/// A Pack whose `ready` fails is unavailable to this run, with its
/// error as the reason. A cancel ends the wait, and each Pack is
/// snapshotted as it stands. A prompt the environment cannot satisfy is
/// prepared all the same: the run's first step ends it with the Engine's
/// notice, or as canceled when the run's cancel fired first.
///
/// # Errors
/// Returns the parse failure when the source does not parse and the input
/// failure from `stage_input` when its declared input file cannot be put
/// in place. In both cases the run is ended as failed, the failure keeps
/// its own code and gains the run's `Run` fact, and `Harness::run_to_end`
/// reports it while the events recorded so far stay at the recorder.
/// Returns `RecorderFailed` with the `Run` fact when the recorder refuses
/// a write.
pub async fn prepare(
    source: &str,
    args: &str,
    begun: Begun,
    services: Services,
) -> Result<Prepared, Error> {
    let Services {
        host,
        services,
        vfs,
        input_text,
        cancel,
        recorder,
        broker,
        timer,
        name,
        file,
        model,
        ui,
    } = services;
    let Begun {
        run_id,
        seed,
        started_at,
    } = begun;
    let recorded = |source| refused(run_id, source);

    // Parse-time events are the run's first records, whether or not the
    // parse succeeds.
    let (prompt, parse_events) = Prompt::parse(source, &file, &name);
    for event in &parse_events {
        let record = event_record(event).map_err(recorded)?;
        recorder.append(run_id, record).await.map_err(recorded)?;
    }
    let prompt = match prompt {
        Ok(prompt) => prompt,
        Err(source) => {
            recorder
                .end_run(run_id, failed(ended_kind(&source), &source))
                .await
                .map_err(recorded)?;
            return Err(of_run(run_id, source));
        }
    };

    // The declared input is in place before anything else sees the
    // store: the run's first section may read it.
    let declared = prompt.frontmatter().input().map(FileDecl::path);
    if let Err(source) = stage_input(&vfs, declared, input_text) {
        recorder
            .end_run(run_id, failed(ended_kind(&source), &source))
            .await
            .map_err(recorded)?;
        return Err(of_run(run_id, source));
    }
    let output_path = prompt
        .frontmatter()
        .output()
        .map(|decl| decl.path().to_owned());

    // The parse events were stamped under task `0` from zero; the run's
    // root task continues the sequence past them, so `(task_id, task_seq)`
    // is unique across every record of the run.
    let provenance_start = u32::try_from(parse_events.len()).unwrap_or(u32::MAX);
    let mut ctx = RunParams::new(name, seed, started_at)
        .with_cancel(cancel.clone())
        .with_provenance_start(provenance_start);
    if let Some(ui) = ui {
        ctx = ctx.with_ui(ui);
    }
    // The pick-wait-snapshot ceremony: the run's whole filesystem, the
    // real directories and the declared store, goes to the params as
    // given; the run's Packs are picked from the prompt's `uses:` entries;
    // the snapshot is taken once each of them is ready or the run is
    // canceled, its catalog is what the run offers, its roster gives each
    // Pack's object in the run's Lua and is what the run's first step
    // checks the declared Packs against, its preludes go to every section
    // VM, and the snapshot itself stays here as the tool performer,
    // lending each call the run's services.
    let packs = host.pick(&prompt);
    let failures = host.wait_ready(&packs, &cancel).await;
    let snapshot = host.begin_run(&packs, services, failures);
    let ctx = ctx
        .with_vfs(vfs)
        .with_tools(snapshot.catalog())
        .with_packs(snapshot.roster())
        .with_preludes(snapshot.preludes(&prompt))
        .with_models(
            model
                .into_iter()
                .map(|model| (ModelRole::Default, model))
                .collect(),
        );

    let run = Run::new(Arc::new(prompt), args, ctx);
    let performers = Performers {
        broker,
        tool: Arc::new(snapshot),
        timer,
    };
    Ok(Prepared {
        run,
        run_id,
        performers,
        output_path,
    })
}

/// The failed outcome of a run that ended in preparation: `kind` is the
/// Engine's kind for the failure and the message is the cause chain.
fn failed(kind: RunErrorKind, source: &dyn std::error::Error) -> RunOutcome {
    RunOutcome::Failed {
        kind,
        message: display_chain(source),
    }
}

/// One event's record under its own provenance.
fn event_record(event: &Event) -> Result<Record, Error> {
    let provenance = event.provenance();
    Ok(Record {
        task_id: provenance.task.to_string(),
        task_seq: provenance.seq,
        kind: RecordKind::Event,
        effect_id: None,
        payload: serde_json::to_value(event).map_err(recorder_error)?,
    })
}

/// The prompt text's content hash for the run's metadata, `sha256:` and
/// the lowercase hex digest.
fn prompt_hash(source: &str) -> String {
    let digest = Sha256::digest(source.as_bytes());
    let mut hash = String::with_capacity(7 + digest.len() * 2);
    hash.push_str("sha256:");
    for byte in digest {
        // Writing to a String is infallible.
        let _ = write!(hash, "{byte:02x}");
    }
    hash
}

/// The system clock now as the Engine's `Timestamp`: the Harness's stamp for
/// a run's `started_at`, since the Engine reads no clock of its own. A
/// clock before the epoch or beyond `i64` milliseconds (neither reachable
/// on a real machine) saturates to the epoch rather than refusing the launch.
fn now_timestamp() -> Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok())
        .map_or(Timestamp::UNIX_EPOCH, Timestamp::from_unix_millis)
}
