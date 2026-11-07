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
//! the model names directly, by the discovery route; any other tool has
//! its attempt counted and is issued as a `ToolCall` effect under the
//! binding its route holds, carrying an identity forked from the chain's
//! access, whose answer the driver applies through the shared dispatch
//! body.
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
//! An adhoc tool call is a two-yield handshake. The `tool_call` answer is
//! [`ToolCallOutcome::Adhoc`], which hands the shim the handler, and the
//! frame opens an [`AdhocCall`] for it. The shim runs the handler inside
//! the chain's block coroutine, so the handler's own suspending calls are
//! ordinary requests of this chain, then yields `adhoc_tool_done`, which
//! closes the innermost open call and reports it. An adhoc call never
//! issues leaf work of its own.
//!
//! A discovery call is decided in full before any preparer runs (the
//! freeze-before-preparer guarantee). A direct call to a Dynamic Set tool,
//! and every malformed discovery call, is answered with refusal text that
//! says what to do next, so the loop goes on. A listing is untrusted,
//! because its descriptions come from the catalog Packs, so it is wrapped
//! with the run's nonce; a refusal is Engine text and passes verbatim. A
//! `GetDynamicTools` call counts under `dynamic/search` whatever its
//! outcome, and any other refused call under `dynamic/call`.

use std::borrow::Cow;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use regex::Regex;
use runvil_types::event_lifecycle;
use runvil_types::tools::{OutputTrust, ToolId};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::execute::run::{Effect, ToolCallOrigin, ToolCaller};
use crate::execute::scope::PATTERN_LIMIT;
use crate::execute::section_context::AdhocCall;
use crate::lua::{
    AdhocToolOutcome, Answer, DiscoveryTool, ModelCall, OfferMode, RenderedTools, ScriptReport,
    ToolBinding, ToolCallCounts, ToolCallOutcome, ToolCallRecord, ToolSet, current_dynamic_set,
    current_static_set,
};
use crate::{Error, Result, error};

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
    let ids: Vec<String> = ids.into_iter().map(ToString::to_string).collect();
    error::unbound_tool(name, &ids)
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

/// The longest description a search result keeps whole, in characters.
const DESCRIPTION_LIMIT: usize = 200;

/// What one discovery call comes to.
#[derive(Debug)]
enum Discovered {
    /// A listing, answered as untrusted text.
    Listing(String),
    /// A refusal, answered as trusted text.
    Refused(String),
    /// A `CallDynamicTool` call that reaches the Dynamic Set tool it names,
    /// with the inner arguments.
    Reached(ToolBinding, Value),
}

/// One Dynamic Set tool as a `GetDynamicTools` answer shows it.
#[derive(Debug, Serialize)]
struct Listed<'a> {
    tool: &'a str,
    description: Cow<'a, str>,
    /// The tool's input schema, sent only by the `toolName` branch: a
    /// search result leaves it out, so a listing stays small.
    #[serde(rename = "inputSchema", skip_serializing_if = "Option::is_none")]
    input_schema: Option<&'a Value>,
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
        model: Option<ModelCall>,
    ) -> Result<()> {
        let site = model.map_or(CallSite::Script, |ModelCall { call_id, turn }| {
            CallSite::Model { call_id, turn }
        });
        match self.prepare_tool_call(alias, args, site) {
            // An issued call is parked on the effect table.
            Ok(Dispatched::Issued) => Ok(()),
            Ok(Dispatched::Answered(answer)) => self.answer(Ok(answer)),
            Err(error) => self.answer(Err(error)),
        }
    }

    /// Dispatches a `tool_calls` request, the unanswered calls of one
    /// model round, under the batch rule in the module doc: each catalog
    /// call is issued as a `CallSite::Batch` under the round's `turn` and
    /// its own slot, and the slots park on the chain.
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

    /// Dispatches an `adhoc_tool_done` request: closes the chain's
    /// innermost open adhoc tool call and answers it. A returned text
    /// reports `TOOL_CALL_SUCCEEDED` and a trusted `ToolResult` - the
    /// prompt author wrote the handler, so its output passes verbatim -
    /// under the tool's id, no vendor, the call id (or none for a script
    /// call), and the turn recorded at dispatch, then resumes as the text.
    /// A call marked as having fetched a listing while it was open instead
    /// returns its text wrapped with the run's nonce and reports it
    /// untrusted. A rejected return reports `TOOL_CALL_FAILED` and resumes
    /// as its error. A raise reports `TOOL_CALL_FAILED` and resumes empty:
    /// the shim raises the handler's own value in its place.
    ///
    /// # Errors
    /// Returns an `Internal` failure when the chain has no frame or no open
    /// adhoc call, or no suspended block to take the answer.
    pub(in crate::execute::scheduler) fn dispatch_adhoc_tool_done(
        &mut self,
        outcome: AdhocToolOutcome,
    ) -> Result<()> {
        let chain = &mut *self.chain;
        let emitter = Arc::clone(chain.ctx.emitter());
        let section = chain.section_name().to_owned();
        let call = chain.frame_mut()?.pop_adhoc_call().ok_or(error::internal(
            "an adhoc tool's completion closes the call that opened it",
        ))?;
        let answer = match outcome {
            AdhocToolOutcome::Returned(text) => {
                emitter.report(&section, event_lifecycle::TOOL_CALL_SUCCEEDED);
                let (text, trust) = if call.listing {
                    (chain.ctx.nonce().wrap(&text), OutputTrust::Untrusted)
                } else {
                    (text, OutputTrust::Trusted)
                };
                emitter.tool_result(
                    &section,
                    call.report.turn,
                    call.call_id.as_deref().unwrap_or(""),
                    &call.alias,
                    &call.tool,
                    None,
                    &text,
                    trust,
                );
                Ok(ToolCallOutcome::Plain(text))
            }
            AdhocToolOutcome::BadReturn(failure) => {
                emitter.report(&section, event_lifecycle::TOOL_CALL_FAILED);
                Err(failure)
            }
            AdhocToolOutcome::Raised => {
                emitter.report(&section, event_lifecycle::TOOL_CALL_FAILED);
                Ok(ToolCallOutcome::Plain(String::new()))
            }
        };
        self.answer(answer.map(Answer::ToolCallResult))
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
    /// Returns an `UnboundTool` failure for a name that resolves to no
    /// tool, and a `ModelOnlyTool` failure for a script's call of a
    /// catalog tool marked model-only.
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
            let adhoc = vm.adhoc_tools().by_id(binding.id())?;
            return Ok(adhoc.map_or(ToolRoute::Catalog(binding), ToolRoute::Adhoc));
        }
        let tool = ToolId::parse(name).ok();
        if let Some(tool) = &tool
            && let Some(binding) = vm.adhoc_tools().by_id(tool)?
        {
            return Ok(ToolRoute::Adhoc(binding));
        }
        let Some(info) = tool.and_then(|tool| tool_set.catalog().get(&tool)) else {
            return Err(unbound_tool_call(tool_set, name));
        };
        // A script cannot know the arguments a render shapes for a model,
        // so a model-only tool refuses the call while the catalog keeps
        // naming it.
        if info.model_only_call {
            return Err(error::model_only_tool(name, &info.id.to_string()));
        }
        Ok(ToolRoute::Catalog(ToolBinding::from_info(
            &info.id.to_string(),
            info,
        )))
    }

    /// The turn every report of a call at `site` carries: the requesting
    /// round's turn for a model-issued call, else the chain's turn
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
                    || vm.adhoc_tools().by_id(&tool)?.is_some();
                if seeded {
                    ids.push(tool);
                }
            }
            ids
        } else {
            let (offers, adhoc) = (vm.offers(), vm.adhoc_tools());
            let static_tools = current_static_set(tool_set, offers, adhoc)?;
            let dynamic_tools = current_dynamic_set(tool_set, offers, adhoc, &static_tools)?;
            static_tools
                .iter()
                .chain(&dynamic_tools)
                .map(|tool| tool.id().clone())
                .collect()
        };
        chain.frame_mut()?.tool_call_counts(&ids)
    }

    /// Fills `slot` of the batch the chain is parked on with a landed
    /// call's answer: a plain text as the text, a failure as the failure,
    /// and any other outcome, which a model-issued call never resumes
    /// with, as an internal error. Once no slot is open the chain is
    /// answered.
    ///
    /// # Errors
    /// Returns an `Internal` failure when the chain holds no such slot,
    /// which only an answer the scheduler should have orphaned produces,
    /// or when the full batch finds no suspended block to answer.
    pub(super) fn fill_batch_slot(&mut self, slot: usize, answer: Result<Answer>) -> Result<()> {
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
            ) => Err(error::internal(
                "a batched call lands as plain text or a failure",
            )),
        };
        let open = match &mut self.chain.parked {
            Some(Parked::Batch(batch)) => batch.get_mut(slot).filter(|open| open.is_none()),
            _ => None,
        }
        .ok_or(error::internal(
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
    /// that runs model rounds cannot shift it. A script call reports the
    /// counter's value at dispatch.
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

    /// Answers a call routed to the adhoc tool `binding` binds: the
    /// one-time counts install, the attempt counted under the tool's id,
    /// the call opened on the frame, then the handler's answer, settled on
    /// the driver thread without an effect.
    fn prepare_adhoc_call(
        &mut self,
        binding: &ToolBinding,
        args: serde_json::Value,
        site: &CallSite,
    ) -> Result<Dispatched> {
        let report = ScriptReport {
            turn: self.call_turn(site),
        };
        let counts = self.section_call_counts()?;
        // The attempt counts at dispatch, before the handler runs, so a
        // handler that then fails still counts.
        let counted = binding.id().to_string();
        counts.increment(&counted)?;
        self.chain.frame_mut()?.push_adhoc_call(AdhocCall {
            tool: binding.id().clone(),
            alias: binding.alias().to_owned(),
            call_id: site.call_id().map(str::to_owned),
            report,
            listing: false,
        });
        Ok(Dispatched::Answered(Answer::ToolCallResult(
            ToolCallOutcome::Adhoc {
                alias: binding.alias().to_owned(),
                args,
            },
        )))
    }

    /// Answers a model call routed to `binding`, a discovery tool or a
    /// Dynamic Set tool the model named directly. A `CallDynamicTool` call
    /// that reaches its target goes to that tool's preparer. Any other
    /// call is answered inline: the attempt counted, then a listing
    /// reported succeeded and untrusted, wrapped with the run's nonce, or
    /// a refusal reported failed and trusted, its `ToolResult` under the
    /// call's id and `binding`, as a failed model call reports.
    ///
    /// # Errors
    /// Returns an `Internal` failure when the chain's section holds no frozen
    /// sets, which a model call can only reach after the freeze, and the
    /// error of the counts install or of the target's preparer.
    fn prepare_discovery_call(
        &mut self,
        binding: &ToolBinding,
        args: Value,
        site: CallSite,
    ) -> Result<Dispatched> {
        let tool = DiscoveryTool::of(binding.id());
        let frozen =
            self.chain
                .frame()?
                .vm()
                .messages()
                .rendered_tools()
                .ok_or(error::internal(
                    "a model call's section holds its frozen sets",
                ))?;
        let discovered = match tool {
            Some(DiscoveryTool::Search) => search(&frozen, &args)?,
            Some(DiscoveryTool::Call) => reach(&frozen, args),
            None => Discovered::Refused(format!(
                "{name:?} is a dynamic tool; call it through {call} with toolName {name:?}",
                name = binding.alias(),
                call = DiscoveryTool::Call.wire_name(),
            )),
        };
        let (text, trust, boundary) = match discovered {
            Discovered::Reached(target, args) => {
                return self.prepare_reached(target, args, site);
            }
            Discovered::Listing(listing) => (
                listing,
                OutputTrust::Untrusted,
                event_lifecycle::TOOL_CALL_SUCCEEDED,
            ),
            Discovered::Refused(refusal) => (
                refusal,
                OutputTrust::Trusted,
                event_lifecycle::TOOL_CALL_FAILED,
            ),
        };
        let counted = tool.unwrap_or(DiscoveryTool::Call).id();
        let counts = self.section_call_counts()?;
        counts.increment(counted)?;
        let turn = self.call_turn(&site);
        let chain = &*self.chain;
        let text = match trust {
            OutputTrust::Trusted => text,
            _ => chain.ctx.nonce().wrap(&text),
        };
        let emitter = chain.ctx.emitter();
        let section = chain.section_name();
        emitter.report(section, boundary);
        emitter.tool_result(
            section,
            turn,
            site.call_id().unwrap_or_default(),
            binding.alias(),
            binding.id(),
            chain.ctx.tool_set().vendor_for(binding.id()),
            &text,
            trust,
        );
        Ok(Dispatched::Answered(Answer::ToolCallResult(
            ToolCallOutcome::Plain(text),
        )))
    }

    /// Hands a `CallDynamicTool` call's inner `args`, under the call's own
    /// `site`, to the preparer of `target`: the adhoc tool the section's VM
    /// defines under its id, else the catalog tool, issued as a model call.
    fn prepare_reached(
        &mut self,
        target: ToolBinding,
        args: Value,
        site: CallSite,
    ) -> Result<Dispatched> {
        let adhoc = self.chain.frame()?.vm().adhoc_tools().by_id(target.id())?;
        if let Some(adhoc) = adhoc {
            return self.prepare_adhoc_call(&adhoc, args, &site);
        }
        self.prepare_catalog_call(target, args, site)?;
        Ok(Dispatched::Issued)
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
        // canceled dispatch still counts.
        let counted = binding.id().to_string();
        counts.increment(&counted)?;
        let prompt = self.prompt();
        let chain = &*self.chain;
        let frame = chain.frame()?;
        // The list's last send is the requesting round's: a section runs
        // one loop at a time, so no other round commits to the list
        // between a round's answer and the issue of its calls.
        let (caller, model) = match &site {
            CallSite::Script => (ToolCaller::Script, None),
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
        let access = self
            .scope()?
            .spawn(
                chain.access()?,
                prompt_origin(&prompt, &binding.id().to_string(), chain.blocks(&prompt)),
            )
            .map_err(|cause| error::store(&cause))?;
        let exec = access.id();
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

/// What a `GetDynamicTools` call with `args` comes to: the lookup of its
/// `toolName`, else the search its `pattern` makes, else the whole
/// Dynamic Set.
///
/// # Errors
/// Returns an `Internal` failure should a listing not serialize.
fn search(frozen: &RenderedTools, args: &Value) -> Result<Discovered> {
    let tool = DiscoveryTool::Search.wire_name();
    let refused = |refusal| Ok(Discovered::Refused(refusal));
    match text_arg(tool, args, "toolName") {
        Err(refusal) => return refused(refusal),
        Ok(Some(name)) => {
            return match dynamic_target(tool, frozen, name) {
                Ok(binding) => listing(&Listed {
                    tool: binding.alias(),
                    description: Cow::Borrowed(binding.description()),
                    input_schema: Some(binding.schema()),
                }),
                Err(refusal) => refused(refusal),
            };
        }
        Ok(None) => {}
    }
    let pattern = match text_arg(tool, args, "pattern") {
        Err(refusal) => return refused(refusal),
        Ok(None) => None,
        Ok(Some(pattern)) => match compiled(pattern) {
            Ok(regex) => Some(regex),
            Err(refusal) => return refused(refusal),
        },
    };
    let found: Vec<Listed<'_>> = frozen
        .dynamic_set
        .iter()
        .filter(|binding| {
            pattern
                .as_ref()
                .is_none_or(|pattern| pattern.is_match(binding.alias()))
        })
        .map(|binding| Listed {
            tool: binding.alias(),
            description: cut(binding.description()),
            input_schema: None,
        })
        .collect();
    listing(&found)
}

/// What a `CallDynamicTool` call with `args` comes to: the Dynamic Set
/// tool its `toolName` names, reached with its `arguments`, an empty
/// object when absent.
fn reach(frozen: &RenderedTools, mut args: Value) -> Discovered {
    let tool = DiscoveryTool::Call.wire_name();
    let target = match text_arg(tool, &args, "toolName") {
        Ok(Some(name)) => dynamic_target(tool, frozen, name).cloned(),
        Ok(None) => Err(format!(
            "{tool}: toolName is required; pass a name from {search}",
            search = DiscoveryTool::Search.wire_name(),
        )),
        Err(refusal) => Err(refusal),
    };
    let target = match target {
        Ok(target) => target,
        Err(refusal) => return Discovered::Refused(refusal),
    };
    match args.get_mut("arguments").map(Value::take) {
        None => Discovered::Reached(target, Value::Object(Map::new())),
        Some(inner @ Value::Object(_)) => Discovered::Reached(target, inner),
        Some(other) => Discovered::Refused(format!(
            "{tool}: arguments must be an object of the tool's arguments, got {}",
            json_kind(&other)
        )),
    }
}

/// Reads the string `key` of `args`, a call to `tool`: `None` when absent,
/// and the refusal when present as anything but a string.
fn text_arg<'a>(
    tool: &str,
    args: &'a Value,
    key: &str,
) -> std::result::Result<Option<&'a str>, String> {
    match args.get(key) {
        None => Ok(None),
        Some(Value::String(text)) => Ok(Some(text)),
        Some(other) => Err(format!(
            "{tool}: {key} must be a string, got {}",
            json_kind(other)
        )),
    }
}

/// Resolves `name`, the `toolName` of a call to `tool`, in the Dynamic
/// Set, or the refusal for a Static Set name or a name neither set holds.
fn dynamic_target<'a>(
    tool: &str,
    frozen: &'a RenderedTools,
    name: &str,
) -> std::result::Result<&'a ToolBinding, String> {
    match frozen.binding(name) {
        Some((binding, OfferMode::Dynamic)) => Ok(binding),
        Some((_, OfferMode::Static)) => Err(format!(
            "{tool}: {name:?} is in your tool list, not a dynamic tool; call it directly"
        )),
        None => Err(format!(
            "{tool}: no dynamic tool is named {name:?}; call {search} with a pattern or \
             no arguments to find the names",
            search = DiscoveryTool::Search.wire_name(),
        )),
    }
}

/// Compiles a search's `pattern`, or the refusal for one past
/// [`PATTERN_LIMIT`] characters or that is no regex, naming the parser's
/// reason, the last line of its report.
fn compiled(pattern: &str) -> std::result::Result<Regex, String> {
    let tool = DiscoveryTool::Search.wire_name();
    let length = pattern.chars().count();
    if length > PATTERN_LIMIT {
        return Err(format!(
            "{tool}: pattern is {length} characters, over the limit of \
             {PATTERN_LIMIT}; shorten it"
        ));
    }
    Regex::new(pattern).map_err(|error| {
        let report = error.to_string();
        let reason = report.lines().next_back().unwrap_or_default().trim();
        let reason = reason.strip_prefix("error: ").unwrap_or(reason);
        format!(
            "{tool}: pattern {pattern:?} is not a valid regex: {reason}; fix it, or \
             omit it to list every tool"
        )
    })
}

/// `description` whole, or its first [`DESCRIPTION_LIMIT`] characters and
/// `... [truncated]` when it is longer.
fn cut(description: &str) -> Cow<'_, str> {
    match description.char_indices().nth(DESCRIPTION_LIMIT) {
        Some((end, _)) => Cow::Owned(format!("{}... [truncated]", &description[..end])),
        None => Cow::Borrowed(description),
    }
}

/// A listing's text: `listed` as compact JSON.
///
/// # Errors
/// Returns an `Internal` failure should `listed` not serialize.
fn listing(listed: &impl Serialize) -> Result<Discovered> {
    serde_json::to_string(listed)
        .map(Discovered::Listing)
        .map_err(|_| error::internal("a discovery listing serializes"))
}

/// The JSON type of `value`, for a refusal.
fn json_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}
