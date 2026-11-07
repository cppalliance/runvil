//! Tests for the HTTP failure classifier: one per rule, per overflow
//! phrase, per token-count form, the in-stream envelope, and a body that
//! matches nothing.

use runvil::model::phrase;
use runvil::{Error, ErrorCode, ErrorFact};

use super::{classify_http_failure, classify_stream_error};

fn code_of(status: u16, body: &str) -> ErrorCode {
    classify_http_failure(status, body).code
}

/// The `(prompt_tokens, window)` counts an overflow names as its `Count`
/// and `Limit` facts.
fn overflow(error: &Error) -> (Option<u32>, Option<u32>) {
    let count = |key: ErrorFact| {
        error
            .facts
            .get(&key)
            .and_then(serde_json::Value::as_u64)
            .and_then(|count| u32::try_from(count).ok())
    };
    (count(ErrorFact::Count), count(ErrorFact::Limit))
}

#[test]
fn every_overflow_phrase_classifies_a_400_as_context_overflow() {
    let phrases = [
        "context length",
        "context window",
        "context size",
        "context_length_exceeded",
        "maximum context length",
        "prompt is too long",
        "too many tokens",
        "exceeds the available context size",
        "exceed_context_size",
        "input is too long",
        "exceeds the maximum number of tokens",
        "too large for model",
    ];
    assert_eq!(phrases.len(), 12, "the plan lists twelve phrases");
    for phrase in phrases {
        let body = format!("{{\"error\":\"the request hit a limit: {phrase} reached\"}}");
        assert_eq!(
            code_of(400, &body),
            ErrorCode::ContextOverflow,
            "400 with {phrase:?}"
        );
        assert_eq!(
            code_of(413, &body),
            ErrorCode::ContextOverflow,
            "413 with {phrase:?}"
        );
    }
}

#[test]
fn overflow_phrases_match_case_insensitively() {
    assert_eq!(
        code_of(400, "CONTEXT WINDOW exceeded"),
        ErrorCode::ContextOverflow
    );
    assert_eq!(
        code_of(413, "Prompt Is Too Long"),
        ErrorCode::ContextOverflow
    );
}

#[test]
fn provider_overflow_detection_matches_known_signatures() {
    let cases: &[(u16, &str, ErrorCode)] = &[
        (
            400,
            "This model's maximum context length is 4096 tokens.",
            ErrorCode::ContextOverflow,
        ),
        (400, "context_length_exceeded", ErrorCode::ContextOverflow),
        (
            400,
            "the request exceeds the available context size",
            ErrorCode::ContextOverflow,
        ),
        (413, "prompt is too long", ErrorCode::ContextOverflow),
        (400, "CONTEXT WINDOW exceeded", ErrorCode::ContextOverflow),
        (400, "too many tokens in prompt", ErrorCode::ContextOverflow),
        // A server fault never classifies as an overflow, even with
        // overflow wording.
        (
            500,
            "maximum context length is 4096 tokens",
            ErrorCode::ServerError,
        ),
        // A client rejection without overflow wording stays a plain
        // rejection.
        (
            400,
            "invalid request: unknown field `stream`",
            ErrorCode::Rejected,
        ),
        // Credentials win over overflow wording outside 400 and 413.
        (401, "context length", ErrorCode::Unavailable),
    ];
    for (status, body, expected) in cases {
        assert_eq!(
            code_of(*status, body),
            *expected,
            "status {status} with body {body:?}"
        );
    }
}

#[test]
fn an_overflow_phrase_on_another_status_is_not_an_overflow() {
    assert_eq!(code_of(422, "context length exceeded"), ErrorCode::Rejected);
    assert_eq!(code_of(429, "context length"), ErrorCode::RateLimited);
}

#[test]
fn the_openai_token_counts_are_read_from_the_message() {
    let error = classify_http_failure(
        400,
        "This model's maximum context length is 4096 tokens. However, your messages \
         resulted in 5120 tokens. Please reduce the length of the messages.",
    );
    assert_eq!(error.code, ErrorCode::ContextOverflow);
    assert_eq!(overflow(&error), (Some(5120), Some(4096)));
}

#[test]
fn the_greater_than_maximum_token_counts_are_read_from_the_message() {
    let error = classify_http_failure(400, "prompt is too long: 205000 tokens > 200000 maximum");
    assert_eq!(error.code, ErrorCode::ContextOverflow);
    assert_eq!(overflow(&error), (Some(205_000), Some(200_000)));
}

#[test]
fn an_overflow_that_states_no_counts_leaves_them_unset() {
    let error = classify_http_failure(400, "context_length_exceeded");
    assert_eq!(error.code, ErrorCode::ContextOverflow);
    assert_eq!(overflow(&error), (None, None));
}

#[test]
fn a_count_too_large_for_u32_is_left_unset() {
    let error = classify_http_failure(
        400,
        "maximum context length is 99999999999 tokens, however you sent 5 tokens",
    );
    assert_eq!(error.code, ErrorCode::ContextOverflow);
    assert_eq!(overflow(&error), (Some(5), None));
}

#[test]
fn a_429_naming_quota_or_billing_is_quota_exhausted() {
    for word in ["quota", "billing", "insufficient_quota", "credit", "QUOTA"] {
        let body = format!("{{\"error\":\"your {word} has run out\"}}");
        assert_eq!(
            code_of(429, &body),
            ErrorCode::QuotaExhausted,
            "429 with {word:?}"
        );
    }
}

#[test]
fn any_other_429_is_rate_limited() {
    assert_eq!(
        code_of(429, "slow down: too many requests"),
        ErrorCode::RateLimited
    );
    assert_eq!(code_of(429, "(empty body)"), ErrorCode::RateLimited);
}

#[test]
fn a_503_or_529_is_overloaded() {
    assert_eq!(code_of(503, "service unavailable"), ErrorCode::Overloaded);
    assert_eq!(code_of(529, "busy"), ErrorCode::Overloaded);
}

#[test]
fn a_5xx_naming_overloaded_is_overloaded() {
    assert_eq!(
        code_of(500, "{\"type\":\"overloaded_error\"}"),
        ErrorCode::Overloaded
    );
    assert_eq!(
        code_of(502, "The server is OVERLOADED"),
        ErrorCode::Overloaded
    );
}

#[test]
fn every_other_5xx_is_a_server_error() {
    for status in [500, 501, 502, 504, 599] {
        assert_eq!(
            code_of(status, "internal failure"),
            ErrorCode::ServerError,
            "status {status}"
        );
    }
}

#[test]
fn a_401_or_403_is_unavailable_with_the_credentials_phrase() {
    for status in [401, 403] {
        let error = classify_http_failure(status, "bad key");
        assert_eq!(error.code, ErrorCode::Unavailable);
        assert_eq!(
            error.prose,
            format!("the model backend did not accept the credentials (status {status})")
        );
    }
}

#[test]
fn a_400_naming_a_content_policy_term_is_refused() {
    for word in ["content_filter", "content policy", "safety", "refus"] {
        let body = format!("{{\"error\":\"blocked by {word} rules\"}}");
        assert_eq!(code_of(400, &body), ErrorCode::Refused, "400 with {word:?}");
    }
}

#[test]
fn a_400_naming_both_a_context_limit_and_a_safety_term_is_an_overflow() {
    assert_eq!(
        code_of(400, "context length exceeded; safety settings unchanged"),
        ErrorCode::ContextOverflow
    );
}

#[test]
fn a_content_policy_term_on_another_status_is_rejected() {
    assert_eq!(code_of(422, "content_filter"), ErrorCode::Rejected);
}

#[test]
fn a_body_that_matches_no_rule_is_rejected_never_a_success() {
    for (status, body) in [
        (400, "invalid request: unknown field `stream`"),
        (404, "no such model"),
        (422, "unprocessable"),
        (418, "(empty body)"),
        (302, "moved"),
        (200, "ok"),
    ] {
        assert_eq!(
            code_of(status, body),
            ErrorCode::Rejected,
            "status {status} with body {body:?}"
        );
    }
}

#[test]
fn the_prose_names_the_code_and_the_status_and_the_message_adds_the_body() {
    let error = classify_http_failure(503, "upstream <busy>");
    assert_eq!(error.prose, "the model backend is overloaded (status 503)");
    assert_eq!(
        error.message,
        "the model backend is overloaded (status 503): upstream <busy>"
    );
    assert!(
        !error.prose.contains("upstream"),
        "the prose never carries the body"
    );
}

#[test]
fn each_http_code_carries_its_fixed_phrase_and_the_status_suffix() {
    let cases: &[(u16, &str, &str)] = &[
        (
            400,
            "context length",
            "the request is larger than the model's context window (status 400)",
        ),
        (
            429,
            "slow down",
            "the model backend is limiting the request rate (status 429)",
        ),
        (
            429,
            "quota",
            "the model backend says the usage quota is spent (status 429)",
        ),
        (503, "busy", "the model backend is overloaded (status 503)"),
        (
            400,
            "content_filter",
            "the model backend refused the request on content policy grounds (status 400)",
        ),
        (
            500,
            "boom",
            "the model backend reported a fault of its own (status 500)",
        ),
        (
            404,
            "no model",
            "the model backend rejected the request (status 404)",
        ),
        (
            401,
            "no key",
            "the model backend did not accept the credentials (status 401)",
        ),
    ];
    for (status, body, prose) in cases {
        let error = classify_http_failure(*status, body);
        assert_eq!(error.prose, *prose, "status {status}, {body:?}");
        assert_eq!(error.message, format!("{prose}: {body}"));
    }
}

#[test]
fn a_429_is_rate_limited_unless_its_body_names_a_spent_quota() {
    assert_eq!(
        classify_http_failure(429, "slow down").code,
        ErrorCode::RateLimited
    );
    assert_eq!(
        classify_http_failure(429, "insufficient_quota").code,
        ErrorCode::QuotaExhausted
    );
}

#[test]
fn provider_text_stays_in_the_message_and_out_of_the_prose() {
    let error = classify_http_failure(503, "upstream <busy>");
    assert!(error.prose.starts_with(phrase(ErrorCode::Overloaded)));
    assert!(!error.prose.contains("upstream"));
    assert!(error.message.ends_with(": upstream <busy>"));
}

#[test]
fn the_classifier_keeps_only_the_body_it_was_given() {
    let body = "x".repeat(2000);
    let error = classify_http_failure(400, &body);
    assert_eq!(error.message, format!("{}: {body}", error.prose));
}

#[test]
fn an_in_stream_envelope_naming_a_context_limit_is_an_overflow_without_a_status() {
    let error = classify_stream_error(
        "This model's maximum context length is 4096 tokens, however you sent 5000 tokens",
    );
    assert_eq!(error.code, ErrorCode::ContextOverflow);
    assert_eq!(overflow(&error), (Some(5000), Some(4096)));
    assert_eq!(
        error.prose,
        "the request is larger than the model's context window"
    );
    assert!(
        !error.prose.contains("status"),
        "an in-stream envelope carries no status"
    );
}

#[test]
fn an_in_stream_envelope_applies_the_other_body_text_rules() {
    assert_eq!(
        classify_stream_error("overloaded_error: try again").code,
        ErrorCode::Overloaded
    );
    assert_eq!(
        classify_stream_error("insufficient_quota").code,
        ErrorCode::QuotaExhausted
    );
    assert_eq!(
        classify_stream_error("blocked by content_filter").code,
        ErrorCode::Refused
    );
}

#[test]
fn an_in_stream_envelope_that_matches_no_rule_stays_a_transport_failure() {
    let error = classify_stream_error("upstream\\ndied");
    assert_eq!(error.code, ErrorCode::Transport);
    assert_eq!(error.prose, "the connection to the model backend failed");
    assert_eq!(
        error.message,
        "the connection to the model backend failed: upstream\\ndied"
    );
}
