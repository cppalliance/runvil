//! Answer application: the one path every performed effect's answer takes
//! back into the scheduler.
//!
//! The caller hands back a raw [`EffectAnswer`] - a completion, a tool's own
//! output, a store outcome, a timer's firing - and knows nothing of what
//! the parked chain asked for. `apply_answer` pairs the answer with the
//! effect's [`Continuation`] and turns it into the chain's protocol
//! [`Answer`] on the caller's thread, emitting the round's events there:
//! the model turn's boundaries and content, the tool call's
//! succeeded/failed event and `ToolResult` under the trust rule, the store
//! operation's outcome. A timer's firing completes its
//! entry and wakes the waiter instead of resuming a chain, and a render's
//! answer freezes the section entry's sets and dispatches the round that
//! issued it. A `Dropped` answer resumes the chain with the cancelled
//! error, whatever it was parked on; for a render, that is the round's
//! cancelled answer. A tool call's answer, of either kind, also ends the
//! call's identity: what the tool did is joined back into the chain, and
//! the call's access refuses every later operation.

use runvil_types::event::lifecycle::{self, Lifecycle};
use runvil_types::ids::RoundId;
use runvil_types::tools::{ToolError, ToolOutput};
use runvil_vfs::{ExecId, VfsError, detail};

use crate::execute::infer::accept_infer;
use crate::lua::{
    Answer, ModelReport, ToolCallOutcome, VfsOp, VfsOutcome, prepare_dispatch,
    prepare_model_dispatch,
};
use crate::model::{Completion, CompletionError};
use crate::{Error, Result};

use super::tasks::{SlotState, TaskBacking};
use super::{
    CallSite, ChainIndex, ChainOps, Continuation, EffectAnswer, EffectId, Pending, Scheduler,
    ToolCallContinuation,
};

/// The succeeded/failed observation pair one store operation reports;
/// nothing for an op this crate does not name.
fn vfs_observations(op: &VfsOp) -> Option<(Lifecycle, Lifecycle)> {
    let pair = match op {
        VfsOp::Write { .. } => (lifecycle::VFS_WRITE_SUCCEEDED, lifecycle::VFS_WRITE_FAILED),
        VfsOp::Append { .. } => (
            lifecycle::VFS_APPEND_SUCCEEDED,
            lifecycle::VFS_APPEND_FAILED,
        ),
        VfsOp::Read { .. } => (lifecycle::VFS_READ_SUCCEEDED, lifecycle::VFS_READ_FAILED),
        VfsOp::ReadNumbered { .. } => (
            lifecycle::VFS_READ_NUMBERED_SUCCEEDED,
            lifecycle::VFS_READ_NUMBERED_FAILED,
        ),
        VfsOp::StrReplace { .. } => (
            lifecycle::VFS_REPLACE_SUCCEEDED,
            lifecycle::VFS_REPLACE_FAILED,
        ),
        VfsOp::Delete { .. } => (
            lifecycle::VFS_DELETE_SUCCEEDED,
            lifecycle::VFS_DELETE_FAILED,
        ),
        VfsOp::Glob { .. } => (lifecycle::VFS_GLOB_SUCCEEDED, lifecycle::VFS_GLOB_FAILED),
        VfsOp::Exists { .. } => (
            lifecycle::VFS_EXISTS_SUCCEEDED,
            lifecycle::VFS_EXISTS_FAILED,
        ),
        // Any op `runvil-lua` adds behind its `#[non_exhaustive]`
        // `VfsOp` before this crate names it reports nothing.
        _ => return None,
    };
    Some(pair)
}

/// Classifies one store operation's failure for the answer channel. A
/// claims-model conflict becomes the fatal determinism violation: the
/// driver intercepts it at the answer boundary and ends the run on the
/// spot rather than resuming it into Lua, so no author `pcall` can catch
/// it. Every other failure returns as the call's answer as an
/// [`Error::Store`] holding the model-facing message rendered for the
/// operation and the structured cause, which ends the run as a
/// `RunErrorKind::Vfs` error if it aborts the chunk uncaught.
fn classify_vfs_failure(op: &VfsOp, error: VfsError) -> Error {
    match error {
        VfsError::Conflict { detail, .. } => Error::Determinism(detail),
        // `VfsError` is `#[non_exhaustive]` across the crate seam; every
        // other failure is the call's answer.
        error => Error::store_op(op, error),
    }
}

/// The cancelled answer for a chain parked on `resume`'s kind of effect:
/// the protocol variant the chain's shim expects, holding the run's
/// cancellation error, which for a render is the round's chat answer. A
/// timer's drop is applied to its entry and its waiter instead, before
/// this is reached.
fn dropped_answer(resume: &Continuation) -> Answer<Error> {
    match resume {
        Continuation::Infer(_) => Answer::Infer(Err(Error::Interrupted)),
        Continuation::Chat(_) | Continuation::Render(_) => Answer::Chat(Err(Error::Interrupted)),
        Continuation::ToolCall(_) => Answer::ToolCallResult(Err(Error::Interrupted)),
        // A timer's drop never reaches here; the cancelled store answer
        // is the harmless stand-in should it ever do so.
        Continuation::Vfs(_) | Continuation::Timer => Answer::Store(Err(Error::Interrupted)),
    }
}

impl Scheduler {
    /// Applies one performed effect's answer to `parked`, the entry
    /// `resume` took out of the effect table: turns the raw answer into the
    /// parked chain's protocol answer under the effect's continuation
    /// (emitting the round's events), and re-queues the chain. A batched
    /// tool call's answer fills its slot instead, and the chain re-queues
    /// once the whole batch has landed. A timer's firing completes its entry
    /// and wakes its waiter instead, and a render's answer is applied by
    /// [`ChainOps::accept_render`]. A `Dropped` answer is the caller
    /// giving the effect up: the chain resumes with the cancelled error.
    /// Every answer to a tool call, `Dropped` included, first ends the
    /// call's identity through [`Scheduler::end_tool_call`], so whatever
    /// the tool did happens before the chain's next step and nothing it
    /// tries after is admitted.
    ///
    /// # Errors
    /// Returns [`Error::Internal`] when the answer's kind does not match
    /// the effect's, when a batched call's answer finds no open slot to
    /// fill, or when the chain it resumes has no suspended block.
    /// Returns [`Error::Determinism`] when a store answer reports a
    /// claims-model conflict: the run ends on the spot rather than
    /// resuming the conflict into Lua, where an author `pcall` could catch
    /// it.
    pub(super) fn apply_answer(
        &mut self,
        id: EffectId,
        parked: Pending,
        answer: EffectAnswer,
    ) -> Result<()> {
        let Pending { chain, resume } = parked;
        let slot = match &resume {
            Continuation::ToolCall(call) => {
                self.end_tool_call(chain, call.exec);
                match call.site {
                    CallSite::Batch { slot, .. } => Some(slot),
                    CallSite::Script { .. } | CallSite::Model { .. } => None,
                }
            }
            Continuation::Infer(_)
            | Continuation::Chat(_)
            | Continuation::Render(_)
            | Continuation::Vfs(_)
            | Continuation::Timer => None,
        };
        let answer = match (resume, answer) {
            (Continuation::Timer, EffectAnswer::Dropped) => return self.drop_timer(id),
            (resume, EffectAnswer::Dropped) => dropped_answer(&resume),
            (Continuation::Infer(round), EffectAnswer::Chat(result)) => {
                Answer::Infer(self.with_chain(chain, |ops| ops.accept_infer(round, result)))
            }
            (Continuation::Chat(round), EffectAnswer::Chat(result)) => self
                .with_chain(chain, |ops| {
                    ops.accept_chat(round, result.map_err(Error::from))
                })?,
            (Continuation::Render(binding), EffectAnswer::Render(rendered)) => {
                return self.with_chain(chain, |ops| ops.accept_render(binding, rendered));
            }
            (Continuation::ToolCall(call), EffectAnswer::ToolCall(result)) => {
                Answer::ToolCallResult(
                    self.with_chain(chain, |ops| ops.accept_tool_call(&call, result)),
                )
            }
            (Continuation::Vfs(op), EffectAnswer::Vfs(result)) => {
                match self.with_chain(chain, |ops| ops.accept_vfs(&op, result)) {
                    // A claims-model conflict is fatal: the suspended
                    // chains drop unarmed in the run's teardown, exactly
                    // as on the cancellation path.
                    Err(error @ Error::Determinism(_)) => return Err(error),
                    result => Answer::Store(result),
                }
            }
            (Continuation::Timer, EffectAnswer::Timer) => return self.fire_timer(id),
            (
                Continuation::Infer(_) | Continuation::Chat(_),
                EffectAnswer::Render(_)
                | EffectAnswer::ToolCall(_)
                | EffectAnswer::Vfs(_)
                | EffectAnswer::Timer,
            )
            | (
                Continuation::Render(_),
                EffectAnswer::Chat(_)
                | EffectAnswer::ToolCall(_)
                | EffectAnswer::Vfs(_)
                | EffectAnswer::Timer,
            )
            | (
                Continuation::ToolCall(_),
                EffectAnswer::Chat(_)
                | EffectAnswer::Render(_)
                | EffectAnswer::Vfs(_)
                | EffectAnswer::Timer,
            )
            | (
                Continuation::Vfs(_),
                EffectAnswer::Chat(_)
                | EffectAnswer::Render(_)
                | EffectAnswer::ToolCall(_)
                | EffectAnswer::Timer,
            )
            | (
                Continuation::Timer,
                EffectAnswer::Chat(_)
                | EffectAnswer::Render(_)
                | EffectAnswer::ToolCall(_)
                | EffectAnswer::Vfs(_),
            ) => {
                return Err(Error::internal(
                    "an effect's answer must be of the effect's own kind",
                ));
            }
        };
        if let Some(slot) = slot {
            return self.with_chain(chain, |ops| ops.fill_batch_slot(slot, answer));
        }
        self.answer_inline(chain, answer)
    }

    /// Ends `exec`, the identity of the tool call `chain` was parked on,
    /// whether the call was answered or aborted: merges it into the
    /// chain's access when the chain still holds one, and refuses every
    /// later operation, spawn, or store view through the call's access.
    /// A run whose scope has closed has already refused every access, so
    /// there is nothing to end.
    pub(super) fn end_tool_call(&self, chain: ChainIndex, exec: ExecId) {
        let Some(scope) = &self.scope else {
            return;
        };
        let owner = self.chains[chain]
            .access()
            .ok()
            .map(|access| detail::access_id(access));
        detail::end_access(scope, owner, exec);
    }

    /// Applies a dropped timer: the entry backed by the effect moves to
    /// `Cancelled`. While the run's cancel flag is clear, the caller gave
    /// up the timeout and the run goes on, so the chain parked on the
    /// timer leaves its wait with the cancelled error, as a dropped `Chat`
    /// or `ToolCall` does: a timed `tasks.join` or `tasks.join_any` raises
    /// it. A timer nothing waits on is dropped silently. Under a run cancel the
    /// drop wakes no one, since the cancel tears the waiter down with
    /// every other chain.
    fn drop_timer(&mut self, effect: EffectId) -> Result<()> {
        let Some((task, slot)) = self
            .tasks
            .iter_mut()
            .find(|(_, slot)| slot.backing == TaskBacking::Effect(effect) && slot.state.is_live())
        else {
            return Ok(());
        };
        slot.state = SlotState::Cancelled;
        let (task, owner) = (task.clone(), slot.owner);
        if self.ctx.cancel().is_cancelled() || !self.chains[owner].end_wait_for(&task) {
            return Ok(());
        }
        self.wake_from_wait(owner, Answer::JoinAny(Err(Error::Interrupted)))
    }
}

impl ChainOps<'_> {
    /// Applies a nested infer round's completion: the single-prose-round
    /// reporting through the chain's own emitter, stamped with `round`,
    /// then the round's text.
    fn accept_infer(
        &self,
        round: RoundId,
        result: std::result::Result<Box<Completion>, CompletionError>,
    ) -> Result<String> {
        let chain = &*self.chain;
        accept_infer(
            result,
            round,
            chain.ctx.emitter(),
            chain.section_name(),
            chain.ctx.turns(),
        )
    }

    /// Applies a bound tool call's own answer through the shared dispatch
    /// body: the succeeded/failed event, the trust rule, and the
    /// `ToolResult` report - under the model's call id when the model
    /// issued the call (a tool's own failure then resumes as untrusted
    /// failure text), else as a script call classified by the binding's
    /// declared output kind. A script call that receives a listing marks
    /// the chain's frame's innermost open adhoc call. The counts were
    /// taken at dispatch, so the body is handed `None` for them.
    fn accept_tool_call(
        &mut self,
        call: &ToolCallContinuation,
        result: std::result::Result<ToolOutput, ToolError>,
    ) -> Result<ToolCallOutcome> {
        let chain = &mut *self.chain;
        // The shared dispatch body reports through this chain's emitter, so
        // its reports land in the buffer under this chain's task.
        let emitter = chain.ctx.emitter();
        let section = chain.section_name();
        let nonce = chain.ctx.nonce();
        let vendor = chain.ctx.tool_set().vendor_for(call.binding.id());
        match &call.site {
            // Model-issued, alone or batched: the content always resumes,
            // plain - it is the tool record's text for the next round,
            // never classified by output kind.
            CallSite::Model { call_id, .. } | CallSite::Batch { call_id, .. } => {
                let report = ModelReport {
                    script: call.report,
                    call_id: call_id.clone(),
                };
                prepare_model_dispatch(
                    &call.binding,
                    vendor,
                    result,
                    None,
                    nonce,
                    emitter,
                    section,
                    &report,
                )
                .map(|outcome| ToolCallOutcome::Plain(outcome.into_content()))
                .map_err(Error::from)
            }
            CallSite::Script { .. } => match prepare_dispatch(
                &call.binding,
                vendor,
                result,
                None,
                nonce,
                emitter,
                section,
                Some(call.report),
            ) {
                Ok(outcome) => {
                    if outcome.listing() {
                        chain.frame_mut()?.mark_listing();
                    }
                    ToolCallOutcome::from_dispatch(
                        call.binding.output_kind(),
                        call.binding.alias(),
                        outcome.into_content(),
                    )
                    .map_err(Error::from)
                }
                Err(error) => Err(Error::from(error)),
            },
        }
    }

    /// Applies a store operation's answer: the operation's succeeded or
    /// failed observation (pushed before the chain resumes, so the op's
    /// outcome precedes the chunk's closing boundary), then the outcome,
    /// with a failure classified for the answer channel under the
    /// operation's own wording.
    fn accept_vfs(
        &self,
        op: &VfsOp,
        result: std::result::Result<VfsOutcome, VfsError>,
    ) -> Result<VfsOutcome> {
        let chain = &*self.chain;
        if let Some((succeeded, failed)) = vfs_observations(op) {
            chain.ctx.emitter().report(
                chain.section_name(),
                if result.is_ok() { succeeded } else { failed },
            );
        }
        result.map_err(|error| classify_vfs_failure(op, error))
    }
}
