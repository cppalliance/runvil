//! The `tool_call` arm: one dispatched tool call yielded by a section VM,
//! script-initiated (`tools.call`) or model-issued (the loop shim, with the
//! model's `call_id`), plus the `adhoc_tool_done` arm that closes an adhoc
//! tool call.
//!
//! Each call's route is decided once, by `tool_route`, on the driver thread
//! before any leaf work, from the call's `CallSite`, which names who
//! asked. A model call's wire name routes by the entry the current frame's
//! section's rendered list holds for it, and a script call routes by id.
//! An adhoc tool is answered with its handler; anything else is a catalog
//! tool, which resolves against the run's offering (a model call by wire
//! name, a script call by id, which also reaches a catalog tool left out
//! of the offering), has its attempt counted, and is issued as a
//! `ToolCall` effect, carrying an identity forked from the chain's access,
//! whose answer the driver applies through the shared dispatch body.
//! A model-issued call, alone or batched, always resumes a bound tool with
//! content - a tool's own failure becomes untrusted failure text - and
//! `ToolResult` fires under the id and the turn of the round that requested
//! the call; a script call keeps the raise-at-call-site behavior, and its
//! `ToolResult` fires under no id and the turn at dispatch.
//!
//! The loop sends a round's unanswered calls together as one
//! `tool_calls` request. The arm walks them in the model's order and
//! issues each catalog call at once into a slot of the chain's batch,
//! stopping at the first call whose route is an adhoc tool. A call that
//! fails to route or to prepare fills its slot with the error and stops
//! the batch too. Each landed answer fills its call's slot, and the
//! chain is answered once no slot is open: every text in call order, or
//! the first failure in call order. Fewer texts than calls tells the
//! loop where the batch stopped, and the loop sends that call alone as
//! an ordinary `tool_call`. The calls in flight run concurrently, each
//! under its own forked identity.
//!
//! An adhoc tool call is a two-yield handshake. The `tool_call` answer is
//! [`ToolCallOutcome::Adhoc`], which hands the shim the handler, and the
//! frame opens an [`AdhocCall`](crate::execute::section_context::AdhocCall)
//! for it. The shim runs the handler inside the
//! chain's block coroutine, so the handler's own suspending calls are
//! ordinary requests of this chain, then yields `adhoc_tool_done`. That
//! arm closes the innermost open call and reports it: the succeeded or
//! failed observation and, on success, the `ToolResult` under the tool's
//! id and the call id recorded at dispatch, trusted unless the handler
//! fetched a listing while the call was open. An adhoc call never issues
//! leaf work of its own. The adhoc arm sits in the `adhoc` child.

#[path = "tool_call-adhoc.rs"]
mod adhoc;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use runvil_types::tools::ToolId;
use runvil_vfs::detail;

use crate::execute::protocol::{Answer, ToolCallOutcome, ToolCallRecord};
use crate::execute::run::{Effect, ToolCallOrigin, ToolCaller};
use crate::lua::{ScriptReport, ToolBinding, ToolCallCounts, current_tool_bindings};
use crate::{Error, Result};

use super::dispatch::unbound_tool_call;
use super::{CallSite, ChainIndex, Continuation, Scheduler, ToolCallContinuation, prompt_origin};

/// How one `tool_call` dispatch resolved: a bound call issued as an effect
/// and parked on the pending table, or an adhoc tool's answer settled
/// on the driver thread. A preparation failure returns `Err` instead.
#[derive(Debug)]
pub(super) enum ToolCallDispatch {
    Issued,
    Answered(Answer<Error>),
}

/// Where one tool call is answered, decided before any catalog lookup so
/// a lone call and a batched one route alike.
#[derive(Debug)]
enum ToolRoute {
    /// An adhoc tool of the chain's section VM, answered with its
    /// handler.
    Adhoc(ToolBinding),
    /// A catalog tool, resolved against the run's offering and issued as
    /// an effect.
    Catalog,
}

impl Scheduler {
    /// Dispatches a `tool_call` request. An issued bound call parks the
    /// chain in the pending table; an adhoc tool's handler resumes the
    /// chain on the spot; every preparation failure - an unbound alias,
    /// the counts install - is the call's answer, resumed
    /// into the caller so an author `pcall` catches it exactly as a tool
    /// failure.
    pub(super) fn dispatch_tool_call(
        &mut self,
        id: ChainIndex,
        alias: &str,
        args: serde_json::Value,
        call_id: Option<String>,
        turn: Option<u32>,
    ) {
        let site = match call_id {
            Some(call_id) => CallSite::Model { call_id, turn },
            None => CallSite::Script { turn },
        };
        match self.prepare_tool_call(id, alias, args, site) {
            // An issued call is parked on the pending table.
            Ok(ToolCallDispatch::Issued) => {}
            Ok(ToolCallDispatch::Answered(answer)) => {
                self.answer_inline(id, answer);
            }
            Err(error) => {
                self.answer_inline(id, Answer::ToolCallResult(Err(error)));
            }
        }
    }

    /// Dispatches a `tool_calls` request: the unanswered calls of one
    /// model round, in the model's order. Each call before the first
    /// whose route is not the catalog is issued as a `CallSite::Batch`
    /// under the round's `turn` and its own slot, so every catalog call
    /// issues in this step. A call that fails to route or to prepare
    /// fills its slot with the error and stops the batch. The slots park
    /// on the chain, which is answered at once when none is open.
    pub(super) fn dispatch_tool_calls(
        &mut self,
        id: ChainIndex,
        calls: Vec<ToolCallRecord>,
        turn: u32,
    ) {
        let mut batch = Vec::with_capacity(calls.len());
        for (slot, call) in calls.into_iter().enumerate() {
            let site = CallSite::Batch {
                call_id: call.id,
                turn,
                slot,
            };
            let issued = match self.tool_route(id, &call.name, &site) {
                Ok(ToolRoute::Adhoc(_)) => break,
                Ok(ToolRoute::Catalog) => {
                    self.prepare_catalog_call(id, &call.name, call.arguments, site)
                }
                Err(error) => Err(error),
            };
            match issued {
                Ok(()) => batch.push(None),
                Err(error) => {
                    batch.push(Some(Err(error)));
                    break;
                }
            }
        }
        self.chains[id.index()].batch = batch;
        self.answer_full_batch(id);
    }

    /// Decides the route of the call at `site` naming `name`, the one
    /// decision a lone call and a batch share. A model call looks its wire
    /// name up in the rendered list of the current frame's section, never
    /// in the chain's advertised scope, which another section's rounds
    /// overwrite while an adhoc tool's handler runs; with no rendered list
    /// it routes as a catalog call. A script call routes by id. Either goes
    /// to the adhoc tool the id names, when it is one.
    fn tool_route(&self, id: ChainIndex, name: &str, site: &CallSite) -> Result<ToolRoute> {
        let vm = self.chains[id.index()].frame()?.vm()?;
        let target = if site.call_id().is_some() {
            match vm.rendered_target(name) {
                Some(advertised) => advertised.id.to_string(),
                None => return Ok(ToolRoute::Catalog),
            }
        } else {
            name.to_owned()
        };
        let adhoc = if target.contains('/') {
            vm.adhoc_tools().binding(&target)?
        } else {
            None
        };
        Ok(adhoc.map_or(ToolRoute::Catalog, ToolRoute::Adhoc))
    }

    /// The turn every report of a call at `site` carries: the requesting
    /// round's turn when the site names one, else the chain's turn
    /// counter at dispatch.
    fn call_turn(&self, id: ChainIndex, site: &CallSite) -> u32 {
        site.turn()
            .unwrap_or_else(|| self.chains[id.index()].ctx.turns().load(Ordering::Relaxed))
    }

    /// The chain's section's tool-call counts, which the section's first
    /// call installs, seeded from its rendered list once there is one and
    /// from its live scope before.
    fn section_call_counts(&mut self, id: ChainIndex) -> Result<ToolCallCounts> {
        let chain = &mut self.chains[id.index()];
        let vm = chain.frame()?.vm()?;
        let ids = match vm.rendered_ids() {
            Some(ids) => ids,
            None => {
                current_tool_bindings(chain.ctx.tool_set(), vm.tool_runtime(), vm.adhoc_tools())?
                    .iter()
                    .map(|binding| binding.id().clone())
                    .collect()
            }
        };
        let ctx = chain.ctx.clone();
        chain.frame_mut()?.script_call_counts(&ctx, &ids)
    }

    /// Fills `slot` of the batch the chain is parked on with a landed
    /// call's answer: a plain text as the text, a failure as the failure,
    /// and any other outcome, which a model-issued call never resumes
    /// with, as an internal error. Once no slot is open the chain is
    /// answered.
    ///
    /// # Errors
    /// Returns [`Error::Internal`] when the chain holds no such slot,
    /// which only an answer the scheduler should have orphaned produces.
    pub(super) fn fill_batch_slot(
        &mut self,
        id: ChainIndex,
        slot: usize,
        answer: Answer<Error>,
    ) -> Result<()> {
        let landed = match answer {
            Answer::ToolCallResult(Ok(ToolCallOutcome::Plain(text))) => Ok(text),
            Answer::ToolCallResult(Err(error)) => Err(error),
            Answer::ToolCallResult(Ok(
                ToolCallOutcome::Structured(_) | ToolCallOutcome::Adhoc { .. },
            ))
            | Answer::Infer(_)
            | Answer::Call(_)
            | Answer::Spawn(_)
            | Answer::Timer(_)
            | Answer::JoinAny(_)
            | Answer::Ready(_)
            | Answer::Status(_)
            | Answer::Pending(_)
            | Answer::Concurrency(_)
            | Answer::Note(_)
            | Answer::Cancel(_)
            | Answer::Chat(_)
            | Answer::ToolCallsResult(_)
            | Answer::Store(_) => Err(Error::internal(
                "a batched call lands as plain text or a failure",
            )),
        };
        let open = self.chains[id.index()]
            .batch
            .get_mut(slot)
            .filter(|open| open.is_none())
            .ok_or(Error::internal(
                "a batched call's answer fills an open slot of its chain's batch",
            ))?;
        *open = Some(landed);
        self.answer_full_batch(id);
        Ok(())
    }

    /// Answers the chain parked on a batch once no slot is open: every
    /// text in call order, or the first failure in call order.
    fn answer_full_batch(&mut self, id: ChainIndex) {
        let chain = &mut self.chains[id.index()];
        if chain.batch.iter().any(Option::is_none) {
            return;
        }
        let answer = std::mem::take(&mut chain.batch)
            .into_iter()
            .flatten()
            .collect::<Result<Vec<String>>>();
        self.answer_inline(id, Answer::ToolCallsResult(answer));
    }

    /// The fallible half of tool-call dispatch, by the call's route: an
    /// adhoc tool (the one-time counts install, the attempt counted, then
    /// the handler's answer), or a catalog tool, which
    /// `prepare_catalog_call` issues.
    ///
    /// Every report the call makes carries the turn of the model round
    /// that requested it, so an adhoc handler earlier in the same batch
    /// that runs model rounds cannot shift it. A script call, or a
    /// model-issued one yielded without a turn, reports the counter's
    /// value at dispatch.
    fn prepare_tool_call(
        &mut self,
        id: ChainIndex,
        alias: &str,
        args: serde_json::Value,
        site: CallSite,
    ) -> Result<ToolCallDispatch> {
        match self.tool_route(id, alias, &site)? {
            ToolRoute::Adhoc(binding) => self.prepare_adhoc_call(id, &binding, args, &site),
            ToolRoute::Catalog => {
                self.prepare_catalog_call(id, alias, args, site)?;
                Ok(ToolCallDispatch::Issued)
            }
        }
    }

    /// Issues a catalog call: the one-time counts install, then the tool
    /// resolved - a model call's wire name against the run's offering, a
    /// script call's canonical id against the offering and then the full
    /// catalog, the id then standing as the alias of a tool the run could
    /// not offer (the section's effective scope shapes what the model is
    /// offered, and the author's own script is not the model, so the scope
    /// does not gate it); then the attempt counted under the tool's id,
    /// the call's identity forked from the chain's access, and the issued
    /// effect, whose origin names the model as caller for a `Model` or
    /// `Batch` site and the script for a `Script` site. The answer's
    /// rules - the model-issued body under the call id, else the script
    /// body classified by the binding's declared output kind - and a
    /// `Batch` site's slot are the continuation's, applied when the answer
    /// lands.
    fn prepare_catalog_call(
        &mut self,
        id: ChainIndex,
        alias: &str,
        args: serde_json::Value,
        site: CallSite,
    ) -> Result<()> {
        let report = ScriptReport {
            turn: self.call_turn(id, &site),
        };
        let counts = self.section_call_counts(id)?;
        let ctx = &self.chains[id.index()].ctx;
        let tool_set = ctx.tool_set();
        // A model reaches only the wire names a round advertised; a script
        // names a catalog tool by its canonical id, and only a script falls
        // back to a catalog tool the run could not offer.
        let binding = if site.call_id().is_some() {
            tool_set.wire_binding(alias)
        } else if ToolId::parse(alias).is_ok() {
            tool_set
                .offered_binding(alias)
                .or_else(|| ctx.catalog_binding(alias))
        } else {
            None
        }
        .cloned();
        let Some(binding) = binding else {
            return Err(unbound_tool_call(tool_set, alias));
        };
        // The counts seed from the section's effective scope; a tool
        // outside it must still be seeded here, because the increment
        // errors on an unseeded id. The attempt counts at dispatch -
        // before the tool runs, so a cancelled dispatch still counts.
        let counted = binding.id().to_string();
        counts.ensure(&counted)?;
        counts.increment(&counted)?;
        // The call runs under an identity of its own, forked from the
        // chain's, so two calls in flight at once never share one sequence
        // of claims. A refused fork fails the call at its caller, and the
        // counts already taken stay taken.
        let prompt = self.prompt();
        let chain = &self.chains[id.index()];
        let access = detail::access_spawn(
            chain.access()?,
            prompt_origin(&prompt, &binding.id().to_string(), chain.blocks(&prompt)),
        )
        .map_err(Error::store)?;
        let exec = detail::access_id(&access);
        let origin = ToolCallOrigin::new(
            chain.ctx.execution(),
            chain.section_name(),
            match &site {
                CallSite::Script { .. } => ToolCaller::Script,
                CallSite::Model { .. } | CallSite::Batch { .. } => ToolCaller::Model,
            },
        );
        let effect = Effect::ToolCall {
            tool: binding.id().clone(),
            alias: binding.alias().to_owned(),
            args,
            origin,
            access: Arc::new(access),
        };
        let resume = Continuation::ToolCall(ToolCallContinuation {
            binding,
            report,
            site,
            exec,
        });
        self.issue(id, effect, resume);
        Ok(())
    }
}
