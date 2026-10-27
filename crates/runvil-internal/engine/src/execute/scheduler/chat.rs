//! The `chat` arm: one stateless tool-capable model round yielded by a
//! section VM.
//!
//! Dispatch first refuses an empty list. It then resolves the round's
//! binding and the section entry's frozen sets - merged from the section's
//! offers and attached to the list by the first round to reach the merge,
//! then reused by every later round - projects the Static Set's schemas,
//! prechecks the projected conversation, with room kept for the reply,
//! against the model's context window (counting from the provider's usage
//! for the chain's last measured round when the request extends it), and
//! issues the single gateway round as a `Chat` effect - the same effect a
//! nested `infer` issues, over the section's `messages` and those schemas.
//! A first round whose offers name a catalog tool issues a `Render`
//! effect before the merge instead, and dispatches again under the same
//! binding once the render's answer is merged
//! ([`Scheduler::accept_render`]).
//! The driver classifies the answered completion into the round's answer
//! when it arrives ([`Scheduler::accept_chat`]), emitting the round's
//! events (the turn advance, then `report_model_turn`'s sequence, which
//! ends in a text reply's report, or the tool-call batch after it) through
//! the chain's own task-scoped emitter, and rejecting a tool name
//! outside both frozen sets of the section the chain is parked in. The
//! shim that yielded the round emits nothing; the scheduler owns every
//! event.

use std::sync::atomic::Ordering;

use runvil_model_client::detail::tool_call_arguments;
use runvil_types::event::{ReplyOrigin, lifecycle};
use runvil_types::ids::RoundId;
use runvil_types::metrics::ToolCallEvent;
use runvil_types::tools::ToolInfo;

use crate::execute::context::ReportingHandles;
use crate::execute::run::{Effect, PackToolSets};
use crate::execute::scope::{SectionOffers, offered_packs, prepare_effective_scope, tool_schema};
use crate::execute::turn::{Served, advance_turn, report_model_turn};
use crate::lua::{
    Answer, ChatOutcome, ChatResult, MessageList, OverflowReason, RenderedTools, ToolSet,
    output_reserve, precheck, project_messages, resolve_model_binding, wire_name_of,
};
use crate::model::{Completion, CompletionErrorKind, CompletionResult, ModelBinding, ToolCall};
use crate::{Error, Result};

use super::dispatch::Dispatched;
use super::{ChainIndex, Continuation, Scheduler};

/// The answer for a round refused as too large by `reason`'s gate, before
/// or by the provider: no round ran, so no finish reason is present and
/// `turn` is the unadvanced counter.
fn overflow_result(reason: OverflowReason, turn: u32) -> ChatResult {
    ChatResult {
        outcome: ChatOutcome::Overflow(reason),
        finish_reason: None,
        turn,
    }
}

impl Scheduler {
    /// Dispatches a `chat` request: one tool-capable model round over the
    /// author's message list. An issued round parks the chain in the
    /// effect table; a precheck overflow answers the round on the spot
    /// with the overflow flag. Its preparation fails on the empty list,
    /// the binding, the scope, the projection, or the client.
    pub(super) fn dispatch_chat(
        &mut self,
        id: ChainIndex,
        list: &MessageList,
        binding: Option<ModelBinding>,
    ) -> Result<()> {
        match self.prepare_chat(id, list, binding) {
            Ok(Dispatched::Issued) => Ok(()),
            Ok(Dispatched::Answered(answer)) => self.answer_inline(id, answer),
            Err(error) => self.answer_inline(id, Answer::Chat(Err(error))),
        }
    }

    /// The fallible half of chat dispatch: the emptiness check, the
    /// binding (the handle whose `loop` ran, when one did, else the
    /// section's current model), the section entry's frozen sets and the
    /// Static Set's schemas, the per-dispatch projection, the context
    /// precheck, and the issued effect: the round, or the render before
    /// it. Only a precheck overflow is answered without leaving, with the
    /// overflow flag. The first round to reach the merge freezes the sets
    /// and attaches them before the projection and the precheck, so a
    /// refused first round still fixes them; a round refused earlier
    /// attaches nothing, and so does a failed merge. When that round's
    /// offers name a catalog tool, it issues the render instead and parks
    /// the chain on it with the binding, and the render's answer merges and
    /// dispatches the round again ([`Scheduler::accept_render`]), so the
    /// round is projected, prechecked, numbered, and recorded once. The
    /// send is recorded on the list only once the precheck passes, so a
    /// refused round leaves the list's last send in place.
    fn prepare_chat(
        &mut self,
        id: ChainIndex,
        list: &MessageList,
        binding: Option<ModelBinding>,
    ) -> Result<Dispatched> {
        let messages = list.to_records();
        if messages.is_empty() {
            return Err(Error::lua_message("messages must not be empty"));
        }
        let chain = &self.chains[id];
        let section = chain.section_name().to_owned();
        let frame = chain.frame()?;
        let vm = frame.vm()?;
        let binding = if let Some(binding) = binding {
            binding
        } else {
            let current = resolve_model_binding(chain.ctx.models(), vm.model_runtime())?;
            current.ok_or_else(|| Error::ModelRequired {
                section: section.clone(),
            })?
        };
        let handles = frame.reporting_handles().clone();
        let emitter = handles.emitter;
        // The section entry's first round freezes its sets, and every offer
        // raises after that, so each later round sends the same tools.
        let rendered = if let Some(rendered) = list.rendered_tools() {
            rendered
        } else {
            let set = chain.ctx.tool_set();
            let offers = SectionOffers::read(set, vm)?;
            let offered = offered_packs(&offers);
            if !offered.is_empty() {
                let effect = Effect::Render {
                    entry: frame.entry_id().to_owned(),
                    binding: binding.clone(),
                    offered,
                };
                self.issue(id, effect, Continuation::Render(binding));
                return Ok(Dispatched::Issued);
            }
            let frozen = prepare_effective_scope(
                &offers,
                offered,
                set.catalog(),
                emitter.as_ref(),
                &section,
            )?;
            list.attach_rendered_tools(frozen);
            list.rendered_tools()
                .ok_or(Error::internal("a list holds the sets attached to it"))?
        };
        let tools = rendered
            .static_set
            .iter()
            .map(tool_schema)
            .collect::<Result<Vec<_>>>()?;
        // The projection failure reports a failed turn before its call-site
        // error resumes into Lua, the loop's precedent.
        let conversation = match project_messages(&messages) {
            Ok(conversation) => conversation,
            Err(error) => {
                emitter.report(&section, lifecycle::MODEL_TURN_FAILED);
                return Err(Error::from(error));
            }
        };
        let context = binding.context();
        let reserve = output_reserve(context, binding.invocation().max_tokens);
        let chain = &mut self.chains[id];
        // The pre-dispatch precheck: a request that leaves the reply no
        // room never leaves. The refusal is the round's answer - the
        // overflow flag - and is observed as a failed turn, exactly as the
        // loop reported it. The count starts from the provider's numbers
        // for the chain's last measured round when this request extends it
        // and goes to the same model.
        let anchor = chain.anchor.measured(binding.id());
        if let Err(reason) = precheck(&conversation, context, reserve, anchor) {
            emitter.report(&section, lifecycle::MODEL_TURN_FAILED);
            return Ok(Dispatched::Answered(Answer::Chat(Ok(Box::new(
                overflow_result(reason, handles.turns.load(Ordering::Relaxed)),
            )))));
        }
        chain
            .anchor
            .sending(binding.id().clone(), conversation.clone());
        let round = self.number_round(ReplyOrigin::Chat);
        let (after, keep) = list.commit(round.id, binding.id().name(), &conversation);
        let effect = Effect::Chat {
            options: binding.completion_options(),
            binding,
            messages: conversation,
            after,
            keep,
            tools,
            round,
        };
        self.issue(id, effect, Continuation::Chat(round.id));
        Ok(Dispatched::Issued)
    }

    /// Applies a section entry's render: merges the rendered tools into
    /// the entry's frozen sets, attaches them, and dispatches the round
    /// that issued the render under `binding`, which issues its `Chat`
    /// effect or answers the chain inline. A failed merge answers the
    /// parked round with its error and attaches nothing.
    pub(super) fn accept_render(
        &mut self,
        id: ChainIndex,
        binding: ModelBinding,
        rendered: Vec<PackToolSets<ToolInfo>>,
    ) -> Result<()> {
        match self.merge_render(id, rendered) {
            Ok(list) => self.dispatch_chat(id, &list, Some(binding)),
            Err(error) => self.answer_inline(id, Answer::Chat(Err(error))),
        }
    }

    /// The fallible half of a render's application: the merge, over the
    /// offers read back from the parked frame's VM, which runs no Lua
    /// while the chain is parked, so they are the offers the render was
    /// asked about, and the attach to the VM's one list, the list the
    /// round named. Returns that list.
    ///
    /// # Errors
    /// Returns the merge's error, or [`Error::Internal`] when the parked
    /// chain has lost its frame.
    fn merge_render(
        &self,
        id: ChainIndex,
        rendered: Vec<PackToolSets<ToolInfo>>,
    ) -> Result<MessageList> {
        let chain = &self.chains[id];
        let frame = chain.frame()?;
        let vm = frame.vm()?;
        let set = chain.ctx.tool_set();
        let frozen = prepare_effective_scope(
            &SectionOffers::read(set, vm)?,
            rendered,
            set.catalog(),
            frame.reporting_handles().emitter.as_ref(),
            chain.section_name(),
        )?;
        let list = vm.messages().clone();
        list.attach_rendered_tools(frozen);
        Ok(list)
    }

    /// Classifies one arrived chat round into the chain's answer, emitting
    /// the round's events through the chain's task-scoped emitter, each
    /// content event stamped with `round`.
    ///
    /// A failure classifies as `ArrivedRound::failed` describes. A served
    /// completion advances the turn and reports through
    /// `report_model_turn`, a text reply's report included; a tool-call
    /// batch reports after it, and a requested tool outside both frozen
    /// sets of the section the chain is parked in fails the call as out of
    /// scope after a failed-tool-call observation. A chain has one parked
    /// round, and its frame's sets are
    /// read before the round settles, so they are that round's sets. A
    /// round that reported usage becomes the chain's measurement for the
    /// next dispatch's precheck to the same model, whatever the round's
    /// outcome; any other round only releases the messages it sent.
    ///
    /// # Errors
    /// Returns [`Error::Internal`] when the parked chain has lost its frame
    /// or its section's frozen sets.
    pub(super) fn accept_chat(
        &mut self,
        id: ChainIndex,
        round: RoundId,
        result: Result<Box<Completion>>,
    ) -> Result<Answer<Error>> {
        let chain = &self.chains[id];
        let round = ArrivedRound {
            id: round,
            section: chain.section_name().to_owned(),
            reporting: chain.frame()?.reporting_handles().clone(),
        };
        let completion = match result {
            Ok(completion) => completion,
            Err(error) => {
                let failure = round.failed(error);
                self.chains[id].anchor.settle(None);
                return Ok(Answer::Chat(failure));
            }
        };
        // A round trip that produced a reply is a turn, whether the reply
        // is text or a batch of tool calls.
        let turn = advance_turn(&round.reporting.turns);
        let (outcome, served) = round.served(*completion, turn);
        let result = match outcome {
            CompletionResult::Text(text) => Ok(ArrivedRound::text_reply(&served, text, turn)),
            CompletionResult::ToolCalls(calls) => {
                // The sets are frozen before the round is issued; their
                // absence is a scheduler fault, never an author-visible
                // out-of-scope refusal.
                let vm = chain.frame()?.vm()?;
                let frozen = vm.rendered_tools().ok_or(Error::internal(
                    "a parked chat round's section holds its frozen sets",
                ))?;
                let scope = Scope {
                    frozen: &frozen,
                    tool_set: chain.ctx.tool_set(),
                };
                round.tool_calls(&served, turn, &calls, &scope)
            }
            // `CompletionResult` is `#[non_exhaustive]` across the crate
            // boundary: an outcome this build does not recognize can be
            // neither resumed nor promoted to an answer.
            _ => Err(Error::internal("unrecognized completion outcome")),
        };
        let usage = served
            .metrics
            .as_ref()
            .and_then(|metrics| metrics.usage.as_ref());
        self.chains[id].anchor.settle(usage);
        Ok(Answer::Chat(result.map(Box::new)))
    }
}

/// What the scope gate reads for one arrived round: the frozen sets of
/// the section the chain is parked in, and the run's tool set.
struct Scope<'a> {
    frozen: &'a RenderedTools,
    tool_set: &'a ToolSet,
}

impl Scope<'_> {
    /// Whether `name` is a catalog tool's wire name, whatever the section
    /// offered. An adhoc tool's name never counts, because the error's
    /// hint names a catalog tool.
    fn global_exists(&self, name: &str) -> bool {
        self.tool_set
            .catalog()
            .tools()
            .iter()
            .any(|info| wire_name_of(info) == name)
    }
}

/// One arrived round's reporting context: the round's id, the chain's
/// section label, and its reporting handles, whose turn counter is a task
/// chain's own, so its turn numbers, and `tasks.status`'s `turns`, count
/// only its own rounds.
#[derive(Debug)]
struct ArrivedRound {
    id: RoundId,
    section: String,
    reporting: ReportingHandles,
}

impl ArrivedRound {
    /// Classifies a round that produced no completion. A `ContextOverflow`
    /// failure is the overflow answer under a failed turn. An `EmptyReply`
    /// is a completed round with the reply absent - the turn advances and
    /// completes - because whether it is the model's clean exit or a
    /// failure depends on the rounds before it, which only the loop's state
    /// machine (`models_loop`) knows; no debug capture fires because the
    /// failed completion has no request/response bodies to record. Every
    /// other failure is a failed turn and the call's error.
    fn failed(&self, error: Error) -> std::result::Result<Box<ChatResult>, Error> {
        match error {
            Error::Completion(error) if error.kind() == CompletionErrorKind::ContextOverflow => {
                self.reporting
                    .emitter
                    .report(&self.section, lifecycle::MODEL_TURN_FAILED);
                Ok(Box::new(overflow_result(
                    OverflowReason::Provider,
                    self.reporting.turns.load(Ordering::Relaxed),
                )))
            }
            Error::Completion(error) if error.kind() == CompletionErrorKind::EmptyReply => {
                let turn = advance_turn(&self.reporting.turns);
                self.reporting
                    .emitter
                    .report(&self.section, lifecycle::MODEL_TURN_COMPLETED);
                Ok(Box::new(ChatResult {
                    outcome: ChatOutcome::Empty(Some(error.message().to_owned())),
                    finish_reason: error.finish_reason().map(str::to_owned),
                    turn,
                }))
            }
            error => {
                self.reporting
                    .emitter
                    .report(&self.section, lifecycle::MODEL_TURN_FAILED);
                Err(error)
            }
        }
    }

    /// Reports a served completion's round-level events through
    /// [`report_model_turn`] and dissolves the completion into its outcome
    /// and the metadata the answer arms carry beside it.
    fn served(&self, completion: Completion, turn: u32) -> (CompletionResult, Served) {
        report_model_turn(
            &self.reporting.emitter,
            &self.section,
            turn,
            self.id,
            completion,
            ReplyOrigin::Chat,
        )
    }

    /// Builds a text reply's answer. Its `assistant_reply` content report,
    /// with the truncation observation on a `length` finish, fired from
    /// [`report_model_turn`].
    fn text_reply(served: &Served, text: String, turn: u32) -> ChatResult {
        ChatResult {
            outcome: ChatOutcome::Reply(text),
            finish_reason: served.finish_reason.clone(),
            turn,
        }
    }

    /// Reports a tool-call batch and builds its answer, after the scope
    /// gate: every requested name must be one either frozen set holds,
    /// else the call fails as out of scope under a failed tool-call
    /// observation. Each call's event names the tool its name binds and
    /// that tool's vendor. The batch resumes unexecuted; the shim
    /// dispatches each call, and the discovery route answers a Dynamic
    /// Set name with text.
    fn tool_calls(
        &self,
        served: &Served,
        turn: u32,
        calls: &[ToolCall],
        scope: &Scope<'_>,
    ) -> Result<ChatResult> {
        let events: Vec<ToolCallEvent> = calls
            .iter()
            .map(|call| {
                let tool = scope
                    .frozen
                    .binding(call.name())
                    .map(|(binding, _)| binding.id());
                let vendor = tool.and_then(|id| scope.tool_set.vendor_for(id));
                ToolCallEvent::new(call.id(), call.name(), tool_call_arguments(call).clone())
                    .with_tool(tool.cloned())
                    .with_vendor(vendor.map(str::to_owned))
            })
            .collect();
        self.reporting.emitter.assistant_tool_calls(
            &self.section,
            turn,
            self.id,
            &served.model,
            &events,
        );
        if let Some(rogue) = calls
            .iter()
            .find(|call| scope.frozen.binding(call.name()).is_none())
        {
            self.reporting
                .emitter
                .report(&self.section, lifecycle::TOOL_CALL_FAILED);
            return Err(Error::OutOfScopeToolCall {
                name: rogue.name().to_owned(),
                global_exists: scope.global_exists(rogue.name()),
                in_scope: scope
                    .frozen
                    .static_set
                    .iter()
                    .map(|binding| binding.alias().to_owned())
                    .collect(),
            });
        }
        Ok(ChatResult {
            outcome: ChatOutcome::ToolCalls(events),
            finish_reason: served.finish_reason.clone(),
            turn,
        })
    }
}
