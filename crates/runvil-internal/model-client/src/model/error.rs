//! The model failure vocabulary: the fixed phrase each model code's text
//! starts with, and the constructors of a model round's failure.
//!
//! A failure's message and prose start as the code's fixed phrase. A failure
//! built from an HTTP status appends ` (status N)`, and a 401 or 403 is
//! `Unavailable` with `the model backend did not accept the credentials`.
//! Only `MalformedResponse`, `EmptyReply`, `Unavailable`, and `InvalidParts`
//! extend it with `: ` and text this repository's own code wrote. Provider
//! text never enters the prose; a broker appends it to the message only.

use runvil_types::error::{Error, ErrorCode, ErrorFact};
use serde_json::Value;

/// The specific an empty reply adds to its phrase when the reader dropped a
/// reasoning side channel that it could not use as answer text.
const REASONING_IGNORED: &str = "reasoning content was present but ignored";

/// Returns the fixed phrase of a model code: one lowercase phrase written
/// for a model to read. Any other code returns its default prose.
#[must_use]
pub fn phrase(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::ContextOverflow => "the request is larger than the model's context window",
        ErrorCode::RateLimited => "the model backend is limiting the request rate",
        ErrorCode::QuotaExhausted => "the model backend says the usage quota is spent",
        ErrorCode::Overloaded => "the model backend is overloaded",
        ErrorCode::Refused => "the model backend refused the request on content policy grounds",
        ErrorCode::Timeout => "the model backend did not answer in time",
        ErrorCode::Transport => "the connection to the model backend failed",
        ErrorCode::ServerError => "the model backend reported a fault of its own",
        ErrorCode::Rejected => "the model backend rejected the request",
        ErrorCode::MalformedResponse => {
            "the model backend sent a reply that could not be understood"
        }
        ErrorCode::EmptyReply => "the model replied with no text and no tool calls",
        ErrorCode::Unavailable => "model access is turned off or not configured",
        ErrorCode::InvalidParts => "the completion was built from invalid parts",
        other => other.prose(),
    }
}

/// Builds the failure of a model round or a catalog fetch, with `message` as
/// both its message and its prose. `code` is one of the model codes, and
/// `message` is its phrase, extended as the module page says.
#[must_use]
pub fn completion_error(code: ErrorCode, message: impl Into<String>) -> Error {
    let message = message.into();
    Error::new(code, message.clone(), message)
}

/// Builds an `EmptyReply` failure for a turn that carried no text and no tool
/// calls. Its text is the phrase, extended with `: reasoning content was
/// present but ignored` when `reasoning_present` says the turn's reasoning
/// side channel was dropped rather than used as answer text. A caller that
/// knows the backend's finish reason adds it as the `FinishReason` fact.
#[must_use]
pub fn empty_reply(reasoning_present: bool) -> Error {
    if reasoning_present {
        specific(ErrorCode::EmptyReply, REASONING_IGNORED)
    } else {
        phrased(ErrorCode::EmptyReply)
    }
}

/// Builds a `ContextOverflow` failure with the token counts the provider
/// stated: the prompt tokens as the `Count` fact and the window as the
/// `Limit` fact. A count the provider omitted is absent.
#[must_use]
pub fn context_overflow(
    prompt_tokens: Option<u32>,
    window: Option<u32>,
    message: impl Into<String>,
) -> Error {
    let mut error = completion_error(ErrorCode::ContextOverflow, message);
    if let Some(tokens) = prompt_tokens {
        error.facts.insert(ErrorFact::Count, Value::from(tokens));
    }
    if let Some(window) = window {
        error.facts.insert(ErrorFact::Limit, Value::from(window));
    }
    error
}

/// Builds a failure whose text is the code's fixed phrase.
pub(crate) fn phrased(code: ErrorCode) -> Error {
    completion_error(code, phrase(code))
}

/// Builds a failure whose text is the code's fixed phrase extended with `: `
/// and a specific this crate wrote.
fn specific(code: ErrorCode, specific: impl std::fmt::Display) -> Error {
    completion_error(code, format!("{}: {specific}", phrase(code)))
}

/// Builds an `InvalidParts` failure naming which part was refused.
pub(crate) fn invalid_parts(part: impl std::fmt::Display) -> Error {
    specific(ErrorCode::InvalidParts, part)
}

#[cfg(test)]
#[path = "error-tests.rs"]
mod tests;
