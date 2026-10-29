//! The adhoc arm of the `tool_call` arm: a call routed to an adhoc tool,
//! answered with its handler, and the `adhoc_tool_done` request that
//! closes it.
//!
//! An adhoc tool call is a two-yield handshake. The `tool_call` answer is
//! [`ToolCallOutcome::Adhoc`], which hands the shim the handler, and the
//! frame opens an [`AdhocCall`] for it. The shim runs the handler inside the
//! chain's block coroutine, so the handler's own suspending calls are
//! ordinary requests of this chain, then yields `adhoc_tool_done`. That arm
//! closes the innermost open call and reports it: the succeeded or failed
//! observation and, on success, the `ToolResult` under the tool's id and the
//! call id recorded at dispatch, trusted unless the handler fetched a
//! listing while the call was open. An adhoc call never issues leaf work of
//! its own.

use std::sync::Arc;

use runvil_types::event::lifecycle;
use runvil_types::tools::OutputTrust;

use crate::execute::section_context::AdhocCall;
use crate::lua::{AdhocToolOutcome, Answer, ScriptReport, ToolBinding, ToolCallOutcome};
use crate::{Error, Result};

use super::super::dispatch::Dispatched;
use super::super::{CallSite, ChainOps};

impl ChainOps<'_> {
    /// Answers a call routed to the adhoc tool `binding` binds: the
    /// one-time counts install, the attempt counted under the tool's id,
    /// the call opened on the frame, then the handler's answer, settled on
    /// the driver thread without an effect.
    pub(super) fn prepare_adhoc_call(
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
        Ok(Dispatched::Answered(Answer::ToolCallResult(Ok(
            ToolCallOutcome::Adhoc {
                alias: binding.alias().to_owned(),
                args,
            },
        ))))
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
    /// Returns [`Error::Internal`] when the chain has no frame or no open
    /// adhoc call, or no suspended block to take the answer.
    pub(in crate::execute::scheduler) fn dispatch_adhoc_tool_done(
        &mut self,
        outcome: AdhocToolOutcome,
    ) -> Result<()> {
        let chain = &mut *self.chain;
        let emitter = Arc::clone(chain.ctx.emitter());
        let section = chain.section_name().to_owned();
        let call = chain.frame_mut()?.pop_adhoc_call().ok_or(Error::internal(
            "an adhoc tool's completion closes the call that opened it",
        ))?;
        let answer = match outcome {
            AdhocToolOutcome::Returned(text) => {
                emitter.report(&section, lifecycle::TOOL_CALL_SUCCEEDED);
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
                    Some(&call.tool),
                    None,
                    &text,
                    trust,
                );
                Ok(ToolCallOutcome::Plain(text))
            }
            AdhocToolOutcome::BadReturn(error) => {
                emitter.report(&section, lifecycle::TOOL_CALL_FAILED);
                Err(Error::from(error))
            }
            AdhocToolOutcome::Raised => {
                emitter.report(&section, lifecycle::TOOL_CALL_FAILED);
                Ok(ToolCallOutcome::Plain(String::new()))
            }
        };
        self.answer(Answer::ToolCallResult(answer))
    }
}
