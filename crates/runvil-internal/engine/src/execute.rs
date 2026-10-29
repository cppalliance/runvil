//! Section lifecycle execution and fall-through.
//!
//! The public design - the section walk, the caller's step loop, reporting,
//! tool binding, and running without a runtime - is documented on the
//! `runvil` facade's crate page and its role modules.
//!
//! # Module layout
//!
//! The run's outcome type ([`RunResult`]) is defined here; the rest lives in
//! focused private children:
//!
//! - `bindings` - the run's model bindings, [`ModelBindings`].
//! - `config` - the public [`RunContext`], whose `prepare` binds model
//!   roles, and [`RunLimits`]; installing Packs and taking each run's
//!   catalog from them is the Harness's job.
//! - `context` - the ambient `RunState` run state.
//! - `error` - the public [`RunError`] and its stable [`RunErrorKind`].
//! - `fill` - prepare's model-role fill function.
//! - `infer` - the nested-inference round's answer.
//! - `requirements` - the preflight [`Requirements`] report.
//! - `run` - the boundary with the caller: the `Run` state machine with its
//!   `step`, `resume`, and `cancel`, the `Step` it returns, and the effect
//!   vocabulary (the `Effect` a leaf arm issues, its serializable
//!   `EffectRecord`, and the `EffectAnswer` the caller returns).
//! - `scheduler` - the chain scheduler driving the coroutine protocol:
//!   the H1 pass, the walk, call chains, fanout, and the `chat` and
//!   `tool_call` rounds that `models.loop` and a model handle's `loop`
//!   yield over the section's `messages`.
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

mod bindings;
mod config;
pub(crate) mod context;
mod error;
mod fill;
mod infer;
mod requirements;
mod run;
pub(crate) mod scheduler;
pub(crate) mod scope;
mod section_context;
pub(crate) mod section_vm;
mod turn;
mod walk_target;

// Public API surface.
pub use bindings::ModelBindings;
pub use config::{RunContext, RunLimits};
pub use error::{RunError, RunErrorKind, SourceLocation};
pub use requirements::{
    MissingService, RequirementCheck, Requirements, UnavailablePack, UnmetRequirement, UsablePack,
};
pub use run::{
    AnswerRecord, ChatAnswerRecord, Effect, EffectAnswer, EffectId, EffectRecord, FailureRecord,
    IssuedEffect, PackToolSets, Round, Run, Step, ToolAnswerRecord, ToolCallOrigin, ToolCaller,
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

/// The outcome of a run, which the caller reads out of [`Step::Done`].
///
/// Every outcome is a value of this enum. That includes a prompt that
/// declines its task: a decline is not an error and not a cancellation, so
/// the run still completes, with the prompt's reason for declining as the
/// [`Completed`](RunResult::Completed) text. The variant tells code what
/// happened, and the payload explains it to people and models.
///
/// This enum is the one outcome type that is `#[non_exhaustive]`, so a
/// `match` on it needs a wildcard arm, while [`Step`], [`Effect`], and
/// [`EffectAnswer`] are deliberately closed. Those three are the caller's
/// protocol: its loop names every kind, so a new kind there must break its
/// build rather than reach code that cannot handle it. A run's outcome is
/// the exception because the caller consumes it as a result, and a wildcard
/// arm that treats an unfamiliar outcome as a failure is a sound default
/// where an unfamiliar effect kind is not.
///
/// A failure's [`RunError`] has a [`kind`](RunError::kind) that classifies
/// the failure by condition; [`RunErrorKind`] documents each kind.
#[derive(Debug)]
#[non_exhaustive]
pub enum RunResult {
    /// The run completed with its final text.
    Completed(String),
    /// The caller cancelled the run, or dropped an effect the run was
    /// waiting on and the cancelled error went uncaught.
    Cancelled,
    /// The run failed. The error's kind classifies the failure.
    Failure(RunError),
}

#[cfg(test)]
mod tests;
