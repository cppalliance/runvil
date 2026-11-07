//! The nested-inference round a section's `models.infer` yields resolve to.
//!
//! One `infer` shape only: a single direct, tool-free model round on a
//! fresh conversation. `models.infer(model, prompt)` runs it with the
//! handle's frozen binding; `models.infer(prompt)` resolves the section's
//! current model and runs the same path. Neither form sends tools, reads or
//! changes the section's `messages`, sets `reply`, or touches `sys`, and
//! both work inside a loop's tool handlers and compactors, where a loop
//! raises. A Lua block that needs tools uses `call` on a section. The
//! scheduler's leaf dispatch issues the round as a `Chat` effect over one
//! user message and no tools; when the answer arrives, [`accept_infer`]
//! reports the round and renders its text, and the yielding chain resumes
//! with the outcome.

use runvil_types::error::ErrorCode;
use runvil_types::event::ReplyOrigin;
use runvil_types::event_lifecycle;
use runvil_types::ids::RoundId;

use crate::Error;
use crate::model::{Completion, CompletionResult, completion_error, phrase};

use super::context::ReportingHandles;
use super::turn;

/// Reports one completed infer round through [`turn::report_model_turn`],
/// whose sequence ends in the `assistant_reply` tagged `origin = infer`,
/// and renders its text. The no-tools-advertised violation check is what
/// keeps the round tool-free.
fn accept_infer_completion(
    completion: Completion,
    round: RoundId,
    reporting: &ReportingHandles,
    section: &str,
) -> Result<String, Error> {
    let turn = turn::advance_turn(&reporting.turns);
    let (outcome, _) = turn::report_model_turn(
        &reporting.emitter,
        section,
        turn,
        round,
        completion,
        ReplyOrigin::Infer,
    );
    match outcome {
        CompletionResult::Text(text) => Ok(text),
        // No tools were advertised, so a tool-call turn is a backend
        // protocol violation rather than something to dispatch.
        // `CompletionResult` is `#[non_exhaustive]` across the crate boundary:
        // an unrecognized future outcome is the same violation.
        CompletionResult::ToolCalls(_) => Err(malformed_reply(
            "the round sent tool calls, but no tools were advertised",
        )),
        _ => Err(malformed_reply(
            "the round sent an unrecognized outcome, and no tools were advertised",
        )),
    }
}

/// The malformed-reply failure for an infer round whose outcome breaks the
/// round's protocol, with `specific` naming how.
fn malformed_reply(specific: &str) -> Error {
    let code = ErrorCode::MalformedResponse;
    completion_error(code, format!("{}: {specific}", phrase(code)))
}

/// Applies one infer round's answer, the completion the caller obtained
/// or its failure: it reports the round exactly like a prose round,
/// through `turn::report_model_turn` on the chain's reporting handles
/// under `section`, and renders its text.
///
/// A failed completion is a failed turn and the call's error. A round the
/// caller drops never reaches here: the drop resumes the chain with a
/// cancellation, so no `MODEL_TURN_FAILED` fires for it.
///
/// # Errors
/// Returns the completion's failure, or a model failure with code
/// `MalformedResponse` when the round produced tool calls (none were
/// advertised) or an unrecognized outcome.
pub(super) fn accept_infer(
    result: std::result::Result<Box<Completion>, Error>,
    round: RoundId,
    reporting: &ReportingHandles,
    section: &str,
) -> Result<String, Error> {
    let completion = match result {
        Ok(completion) => completion,
        Err(error) => {
            reporting
                .emitter
                .report(section, event_lifecycle::MODEL_TURN_FAILED);
            return Err(error);
        }
    };
    accept_infer_completion(*completion, round, reporting, section)
}

#[cfg(test)]
mod tests {
    //! The nested-inference answer: a round that comes back with tool calls,
    //! though none were advertised, fails as a malformed reply.

    use std::sync::Arc;
    use std::sync::atomic::AtomicU32;

    use runvil_types::ids::RoundId;
    use serde_json::json;

    use super::*;
    use crate::model::ToolCall;
    use crate::test_support::recording::null_emitter;

    #[test]
    fn an_infer_round_answered_with_tool_calls_fails_as_a_retryable_malformed_reply() {
        let calls = CompletionResult::ToolCalls(vec![
            ToolCall::from_parts("call-1", "grab", json!({})).expect("a whole call"),
        ]);
        let completion = Completion::from_result(calls, "test-model").expect("a whole result");
        let error = accept_infer(
            Ok(Box::new(completion)),
            RoundId::new(0),
            &ReportingHandles {
                emitter: Arc::new(null_emitter()),
                turns: Arc::new(AtomicU32::new(0)),
            },
            "Infer",
        )
        .expect_err("no tools were advertised, so tool calls are refused");
        assert_eq!(
            error.code,
            ErrorCode::MalformedResponse,
            "a reply that broke protocol is malformed, so it may come back well formed"
        );
    }
}
