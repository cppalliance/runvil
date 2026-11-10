//! Tests for model resolution: nothing to resolve, a model list the broker
//! cannot fetch, and a selection the list lacks.

use runvil_engine::effect::Round;
use runvil_engine::model::{
    Completion, CompletionOptions, Message, ModelBinding, ModelCatalog, ToolSchema,
    make_completion_error, phrase,
};
use serde_json::json;

use super::*;
use crate::performers::BoxFuture;

/// Lists no model; a round is never made.
struct EmptyBroker;

impl InferenceBroker for EmptyBroker {
    fn models(&self) -> BoxFuture<Result<ModelCatalog, Error>> {
        Box::pin(async { Ok(ModelCatalog::empty()) })
    }

    fn chat(
        &self,
        _binding: ModelBinding,
        _messages: Vec<Message>,
        _tools: Vec<ToolSchema>,
        _options: CompletionOptions,
        _round: Round,
    ) -> BoxFuture<Result<Box<Completion>, Error>> {
        let code = ErrorCode::Unavailable;
        Box::pin(async move { Err(make_completion_error(code, phrase(code))) })
    }
}

#[tokio::test]
async fn no_selection_and_an_empty_broker_catalog_bind_no_model() {
    // No selection and a broker that lists nothing: nothing to resolve,
    // so the roles stay unbound rather than the run failing.
    let model = current_model(None, &EmptyBroker)
        .await
        .expect("an empty list is not a failure");
    assert!(model.is_none());
}

/// Fails its model list as `Transport`; a round is never made.
struct UnlistedBroker;

impl InferenceBroker for UnlistedBroker {
    fn models(&self) -> BoxFuture<Result<ModelCatalog, Error>> {
        let code = ErrorCode::Transport;
        Box::pin(async move { Err(make_completion_error(code, phrase(code))) })
    }

    fn chat(
        &self,
        binding: ModelBinding,
        messages: Vec<Message>,
        tools: Vec<ToolSchema>,
        options: CompletionOptions,
        round: Round,
    ) -> BoxFuture<Result<Box<Completion>, Error>> {
        EmptyBroker.chat(binding, messages, tools, options, round)
    }
}

#[tokio::test]
async fn a_model_list_the_broker_cannot_fetch_keeps_the_brokers_failure() {
    let error = current_model(None, &UnlistedBroker)
        .await
        .expect_err("a failed listing fails the lookup");
    let phrase = phrase(ErrorCode::Transport);
    assert_eq!(error.code, ErrorCode::Transport);
    assert_eq!(
        error.message,
        format!("the model catalog could not be fetched: {phrase}")
    );
    assert_eq!(error.prose, phrase, "the broker's prose stays");
}

#[tokio::test]
async fn a_selection_the_list_lacks_is_an_unmet_requirement_naming_the_selection() {
    let error = current_model(Some("absent"), &EmptyBroker)
        .await
        .expect_err("an absent selection fails the lookup");
    assert_eq!(error.code, ErrorCode::RequirementsUnmet);
    assert_eq!(
        error.message,
        "the selected model `absent` is absent from the fetched catalog"
    );
    assert_eq!(error.prose, ErrorCode::RequirementsUnmet.prose());
    assert_eq!(error.facts.get(&ErrorFact::Name), Some(&json!("absent")));
    assert_eq!(
        error.facts.get(&ErrorFact::Reason),
        Some(&json!("model_absent"))
    );
}
