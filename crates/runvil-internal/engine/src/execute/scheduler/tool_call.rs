//! The `tool_call` arm: one dispatched tool call yielded by a section VM,
//! script-initiated (`tools.call`) or model-issued (the loop shim, with the
//! model's `call_id`), plus the `adhoc_tool_done` arm that closes an adhoc
//! tool call.
//!
//! Each call resolves to its binding once, by `tool_route`, on the driver
//! thread before any leaf work, from the call's `CallSite`, which names who
//! asked. A model call's wire name resolves in the frozen sets of the
//! current frame's section, and a script call's id among the section's
//! adhoc tools or in the run catalog, under the id itself. An adhoc tool
//! is answered with its handler; a discovery tool, or a Dynamic Set tool
//! the model names directly, by the discovery route in the `discovery`
//! child; any other tool has its attempt counted
//! and is issued as a `ToolCall` effect under the binding its route holds,
//! carrying an identity forked from the chain's access, whose answer the
//! driver applies through the shared dispatch body.
//! A model-issued call, alone or batched, always resumes a bound tool with
//! content - a tool's own failure becomes untrusted failure text - and
//! `ToolResult` fires under the id and the turn of the round that requested
//! the call; a script call keeps the raise-at-call-site behavior, and its
//! `ToolResult` fires under no id and the turn at dispatch.
//!
//! The loop sends a round's unanswered calls together as one
//! `tool_calls` request. The arm walks them in the model's order and
//! issues each catalog call at once into a slot of the chain's batch,
//! stopping at the first call whose route is not the catalog. A call that
//! fails to route or to prepare fills its slot with the error and stops
//! the batch too. Each landed answer fills its call's slot, and the
//! chain is answered once no slot is open: every text in call order, or
//! the first failure in call order. Fewer texts than calls tells the
//! loop where the batch stopped, and the loop sends that call alone as
//! an ordinary `tool_call`. The calls in flight run concurrently, each
//! under its own forked identity.
//!
//! The adhoc arm and its two-yield handshake are documented in the `adhoc`
//! child.

#[path = "tool_call-adhoc.rs"]
mod adhoc;
#[path = "tool_call-discovery.rs"]
mod discovery;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use runvil_types::tools::ToolId;
use runvil_vfs::detail;

use crate::execute::run::{Effect, ToolCallOrigin, ToolCaller};
use crate::lua::{
    Answer, DiscoveryTool, OfferMode, ScriptReport, ToolBinding, ToolCallCounts, ToolCallOutcome,
    ToolCallRecord, ToolSet, current_dynamic_set, current_static_set,
};
use crate::{Error, Result};

use super::chain_record::Parked;
use super::dispatch::Dispatched;
use super::{CallSite, ChainOps, Continuation, ToolCallContinuation, prompt_origin};

/// The error for a name that resolves to no tool in the run: the name and
/// the id of every catalog tool, in id order, so the message reads
/// required versus actual. The `tool_call` arm's answer for an unresolved
/// name.
fn unbound_tool_call(tool_set: &ToolSet, name: &str) -> Error {
    let mut ids: Vec<_> = tool_set
        .catalog()
        .tools()
        .iter()
        .map(|info| &info.id)
        .collect();
    ids.sort();
    Error::UnboundToolCall {
        name: name.to_owned(),
        ids: ids.into_iter().map(ToString::to_string).collect(),
    }
}

/// Where one tool call is answered, with the binding it resolved to, so a
/// lone call and a batched one route alike.
#[derive(Debug)]
enum ToolRoute {
    /// An adhoc tool of the chain's section VM, answered with its
    /// handler.
    Adhoc(ToolBinding),
    /// Any other tool, issued as an effect under its binding.
    Catalog(ToolBinding),
    /// A discovery tool, or a Dynamic Set tool the model named directly,
    /// answered by the discovery route.
    Discovery(ToolBinding),
}

impl ChainOps<'_> {
    /// Dispatches a `tool_call` request. An issued bound call parks the
    /// chain in the effect table; an adhoc tool's handler resumes the
    /// chain on the spot. Its preparation fails on an unbound alias or the
    /// counts install.
    pub(super) fn dispatch_tool_call(
        &mut self,
        alias: &str,
        args: serde_json::Value,
        call_id: Option<String>,
        turn: Option<u32>,
    ) -> Result<()> {
        let site = match call_id {
            Some(call_id) => CallSite::Model { call_id, turn },
            None => CallSite::Script { turn },
        };
        match self.prepare_tool_call(alias, args, site) {
            // An issued call is parked on the effect table.
            Ok(Dispatched::Issued) => Ok(()),
            Ok(Dispatched::Answered(answer)) => self.answer(Ok(answer)),
            Err(error) => self.answer(Err(error)),
        }
    }

    /// Dispatches a `tool_calls` request: the unanswered calls of one
    /// model round, in the model's order. Each call before the first
    /// whose route is not the catalog is issued as a `CallSite::Batch`
    /// under the round's `turn` and its own slot, so every catalog call
    /// issues in this step, and the loop sends the call it stopped at
    /// alone. A call that fails to route or to prepare
    /// fills its slot with the error and stops the batch. The slots park
    /// on the chain, which is answered at once when none is open.
    pub(super) fn dispatch_tool_calls(
        &mut self,
        calls: Vec<ToolCallRecord>,
        turn: u32,
    ) -> Result<()> {
        let mut batch = Vec::with_capacity(calls.len());
        for (slot, call) in calls.into_iter().enumerate() {
            let site = CallSite::Batch {
                call_id: call.id,
                turn,
                slot,
            };
            let issued = match self.tool_route(&call.name, &site) {
                Ok(ToolRoute::Adhoc(_) | ToolRoute::Discovery(_)) => break,
                Ok(ToolRoute::Catalog(binding)) => {
                    self.prepare_catalog_call(binding, call.arguments, site)
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
        self.chain.parked = Some(Parked::Batch(batch));
        self.answer_full_batch()
    }

    /// Resolves the call at `site` naming `name` to its binding and route,
    /// the one lookup a lone call and a batch share. A model reaches only
    /// the wire names the frozen sets of the current frame's section hold:
    /// a discovery tool or a Dynamic Set name goes to the discovery route.
    /// A script names a tool by id: an adhoc tool, else any catalog tool,
    /// bound under its id, because the sets shape what the model is
    /// offered and the author's own script is not the model. Either goes
    /// to the adhoc tool the id names, when it is one.
    ///
    /// # Errors
    /// Returns [`Error::UnboundToolCall`] for a name that resolves to no
    /// tool.
    fn tool_route(&self, name: &str, site: &CallSite) -> Result<ToolRoute> {
        let chain = &*self.chain;
        let vm = chain.frame()?.vm();
        let tool_set = chain.ctx.tool_set();
        if site.call_id().is_some() {
            let (binding, set) = vm
                .rendered_target(name)
                .ok_or_else(|| unbound_tool_call(tool_set, name))?;
            if set == OfferMode::Dynamic || DiscoveryTool::of(binding.id()).is_some() {
                return Ok(ToolRoute::Discovery(binding));
            }
            let adhoc = vm.adhoc_tools().binding(&binding.id().to_string())?;
            return Ok(adhoc.map_or(ToolRoute::Catalog(binding), ToolRoute::Adhoc));
        }
        if name.contains('/')
            && let Some(binding) = vm.adhoc_tools().binding(name)?
        {
            return Ok(ToolRoute::Adhoc(binding));
        }
        ToolId::parse(name)
            .ok()
            .and_then(|tool| tool_set.catalog().get(&tool))
            .map(|info| ToolRoute::Catalog(ToolBinding::from_info(&info.id.to_string(), info)))
            .ok_or_else(|| unbound_tool_call(tool_set, name))
    }

    /// The turn every report of a call at `site` carries: the requesting
    /// round's turn when the site names one, else the chain's turn
    /// counter at dispatch.
    fn call_turn(&self, site: &CallSite) -> u32 {
        site.turn()
            .unwrap_or_else(|| self.chain.ctx.turns().load(Ordering::Relaxed))
    }

    /// The chain's section's tool-call counts, which the section's first
    /// call installs, seeded with the ids Lua can see in the section's two
    /// sets, a run catalog tool's or an adhoc tool's: from its frozen sets
    /// once the section froze them, with both discovery tools' ids when
    /// they are rendered, and from its live offers of both kinds before.
    fn section_call_counts(&mut self) -> Result<ToolCallCounts> {
        let chain = &mut *self.chain;
        let vm = chain.frame()?.vm();
        let tool_set = chain.ctx.tool_set();
        let ids = if let Some(frozen) = vm.rendered_ids() {
            let mut ids = Vec::with_capacity(frozen.len());
            for tool in frozen {
                let seeded = DiscoveryTool::of(&tool).is_some()
                    || tool_set.catalog().get(&tool).is_some()
                    || vm.adhoc_tools().binding(&tool.to_string())?.is_some();
                if seeded {
                    ids.push(tool);
                }
            }
            ids
        } else {
            let (runtime, adhoc) = (vm.offers(), vm.adhoc_tools());
            let static_tools = current_static_set(tool_set, runtime, adhoc)?;
            let dynamic_tools = current_dynamic_set(tool_set, runtime, adhoc, &static_tools)?;
            static_tools
                .iter()
                .chain(&dynamic_tools)
                .map(|tool| tool.id().clone())
                .collect()
        };
        let ctx = chain.ctx.clone();
        chain.frame_mut()?.tool_call_counts(&ctx, &ids)
    }

    /// Fills `slot` of the batch the chain is parked on with a landed
    /// call's answer: a plain text as the text, a failure as the failure,
    /// and any other outcome, which a model-issued call never resumes
    /// with, as an internal error. Once no slot is open the chain is
    /// answered.
    ///
    /// # Errors
    /// Returns [`Error::Internal`] when the chain holds no such slot,
    /// which only an answer the scheduler should have orphaned produces,
    /// or when the full batch finds no suspended block to answer.
    pub(super) fn fill_batch_slot(
        &mut self,
        slot: usize,
        answer: Result<Answer<Error>>,
    ) -> Result<()> {
        let landed = match answer {
            Ok(Answer::ToolCallResult(ToolCallOutcome::Plain(text))) => Ok(text),
            Err(error) => Err(error),
            Ok(
                Answer::ToolCallResult(
                    ToolCallOutcome::Structured(_) | ToolCallOutcome::Adhoc { .. },
                )
                | Answer::Infer(_)
                | Answer::Call(_)
                | Answer::Spawn(_)
                | Answer::Timer(_)
                | Answer::JoinAny(_)
                | Answer::Ready(_)
                | Answer::Status(_)
                | Answer::Pending(_)
                | Answer::Concurrency(_)
                | Answer::Note
                | Answer::Cancel
                | Answer::Chat(_)
                | Answer::ToolCallsResult(_)
                | Answer::Store(_),
            ) => Err(Error::internal(
                "a batched call lands as plain text or a failure",
            )),
        };
        let open = match &mut self.chain.parked {
            Some(Parked::Batch(batch)) => batch.get_mut(slot).filter(|open| open.is_none()),
            _ => None,
        }
        .ok_or(Error::internal(
            "a batched call's answer fills an open slot of its chain's batch",
        ))?;
        *open = Some(landed);
        self.answer_full_batch()
    }

    /// Answers the chain parked on a batch once no slot is open: every
    /// text in call order, or the first failure in call order.
    fn answer_full_batch(&mut self) -> Result<()> {
        let Some(Parked::Batch(batch)) = &mut self.chain.parked else {
            return Ok(());
        };
        if batch.iter().any(Option::is_none) {
            return Ok(());
        }
        let answer = std::mem::take(batch)
            .into_iter()
            .flatten()
            .collect::<Result<Vec<String>>>();
        self.answer(answer.map(Answer::ToolCallsResult))
    }

    /// The fallible half of tool-call dispatch, by the call's route: an
    /// adhoc tool (the one-time counts install, the attempt counted, then
    /// the handler's answer, settled on the driver thread), the discovery
    /// route, or any other tool, which `prepare_catalog_call` issues as an
    /// effect.
    ///
    /// Every report the call makes carries the turn of the model round
    /// that requested it, so an adhoc handler earlier in the same batch
    /// that runs model rounds cannot shift it. A script call, or a
    /// model-issued one yielded without a turn, reports the counter's
    /// value at dispatch.
    fn prepare_tool_call(
        &mut self,
        alias: &str,
        args: serde_json::Value,
        site: CallSite,
    ) -> Result<Dispatched> {
        match self.tool_route(alias, &site)? {
            ToolRoute::Adhoc(binding) => self.prepare_adhoc_call(&binding, args, &site),
            ToolRoute::Discovery(binding) => self.prepare_discovery_call(&binding, args, site),
            ToolRoute::Catalog(binding) => {
                self.prepare_catalog_call(binding, args, site)?;
                Ok(Dispatched::Issued)
            }
        }
    }

    /// Issues a call under the `binding` its route resolved: the one-time
    /// counts install, the attempt counted under the tool's id, the call's
    /// identity forked from the chain's access, and the issued effect,
    /// whose origin names the frame's section entry and the caller: the
    /// model, with the requesting round's model, for a `Model` or `Batch`
    /// site, and the script for a `Script` site. The answer's rules - the
    /// model-issued body under the call id, else the script body
    /// classified by the binding's declared output kind - and a `Batch`
    /// site's slot are the continuation's, applied when the answer lands.
    fn prepare_catalog_call(
        &mut self,
        binding: ToolBinding,
        args: serde_json::Value,
        site: CallSite,
    ) -> Result<()> {
        let report = ScriptReport {
            turn: self.call_turn(&site),
        };
        let counts = self.section_call_counts()?;
        // The attempt counts at dispatch - before the tool runs, so a
        // cancelled dispatch still counts.
        let counted = binding.id().to_string();
        counts.increment(&counted)?;
        let prompt = self.prompt();
        let chain = &*self.chain;
        let frame = chain.frame()?;
        // The list's last send is the requesting round's: a section runs
        // one loop at a time, so no other round commits to the list
        // between a round's answer and the issue of its calls.
        let (caller, model) = match &site {
            CallSite::Script { .. } => (ToolCaller::Script, None),
            CallSite::Model { .. } | CallSite::Batch { .. } => {
                (ToolCaller::Model, frame.vm().messages().last_model())
            }
        };
        let mut origin = ToolCallOrigin::new(chain.ctx.execution(), chain.section_name(), caller)
            .with_entry(frame.entry_id());
        origin.model = model;
        // The call runs under an identity of its own, forked from the
        // chain's, so two calls in flight at once never share one sequence
        // of claims. A refused fork fails the call at its caller, and the
        // counts already taken stay taken.
        let access = detail::access_spawn(
            chain.access()?,
            prompt_origin(&prompt, &binding.id().to_string(), chain.blocks(&prompt)),
        )
        .map_err(Error::store)?;
        let exec = detail::access_id(&access);
        let effect = Effect::ToolCall {
            tool: binding.id().clone(),
            alias: binding.alias().to_owned(),
            args,
            origin,
            access: Arc::new(access),
            survives_stop: binding.survives_stop(),
        };
        let resume = Continuation::ToolCall(ToolCallContinuation {
            binding,
            report,
            site,
            exec,
        });
        self.issue(effect, resume);
        Ok(())
    }
}
