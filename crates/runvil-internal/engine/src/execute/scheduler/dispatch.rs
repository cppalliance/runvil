//! The request arms: one dispatch per validated protocol request from a
//! suspended chain. Every leaf arm builds its [`Effect`] and issues it
//! through the chain view's one `issue` path; the arm performs nothing and
//! emits nothing for the answer, which `apply_answer` handles when it
//! lands. Every store operation is a leaf yield, handed to the caller
//! uniformly for all backends - no inline fast path - so interleaving
//! behavior never depends on which backend serves the mount. A refusal
//! decided before dispatch may still report its failure and answer inline,
//! so the arm returns without the chain ever parking.
//!
//! This file keeps three routing arms: `infer`, `call`, and `store`. Every
//! other group lives in its own module - `chat` the `chat` arm, `tasks` the
//! `spawn` arm, `tool_call` the `tool_call`, `tool_calls`, and
//! `adhoc_tool_done` arms, `timer` the `timer` arm, and `waits` the task
//! wait, inspection, note, concurrency, and cancel arms.

use std::sync::Arc;

use runvil_types::event::ReplyOrigin;

use crate::error::DepthVia;
use crate::execute::run::Effect;
use crate::execute::section_context::TaskSeed;
use crate::lua::{Answer, Request, VfsOp, resolve_model_binding};
use crate::model::{Message, ModelBinding};
use crate::{Error, Result};

use super::chain::ChainStart;
use super::chain_record::{ChainKind, Parked};
use super::{ChainIndex, ChainOps, Continuation, Counters, Scheduler};

/// How one leaf dispatch resolved when its preparation succeeded: an
/// effect issued, with the chain parked on the effect table, or an answer
/// settled on the driver thread without leaving. A preparation failure
/// returns `Err` instead.
#[derive(Debug)]
pub(super) enum Dispatched {
    Issued,
    Answered(Answer<Error>),
}

/// What a chain parks on when it yields `request`; `None` for the arms
/// answered inline, whose chain is back on the ready queue before anyone
/// can look. A wait's set and a batch's slots start empty, and their arms
/// fill them when the chain parks.
fn parked_on(request: &Request) -> Option<Parked> {
    match request {
        Request::Infer { .. } | Request::Chat { .. } => Some(Parked::Chat),
        Request::Call { .. } => Some(Parked::Call),
        Request::JoinAny { .. } => Some(Parked::Tasks(Vec::new())),
        Request::ToolCall { .. } => Some(Parked::ToolCall),
        Request::ToolCalls { .. } => Some(Parked::Batch(Vec::new())),
        Request::Store { .. } => Some(Parked::Store),
        Request::Spawn { .. }
        | Request::Timer { .. }
        | Request::Ready { .. }
        | Request::Status { .. }
        | Request::Pending
        | Request::Concurrency { .. }
        | Request::Note { .. }
        | Request::Cancel { .. }
        | Request::AdhocToolDone { .. } => None,
    }
}

impl Scheduler {
    /// Dispatches one validated request from a suspended chain.
    ///
    /// # Errors
    /// Returns the store arm's error when the chain's access capability is
    /// gone, the `adhoc_tool_done` arm's error when no adhoc tool call is
    /// open, or [`Error::Internal`] when an arm's answer finds no suspended
    /// block to take it.
    pub(super) fn dispatch(&mut self, id: ChainIndex, request: Request) -> Result<()> {
        self.chains[id].parked = parked_on(&request);
        match request {
            Request::Infer { prompt, binding } => {
                self.with_chain(id, |ops| ops.dispatch_infer(&prompt, binding))
            }
            Request::Call { target, input, var } => {
                self.dispatch_call(id, &target, input.as_deref(), &var)
            }
            Request::Spawn {
                target,
                input,
                item,
                index,
                var,
                via,
            } => self.dispatch_spawn(
                id,
                &target,
                input.as_deref(),
                TaskSeed { item, index },
                var,
                via,
            ),
            Request::Timer { duration } => self.dispatch_timer(id, duration),
            Request::JoinAny { tasks } => self.dispatch_join_any(id, tasks),
            Request::Ready { task } => self.dispatch_ready(id, &task),
            Request::Status { task } => self.dispatch_status(id, &task),
            Request::Pending => self.dispatch_pending(id),
            Request::Concurrency { limit } => self.dispatch_concurrency(id, limit),
            Request::Note { text } => self.dispatch_note(id, text),
            Request::Cancel { task } => self.dispatch_cancel(id, &task),
            Request::ToolCall { alias, args, model } => {
                self.with_chain(id, |ops| ops.dispatch_tool_call(&alias, args, model))
            }
            Request::ToolCalls { calls, turn } => {
                self.with_chain(id, |ops| ops.dispatch_tool_calls(calls, turn))
            }
            Request::AdhocToolDone { outcome } => {
                self.with_chain(id, |ops| ops.dispatch_adhoc_tool_done(outcome))
            }
            Request::Store { op } => self.with_chain(id, |ops| ops.dispatch_vfs(op)),
            Request::Chat { list, binding } => {
                self.with_chain(id, |ops| ops.dispatch_chat(&list, binding))
            }
        }
    }

    /// Dispatches a `call` request: constructs the child chain and
    /// enqueues it; the parent blocks until the child's finish delivers
    /// its final text as the answer. Its preparation fails on the depth
    /// cap, target resolution, or child construction.
    fn dispatch_call(
        &mut self,
        id: ChainIndex,
        target: &str,
        input: Option<&str>,
        var: &serde_json::Value,
    ) -> Result<()> {
        match self.prepare_call(id, target, input, var) {
            Ok(child) => {
                self.ready.push_back(child);
                Ok(())
            }
            Err(error) => self.answer_inline(id, Err(error)),
        }
    }

    /// The fallible half of call dispatch: the depth cap checked against
    /// the caller's call-depth field, the target resolved over the
    /// caller's visible set, and the child chain constructed one level
    /// deeper under the call's args and `var` snapshot.
    fn prepare_call(
        &mut self,
        id: ChainIndex,
        target: &str,
        input: Option<&str>,
        var: &serde_json::Value,
    ) -> Result<ChainIndex> {
        let chain = &self.chains[id];
        let depth = chain.call_depth.deeper(DepthVia::Call)?;
        // An explicit input forks the chain's args (and `argv` re-derives
        // from them); a no-input call inherits the caller's context whole,
        // so the run's frozen `argv` - H1's repair included - reaches the
        // chain rather than re-deriving from the unchanged args.
        let child_ctx = match input {
            Some(input) => chain.ctx.with_args(input),
            None => chain.ctx.clone(),
        };
        // A call chain is a blocking child: it borrows the caller's access
        // capability (the same serial thread of execution), so the caller's
        // standing claims never false-conflict with the child's ops.
        let access = Arc::clone(chain.access()?);
        // A call chain's effective limit starts with its caller's: the
        // tasks it spawns run within the caller's share.
        let child_concurrency = chain.concurrency;
        // `chain`'s arena borrow ends here; the resolution names the
        // target's slice by path, so nothing borrows the arena across it.
        let resolved = self.with_chain(id, |ops| ops.resolve_chain_target(target))?;
        // The child's id is the caller's next child index: `call` children,
        // spawned tasks, and the wait shims' timers share the caller's
        // counter, so the id depends only on the caller's own dispatch
        // order.
        let chain_id = self.chains[id].next_child_id()?;
        self.start_chain(ChainStart {
            lineage: chain_id,
            counters: Counters::default(),
            ctx: child_ctx,
            access,
            start: resolved.pos,
            kind: ChainKind::Call { parent: id },
            var: var.clone(),
            call_depth: depth,
            concurrency: child_concurrency,
        })
    }
}

impl ChainOps<'_> {
    /// Dispatches an `infer` request: resolves the binding, issues the
    /// single tool-free gateway round as a `Chat` effect over one user
    /// message, and parks the chain in the effect table. Its preparation
    /// fails on the binding resolution.
    fn dispatch_infer(&mut self, prompt: &str, binding: Option<ModelBinding>) -> Result<()> {
        match self.prepare_infer(prompt, binding) {
            Ok(()) => Ok(()),
            Err(error) => self.answer(Err(error)),
        }
    }

    /// The fallible half of infer dispatch: the binding resolution (the
    /// handle's frozen binding, else the section's current model) and the
    /// issued effect.
    fn prepare_infer(&mut self, prompt: &str, binding: Option<ModelBinding>) -> Result<()> {
        let chain = &*self.chain;
        let binding = if let Some(binding) = binding {
            binding
        } else {
            resolve_model_binding(chain.ctx.models(), chain.frame()?.vm().model_selection())?
                .ok_or_else(|| Error::ModelRequired {
                    section: chain.section_name().to_owned(),
                })?
        };
        // A nested infer round consumes only the accumulated completion;
        // its `Infer` origin tells the caller its live deltas have no
        // consumer.
        let round = self.number_round(ReplyOrigin::Infer);
        let effect = Effect::Chat {
            binding,
            messages: vec![Message::user(prompt)],
            after: None,
            keep: 0,
            tools: Vec::new(),
            round,
        };
        self.issue(effect, Continuation::Infer(round.id));
        Ok(())
    }

    /// Dispatches a `store` request: derives the store view from the
    /// chain's access capability and issues the operation through it as a
    /// `Vfs` effect for the caller to perform, parking the chain in the
    /// effect table exactly as a leaf I/O round does. The operation's event
    /// is pushed when the answer is applied, before the chain resumes.
    ///
    /// # Errors
    /// Returns [`Error::Internal`] when the live chain's access capability
    /// is gone, which only the chain-end paths take, or [`Error::Store`]
    /// when the handle the chain's access came from declares no store,
    /// which the run's start probe already ruled out.
    fn dispatch_vfs(&mut self, op: VfsOp) -> Result<()> {
        let access = Arc::clone(self.chain.access()?);
        let view = Arc::new(runvil_vfs::detail::store_view(&access).map_err(Error::store)?);
        let effect = Effect::Vfs {
            access: view,
            op: op.clone(),
        };
        self.issue(effect, Continuation::Vfs(op));
        Ok(())
    }
}
