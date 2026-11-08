//! Run preparation: everything between a parsed prompt and a `Run` the
//! effect loop can drive.
//!
//! [`begin`] draws the run's seed from the OS CSPRNG and hands it to the
//! recorder with the start time the caller read, so the record holds both
//! verbatim. `Harness::run_to_end` begins the run before anything else, so
//! every run has a record. Then [`prepare`] takes the steps that build the
//! run: put the prompt's declared `input:` file in place in the store
//! (`files::stage_input`); hand the run's whole filesystem, real
//! directories and the declared store, to the params as given; make the
//! run's instance of each of its Packs with the run's own services, and
//! take the run's snapshot of them once each is ready, or the run's
//! cancel fires, which yields the catalog, the roster, the preludes, and
//! the tool performer;
//! set the first three, the setup's limits, and the run's model on the
//! params; and build the `Run` beside that snapshot.
//!
//! An input file that cannot be put in place is a run that ended before
//! it began: the recorder ends it with that failure as its result, the
//! same error `prepare` returns, so the record answers "why did this run
//! fail" for a run the loop never saw. An environment that cannot satisfy
//! the prompt fails no preparation: the run is built, and its first step
//! ends it with the Engine's model-readable notice, or canceled when the
//! run's cancel fired first, as when a cancel cuts the wait short while a
//! needed Pack is still starting. The effect loop records that end like
//! any other.
//!
//! The input staging is store work and runs inline, like every other VFS
//! operation of the run.

use std::path::PathBuf;
use std::sync::Arc;

use runvil::Error;
use runvil::cancel::CancelHandle;
use runvil::model::{ModelInfo, ModelRole};
use runvil::prompt::FileDecl;
use runvil::timestamp::Timestamp;
use runvil::{Prompt, Run, RunParams, RunSeed};

use crate::files::stage_input;
use crate::host::Host;
use crate::packs::RunPacks;
use crate::recorder::{RunId, RunMeta, of_run, refused};
use crate::setup::RunSetup;

/// Begins the run at `setup.recorder`: draws its seed from the OS random
/// source, records `started_at`, and returns the run id with params
/// holding the seed and start.
///
/// # Errors
/// Returns the recorder's refusal to begin the run.
pub(crate) async fn begin(
    setup: &RunSetup,
    started_at: Timestamp,
) -> Result<(RunId, RunParams), Error> {
    let seed = RunSeed::from_bytes(rand::random());
    let run_id = setup
        .recorder
        .begin_run(RunMeta {
            seed,
            flags: 0,
            started_at: started_at.unix_millis(),
        })
        .await?;
    Ok((run_id, RunParams::new(seed, started_at)))
}

/// Stages `prompt`'s input in `setup.vfs`, makes the run's Packs from
/// `host.packs` with `setup.services` and snapshots them once each is
/// ready or `cancel` fires, and builds the `Run` over `params` under
/// `setup.limits`. Ends the run at the recorder with an input failure as
/// its result and returns it.
///
/// The run's `ui()` snapshot is `setup`'s selection and first workspace
/// root, and `model` fills all five roles, so the run's first step checks
/// each declared role's requirements against it. A Pack whose factory
/// refuses or panics, or whose `ready` fails, is unavailable to this run,
/// with the reason, and so is `runvil/user` when `setup.has_user` is
/// false. A cancel ends the wait, and each Pack is snapshotted as it
/// stands. A prompt the environment cannot satisfy is prepared all the
/// same: the run's first step ends it with the Engine's notice, or as
/// canceled when the run's cancel fired first.
///
/// # Errors
/// Returns the input failure from `stage_input` when the declared input
/// file cannot be put in place. The failure keeps its own code and gains
/// the run's `Run` fact, and the run is ended with it as its result.
/// Returns `RecorderFailed` with the `Run` fact when the recorder refuses
/// that end, and the input failure never has that code.
pub(crate) async fn prepare(
    host: &Host,
    prompt: Prompt,
    setup: &RunSetup,
    run_id: RunId,
    params: RunParams,
    model: Option<ModelInfo>,
    cancel: &CancelHandle,
) -> Result<(Run, RunPacks), Error> {
    // The declared input is in place before anything else sees the
    // store: the run's first section may read it.
    let declared = prompt.frontmatter().input().map(FileDecl::path);
    if let Err(source) = stage_input(&setup.vfs, declared, setup.input_text.clone()) {
        let error = of_run(run_id, source);
        setup
            .recorder
            .end_run(run_id, Err(error.clone()))
            .await
            .map_err(|source| refused(run_id, source))?;
        return Err(error);
    }

    // The run's whole filesystem, the real directories and the declared
    // store, goes to the params as given. The run makes its instance of
    // each of its Packs with its own services, and the snapshot of them
    // is taken once each is ready or the run is canceled: its catalog is
    // what the run offers, its roster gives each Pack's object in the
    // run's Lua and is what the run's first step checks the declared
    // Packs against, its preludes go to every section VM, and the
    // snapshot itself is the run's tool performer.
    let packs = host
        .packs
        .snapshot(&prompt, setup.services.clone(), setup.has_user, cancel)
        .await;
    let params = params
        .with_cancel(cancel.clone())
        .with_limits(setup.limits)
        .with_ui(ui(setup.selected_model.as_deref(), &setup.workspace_roots))
        .with_vfs(setup.vfs.clone())
        .with_tools(packs.catalog())
        .with_packs(packs.roster())
        .with_preludes(packs.preludes(&prompt))
        .with_models(
            model
                .into_iter()
                .map(|model| (ModelRole::Default, model))
                .collect(),
        );

    let run = Run::new(Arc::new(prompt), &setup.args, params);
    Ok((run, packs))
}

/// The `ui()` snapshot as a JSON object: `selected_model`, and the first
/// of `workspace_roots` as `workspace_root`. Each is `null` when absent.
fn ui(selected_model: Option<&str>, workspace_roots: &[PathBuf]) -> serde_json::Value {
    let root = workspace_roots
        .first()
        .map(|root| root.display().to_string());
    serde_json::json!({ "selected_model": selected_model, "workspace_root": root })
}
