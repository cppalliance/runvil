//! Section lifecycle execution and fall-through.
//!
//! The public design - the section walk, the caller's step loop, reporting,
//! tool binding, and running without a runtime - is documented on the
//! `runvil` facade's crate page and its role modules.
//!
//! # Module layout
//!
//! The run's outcome type ([`RunOutcome`]) is defined here; the rest lives in
//! focused private children:
//!
//! - `config` - the public [`RunConfig`], which carries the caller's
//!   role map for the five model roles, and [`RunLimits`]; installing
//!   Packs and taking each run's catalog from them is the Harness's job.
//! - `context` - the ambient run state: the run-wide `SharedState` and
//!   each chain's `ChainState`.
//! - `error` - the stable [`RunErrorKind`] of a failed outcome, and the
//!   kind and message the Engine's own error ends a run with.
//! - `fill` - the fill function, which builds the model set from the
//!   caller's role map when the run is built.
//! - `infer` - the nested-inference round's answer.
//! - `requirements` - what a run's config lacks for its prompt, which the
//!   run's first step reports as its refusal notice.
//! - `run` - the boundary with the caller: the `Run` state machine with its
//!   `step`, `resume`, and `cancel`, the `Step` it returns, and the effect
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
pub use config::{RunConfig, RunLimits};
pub use error::RunErrorKind;
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

/// The outcome of a run, which the caller reads out of [`Step::Done`].
///
/// Every outcome is a value of this enum. That includes a prompt that
/// declines its task: a decline is not an error and not a cancellation, so
/// the run still completes, with the prompt's reason for declining as the
/// [`Completed`](RunOutcome::Completed) text. The variant tells code what
/// happened, and the payload explains it to people and models.
///
/// Closed on purpose, like [`Step`]: a new outcome breaks every caller's
/// build rather than reach code that cannot handle it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunOutcome {
    /// The run completed.
    Completed {
        /// The run's final text.
        final_text: String,
    },
    /// The run failed.
    Failed {
        /// The failure's stable classification; [`RunErrorKind`] documents
        /// each kind.
        kind: RunErrorKind,
        /// The failure's self-contained message, with the text of each
        /// cause it keeps and, for a parse or internal failure, where it
        /// happened.
        message: String,
    },
    /// The caller cancelled the run or dropped an effect the run was waiting
    /// on and the cancelled error went uncaught, or a block raised a caught
    /// `cancelled` error value again after another suspending call.
    Cancelled,
}

impl RunOutcome {
    /// The text that names this outcome: `completed`, `failed`, or
    /// `cancelled`.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Completed { .. } => "completed",
            Self::Failed { .. } => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[cfg(test)]
mod tests;
