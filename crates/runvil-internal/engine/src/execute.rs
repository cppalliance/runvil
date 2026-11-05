//! Section lifecycle execution and fall-through.
//!
//! The public design - the section walk, the caller's step loop, reporting,
//! tool binding, and running without a runtime - is documented on the
//! `runvil` facade's crate page and its role modules.
//!
//! # Module layout
//!
//! [`perform_vfs_op`] is defined here; the rest lives in focused private
//! children:
//!
//! - `context` - the ambient run state: the run-wide `SharedState` and
//!   each chain's `ChainState`.
//! - `fill` - the fill function, which builds the model set from the
//!   caller's role map when the run is built.
//! - `infer` - the nested-inference round's answer.
//! - `params` - the public [`RunParams`], which carries the caller's
//!   role map for the five model roles, and [`RunLimits`]; installing
//!   Packs and taking each run's catalog from them is the Harness's job.
//! - `requirements` - what a run's params lack for its prompt, which the
//!   run's first step reports as its refusal notice.
//! - `run` - the boundary with the caller: the `Run` state machine with its
//!   `step` and `resume`, the `Step` it returns, and the effect
//!   vocabulary (the `Effect` a leaf arm issues, its JSON record, and the
//!   `EffectAnswer` the caller returns).
//! - `scheduler` - the chain scheduler driving the coroutine protocol:
//!   the H1 pass, the walk, call chains, fanout, and the `chat` and
//!   `tool_call` rounds that `models.loop`
//!   yields over the section's `messages`.
//! - `scope` - the merge that builds and checks a section entry's frozen
//!   sets, and the schemas a round projects from its Static Set.
//! - `section_context` - the per-section `SectionContext` frame the
//!   scheduler's chains construct, run, and tear down.
//! - `section_vm` - the section VM setup both frame constructors share: a
//!   walked section's (a spawned task's first entry included) and the H1
//!   pass's.
//! - `turn` - the model-turn counter and the round report every served
//!   completion goes through.
//! - `walk_target` - the walk-target resolution helpers.

pub(crate) mod context;
mod fill;
mod infer;
mod params;
mod requirements;
mod run;
pub(crate) mod scheduler;
pub(crate) mod scope;
mod section_context;
pub(crate) mod section_vm;
mod turn;
mod walk_target;

// Public API surface.
pub use params::{RunLimits, RunParams};
#[cfg(test)]
pub(crate) use run::{AnswerRecord, ChatAnswerRecord, EffectRecord};
pub use run::{
    Effect, EffectAnswer, EffectId, IssuedEffect, PackToolSets, Round, Run, Step, ToolCallOrigin,
    ToolCaller,
};
// The store vocabulary a `Vfs` effect holds and its answer returns, for
// the Engine's own store handling; other crates name it from `runvil_lua`.
use crate::lua::{VfsOp, VfsOutcome};

/// Performs the store operation of an [`Effect::Vfs`] and returns its answer.
///
/// `access` is the store view the effect carries. It is derived from the
/// chain's capability when the effect is dispatched. Each [`VfsOp`] maps
/// onto one `Access` call on that view, so the caller answers a `Vfs`
/// effect exactly as the Engine's own test drivers do. Line bounds arrive
/// as `i64` and convert to `usize`. A read that gives an `end` and omits
/// `start` is refused as an invalid range.
///
/// The function is synchronous because the store is synchronous by
/// design. The caller can run it inline, on the thread that runs its
/// effect loop.
///
/// # Errors
/// Returns the store's own structured failure for the operation (path
/// validation, not-found, anchor, range, conflict, or backend failure).
/// When the caller resumes the run with that answer, the Engine raises
/// the failure at the prompt author's call site.
pub fn perform_vfs_op(
    access: &runvil_vfs::Access,
    op: VfsOp,
) -> std::result::Result<VfsOutcome, runvil_vfs::VfsError> {
    crate::lua::run_store_op(access, op)
}

#[cfg(test)]
mod tests;
