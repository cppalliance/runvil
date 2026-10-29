//! The shared tool-dispatch body every executor invokes.
//!
//! [`prepare_dispatch`] is the one place a bound tool's answer composes the
//! succeeded/failed observation, the untrusted nonce wrap, and the
//! `ToolResult` report. It is synchronous and takes the tool's
//! outcome as a value; [`prepare_model_dispatch`] is
//! the same body under the model-issued rule (a tool's own failure resumes
//! as untrusted failure text, and the `ToolResult` fires under the model's
//! call id). Nothing here performs a call: the executor issues the call as
//! an effect, the Harness performs it, and one of the two bodies applies the
//! rules when the answer lands. Keeping both bodies here - the crate every
//! executor already depends on - is what stops dispatch semantics from
//! forking.

use runvil_types::emitter::Emitter;
use runvil_types::event::lifecycle;
use runvil_types::tools::{OutputTrust, ToolError, ToolOutput};
use runvil_types::untrusted::GuardNonce;

use crate::bindings::ToolBinding;
use crate::error::{Error, Result};

/// The run coordinates a script-initiated dispatch reports under.
///
/// [`prepare_dispatch`] fires the `ToolResult` event with them under an
/// empty `tool_call_id`, since only a model-issued call has one. The chain
/// and depth are not part of the report: the emitter
/// stamps every event with its provenance, which already names the task the
/// event belongs to.
#[derive(Debug, Clone, Copy)]
pub struct ScriptReport {
    /// The section's completed model-turn count at dispatch.
    pub turn: u32,
}

/// The run coordinates a model-issued dispatch reports under: the turn the
/// call fired in and the call id the model issued.
#[derive(Debug, Clone)]
pub struct ModelReport {
    /// The section's completed model-turn count at dispatch.
    pub turn: u32,
    /// The model-issued call id the result answers.
    pub call_id: String,
}

/// The resolved outcome of one dispatched tool call: the final content -
/// nonce-wrapped when untrusted, except a script's listing - beside its
/// trust marking. [`prepare_model_dispatch`] reports the pair through the
/// `ToolResult` event under the model's call id; a script-initiated
/// dispatch reads only the content, its report already fired inside
/// [`prepare_dispatch`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDispatch {
    content: String,
    trust: OutputTrust,
    listing: bool,
}

impl ToolDispatch {
    /// The final content: trusted output and a script's listing verbatim,
    /// anything else nonce-wrapped.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn content(&self) -> &str {
        &self.content
    }

    /// The content's trust marking: [`OutputTrust::Trusted`] for trusted
    /// output, [`OutputTrust::Untrusted`] for the nonce-wrapped envelope
    /// and for a script's listing.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn trust(&self) -> OutputTrust {
        self.trust
    }

    /// Whether the content is untrusted text passed verbatim: a script's
    /// call to a tool that describes tools, answered with untrusted
    /// output. Trusted output from such a tool is not a listing.
    #[must_use]
    pub fn listing(&self) -> bool {
        self.listing
    }

    /// Dissolves the outcome into its content.
    #[must_use]
    pub fn into_content(self) -> String {
        self.content
    }
}

/// Applies the dispatch rules to one bound tool call's answer: the shared
/// synchronous body every executor invokes once the tool has spoken.
///
/// The sequence is fixed: the succeeded/failed observation, then the trust
/// rule - a trusted output passes verbatim, anything else is
/// nonce-wrapped before it can reach a model turn or a calling script,
/// except that a script-initiated call to a binding that describes tools
/// receives its untrusted output verbatim, still marked untrusted, as a
/// listing the script reads. It also fires the `ToolResult` event with the
/// final content and `vendor`, the vendor of the tool's Pack.
///
/// `call_result` is the tool's own answer, however the caller obtained it.
///
/// # Errors
/// Returns [`Error::Tool`] when `call_result` is the tool's failure (its
/// typed error retained as the cause).
pub fn prepare_dispatch(
    binding: &ToolBinding,
    vendor: Option<&str>,
    call_result: std::result::Result<ToolOutput, ToolError>,
    nonce: &GuardNonce,
    emitter: &Emitter,
    section: &str,
    script: ScriptReport,
) -> Result<ToolDispatch> {
    dispatch_body(
        binding,
        vendor,
        call_result,
        nonce,
        emitter,
        section,
        Some(script),
    )
}

/// The body of [`prepare_dispatch`], with `script` optional. A model-issued
/// call passes `None`, and [`prepare_model_dispatch`] reports the returned
/// [`ToolDispatch`] under the model's call id.
///
/// # Errors
/// Returns [`Error::Tool`] when `call_result` is the tool's failure (its
/// typed error retained as the cause).
pub(crate) fn dispatch_body(
    binding: &ToolBinding,
    vendor: Option<&str>,
    call_result: std::result::Result<ToolOutput, ToolError>,
    nonce: &GuardNonce,
    emitter: &Emitter,
    section: &str,
    script: Option<ScriptReport>,
) -> Result<ToolDispatch> {
    emitter.report(
        section,
        if call_result.is_ok() {
            lifecycle::TOOL_CALL_SUCCEEDED
        } else {
            lifecycle::TOOL_CALL_FAILED
        },
    );
    let output = call_result.map_err(Error::tool)?;
    // Trust travels with the output: an untrusted result is nonce-wrapped
    // before it can reach the next model turn or the calling script, except
    // a listing a script reads, which passes verbatim. Its `listing` flag
    // lets the Engine mark the innermost open adhoc call, whose result is
    // then wrapped before a model reads it; the untrusted marking keeps
    // this call's result event `trusted` false. Every wrap in the run
    // shares the run's nonce, so identical content yields a byte-identical
    // envelope and KV-cache prefixes stay shared across rounds and fanout
    // arms; the `<`-escaping is what actually blocks a forged close tag, so
    // the reuse costs nothing.
    let (content, trust, listing) = match output.trust() {
        OutputTrust::Trusted => (output.text().to_owned(), OutputTrust::Trusted, false),
        OutputTrust::Untrusted if script.is_some() && binding.describes_tools() => {
            (output.text().to_owned(), OutputTrust::Untrusted, true)
        }
        // `OutputTrust` is `#[non_exhaustive]` in the contract crate: an
        // unknown future variant takes the safe path and is nonce-wrapped
        // as untrusted.
        _ => (nonce.wrap(output.text()), OutputTrust::Untrusted, false),
    };
    if let Some(report) = script {
        emitter.tool_result(
            section,
            report.turn,
            "",
            binding.alias(),
            Some(binding.id()),
            vendor,
            &content,
            trust,
        );
    }
    Ok(ToolDispatch {
        content,
        trust,
        listing,
    })
}

/// Applies the model-issued dispatch rules to one bound tool call's answer:
/// the dispatch rules of [`prepare_dispatch`], without its script report,
/// under the model-loop failure rule, then the `ToolResult` event report
/// under the model's call id.
///
/// A model-issued call always resumes with content: the tool's own failure
/// ([`Error::Tool`]) becomes the call's result - the error message
/// nonce-wrapped as untrusted - so the model reads the failure and the
/// round continues; the shared rules have already fired the failed
/// observation. Every other dispatch failure still propagates.
///
/// # Errors
/// Propagates any error of [`prepare_dispatch`] other than the tool's own
/// failure ([`Error::Tool`]), which becomes the call's result.
pub fn prepare_model_dispatch(
    binding: &ToolBinding,
    vendor: Option<&str>,
    call_result: std::result::Result<ToolOutput, ToolError>,
    nonce: &GuardNonce,
    emitter: &Emitter,
    section: &str,
    report: &ModelReport,
) -> Result<ToolDispatch> {
    let outcome = match dispatch_body(binding, vendor, call_result, nonce, emitter, section, None) {
        Ok(outcome) => outcome,
        Err(Error::Tool { message, .. }) => ToolDispatch {
            content: nonce.wrap(&message),
            trust: OutputTrust::Untrusted,
            listing: false,
        },
        Err(error) => return Err(error),
    };
    emitter.tool_result(
        section,
        report.turn,
        &report.call_id,
        binding.alias(),
        Some(binding.id()),
        vendor,
        &outcome.content,
        outcome.trust,
    );
    Ok(outcome)
}

#[cfg(test)]
#[path = "dispatch-tests.rs"]
mod tests;
