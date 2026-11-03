//! Tests for the failure vocabulary: the tags, the fixed phrases, the
//! specifics that extend them, and the constructors' facts.

use super::*;

const ALL_CODES: [ErrorCode; 13] = [
    ErrorCode::ContextOverflow,
    ErrorCode::RateLimited,
    ErrorCode::QuotaExhausted,
    ErrorCode::Overloaded,
    ErrorCode::Refused,
    ErrorCode::Timeout,
    ErrorCode::Transport,
    ErrorCode::ServerError,
    ErrorCode::Rejected,
    ErrorCode::MalformedResponse,
    ErrorCode::EmptyReply,
    ErrorCode::Unavailable,
    ErrorCode::InvalidParts,
];

#[test]
fn each_code_tags_as_its_snake_case_name() {
    let tags: Vec<&str> = ALL_CODES.iter().map(|code| code.tag()).collect();
    assert_eq!(
        tags,
        [
            "context_overflow",
            "rate_limited",
            "quota_exhausted",
            "overloaded",
            "refused",
            "timeout",
            "transport",
            "server_error",
            "rejected",
            "malformed_response",
            "empty_reply",
            "unavailable",
            "invalid_parts",
        ]
    );
}

#[test]
fn each_code_has_its_fixed_phrase() {
    let phrases = [
        (
            ErrorCode::ContextOverflow,
            "the request is larger than the model's context window",
        ),
        (
            ErrorCode::RateLimited,
            "the model backend is limiting the request rate",
        ),
        (
            ErrorCode::QuotaExhausted,
            "the model backend says the usage quota is spent",
        ),
        (ErrorCode::Overloaded, "the model backend is overloaded"),
        (
            ErrorCode::Refused,
            "the model backend refused the request on content policy grounds",
        ),
        (
            ErrorCode::Timeout,
            "the model backend did not answer in time",
        ),
        (
            ErrorCode::Transport,
            "the connection to the model backend failed",
        ),
        (
            ErrorCode::ServerError,
            "the model backend reported a fault of its own",
        ),
        (
            ErrorCode::Rejected,
            "the model backend rejected the request",
        ),
        (
            ErrorCode::MalformedResponse,
            "the model backend sent a reply that could not be understood",
        ),
        (
            ErrorCode::EmptyReply,
            "the model replied with no text and no tool calls",
        ),
        (
            ErrorCode::Unavailable,
            "model access is turned off or not configured",
        ),
        (
            ErrorCode::InvalidParts,
            "the completion was built from invalid parts",
        ),
    ];
    assert_eq!(phrases.len(), ALL_CODES.len());
    for (code, text) in phrases {
        assert_eq!(phrase(code), text, "phrase of {code:?}");
    }
}

#[test]
fn a_code_outside_the_model_family_phrases_as_its_default_prose() {
    for code in [
        ErrorCode::NotFound,
        ErrorCode::ContextExhausted,
        ErrorCode::LuaRuntime,
    ] {
        assert_eq!(phrase(code), code.prose(), "phrase of {code:?}");
    }
}

#[test]
fn completion_error_uses_the_message_as_both_texts_and_adds_nothing_else() {
    let error = completion_error(ErrorCode::Rejected, "the message");
    assert_eq!(error.code, ErrorCode::Rejected);
    assert_eq!(error.message, "the message");
    assert_eq!(error.prose, "the message");
    assert_eq!(error.to_string(), "the message");
    assert_eq!(error.at, None);
    assert!(error.facts.is_empty());
}

#[test]
fn context_overflow_holds_the_counts_it_was_given_as_facts() {
    let both = context_overflow(Some(10), Some(8), "too big");
    assert_eq!(both.code, ErrorCode::ContextOverflow);
    assert_eq!(
        (both.message.as_str(), both.prose.as_str()),
        ("too big", "too big")
    );
    assert_eq!(both.facts.get(&FactKey::Count), Some(&Value::from(10)));
    assert_eq!(both.facts.get(&FactKey::Limit), Some(&Value::from(8)));

    let prompt_only = context_overflow(Some(10), None, "too big");
    assert_eq!(
        prompt_only.facts.get(&FactKey::Count),
        Some(&Value::from(10))
    );
    assert_eq!(prompt_only.facts.get(&FactKey::Limit), None);

    let neither = context_overflow(None, None, "too big");
    assert_eq!(neither.code, ErrorCode::ContextOverflow);
    assert!(neither.facts.is_empty());
}

#[test]
fn a_timeout_and_a_transport_failure_keep_their_cause_out_of_the_prose() {
    let timed_out = completion_error(
        ErrorCode::Timeout,
        "the model backend did not answer in time",
    )
    .with_cause(&std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "deadline",
    ));
    assert_eq!(timed_out.code, ErrorCode::Timeout);
    assert_eq!(
        timed_out.message,
        "the model backend did not answer in time: deadline"
    );
    assert_eq!(timed_out.prose, "the model backend did not answer in time");

    let refused = completion_error(
        ErrorCode::Transport,
        "the connection to the model backend failed",
    )
    .with_cause(&std::io::Error::other("connection refused"));
    assert_eq!(refused.code, ErrorCode::Transport);
    assert!(refused.message.contains("connection refused"));
    assert!(!refused.prose.contains("connection refused"));
}

#[test]
fn a_phrased_failure_shows_only_the_fixed_phrase() {
    for code in ALL_CODES {
        let error = phrased(code);
        assert_eq!(error.code, code);
        assert_eq!(error.to_string(), phrase(code));
        assert_eq!(error.prose, phrase(code));
        assert!(error.facts.is_empty());
    }
}

#[test]
fn a_specific_extends_the_phrase_after_a_colon_in_both_texts() {
    let malformed = specific(ErrorCode::MalformedResponse, "no choices in response");
    assert_eq!(malformed.code, ErrorCode::MalformedResponse);
    assert_eq!(
        malformed.to_string(),
        "the model backend sent a reply that could not be understood: no choices in response"
    );
    assert_eq!(malformed.prose, malformed.message);

    let refused = invalid_parts("tool call id was blank");
    assert_eq!(refused.code, ErrorCode::InvalidParts);
    assert_eq!(
        refused.prose,
        "the completion was built from invalid parts: tool call id was blank"
    );
}

#[test]
fn the_shared_empty_reply_owner_builds_the_phrase_and_the_reasoning_specific() {
    // One owner for the empty-reply text: the bare phrase when the turn
    // carried no reasoning, and the phrase plus the reasoning-ignored
    // specific when one was dropped. The finish reason is the caller's fact.
    let plain = empty_reply(false);
    assert_eq!(plain.code, ErrorCode::EmptyReply);
    assert_eq!(plain.to_string(), phrase(ErrorCode::EmptyReply));
    assert_eq!(plain.prose, phrase(ErrorCode::EmptyReply));
    assert_eq!(plain.facts.get(&FactKey::FinishReason), None);

    let reasoned = empty_reply(true);
    let expected = format!(
        "{}: reasoning content was present but ignored",
        phrase(ErrorCode::EmptyReply)
    );
    assert_eq!(reasoned.code, ErrorCode::EmptyReply);
    assert_eq!(reasoned.message, expected);
    assert_eq!(reasoned.prose, expected);
    assert!(reasoned.facts.is_empty());
}
