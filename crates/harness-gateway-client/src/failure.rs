//! The failures this crate builds itself: each is an [`Error`] made from a
//! model code and that code's fixed phrase.
//!
//! A non-success HTTP status is not built here; `make_classify_http_failure_error`
//! reads its status and bounded body. These helpers cover the rest: a
//! send or read that went wrong, a disabled client, and a reply this
//! crate cannot read.

use std::fmt::Display;

use runvil::model::{make_completion_error, phrase};
use runvil::{Error, ErrorCode};

/// A send or read that failed: `Timeout` when `error` was a timeout and
/// `Transport` otherwise, with `error` appended to the message as its
/// cause.
#[expect(
    clippy::needless_pass_by_value,
    reason = "a `map_err` adapter takes the error by value"
)]
pub(crate) fn make_transport_failure_error(error: reqwest::Error) -> Error {
    if error.is_timeout() {
        return make_timeout_error(&error);
    }
    let code = ErrorCode::Transport;
    make_completion_error(code, phrase(code)).with_cause(&error)
}

/// A receive deadline that elapsed: a `Timeout` with the elapsed error
/// appended to the message as its cause.
#[expect(
    clippy::needless_pass_by_value,
    reason = "a `map_err` adapter takes the error by value"
)]
pub(crate) fn make_elapsed_error(error: tokio::time::error::Elapsed) -> Error {
    make_timeout_error(&error)
}

fn make_timeout_error(cause: &(dyn std::error::Error + 'static)) -> Error {
    let code = ErrorCode::Timeout;
    make_completion_error(code, phrase(code)).with_cause(cause)
}

/// The failure of a client built with `GatewayChat::disabled`: model
/// access is off, so no round is sent.
pub(crate) fn make_unavailable_error() -> Error {
    let code = ErrorCode::Unavailable;
    make_completion_error(code, phrase(code))
}

/// A `MalformedResponse` failure: the code's fixed phrase extended with
/// `: ` and `specific`, which this crate's own code wrote. Provider text
/// never goes in `specific`.
pub(crate) fn make_malformed_error(specific: impl Display) -> Error {
    let code = ErrorCode::MalformedResponse;
    make_completion_error(code, format!("{}: {specific}", phrase(code)))
}

/// Appends provider `text`, already bounded and control-escaped, to
/// `error`'s message after `: `. The prose stays as it was, so provider
/// text never reaches the model.
pub(crate) fn with_detail(mut error: Error, text: &str) -> Error {
    error.message.push_str(": ");
    error.message.push_str(text);
    error
}

/// A validating constructor's refusal of parts this crate decoded from a
/// reply: an `InvalidParts` refusal becomes the `MalformedResponse` the
/// backend's reply earned, keeping its specific, since another reply may
/// hold whole parts. Any other failure passes through.
pub(crate) fn decoded(error: Error) -> Error {
    let code = ErrorCode::InvalidParts;
    if error.code != code {
        return error;
    }
    let prose = error.prose.as_str();
    let specific = prose
        .strip_prefix(phrase(code))
        .and_then(|rest| rest.strip_prefix(": "))
        .unwrap_or(prose);
    make_malformed_error(specific)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn each_helper_uses_the_fixed_phrase_for_its_code() {
        let elapsed_error = tokio::time::timeout(
            std::time::Duration::from_millis(1),
            std::future::pending::<()>(),
        )
        .await
        .expect_err("a pending future elapses");
        let cause = elapsed_error.to_string();
        let timed_out = make_elapsed_error(elapsed_error);
        assert_eq!(timed_out.code, ErrorCode::Timeout);
        assert_eq!(timed_out.prose, "the model backend did not answer in time");
        assert!(
            timed_out.message.contains(&cause) && !timed_out.prose.contains(&cause),
            "the elapsed error is in the message only: {timed_out:?}"
        );

        let disabled = make_unavailable_error();
        assert_eq!(disabled.code, ErrorCode::Unavailable);
        assert_eq!(
            disabled.to_string(),
            "model access is turned off or not configured"
        );

        let unreadable = make_malformed_error("model list body exceeds the 8-byte limit");
        assert_eq!(unreadable.code, ErrorCode::MalformedResponse);
        assert_eq!(
            unreadable.to_string(),
            "the model backend sent a reply that could not be understood: \
             model list body exceeds the 8-byte limit"
        );
        assert_eq!(unreadable.message, unreadable.prose);
    }

    #[test]
    fn a_decoded_parts_refusal_is_a_malformed_reply_with_its_specific() {
        use runvil::model::ToolCall;
        let refusal = ToolCall::from_parts(" ", "fetch", serde_json::json!({}))
            .expect_err("a blank id is refused");
        assert_eq!(refusal.code, ErrorCode::InvalidParts);
        let reply = decoded(refusal);
        assert_eq!(reply.code, ErrorCode::MalformedResponse);
        assert_eq!(
            reply.to_string(),
            "the model backend sent a reply that could not be understood: tool call id was blank"
        );

        let passed = decoded(make_unavailable_error());
        assert_eq!(passed.code, ErrorCode::Unavailable);
    }
}
