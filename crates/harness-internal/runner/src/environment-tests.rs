//! Tests for the Host snapshot and for model resolution with nothing to
//! resolve.

use runvil::effect::Round;
use runvil::error::ErrorCode;
use runvil::model::{
    Completion, CompletionOptions, Message, ModelBinding, ModelCatalog, ToolSchema,
    completion_error, phrase,
};

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
        Box::pin(async move { Err(completion_error(code, phrase(code))) })
    }
}

#[test]
fn the_host_snapshot_serves_the_first_root_and_the_selection() {
    let host = HostSnapshot {
        selected_model: Some("gpt".to_owned()),
        workspace_roots: vec![PathBuf::from("/w/one"), PathBuf::from("/w/two")],
    };
    let ui = host.ui();
    assert_eq!(ui["selected_model"], "gpt");
    assert_eq!(
        ui["workspace_root"],
        PathBuf::from("/w/one").display().to_string()
    );
    let empty = HostSnapshot::default().ui();
    assert!(empty["selected_model"].is_null());
    assert!(empty["workspace_root"].is_null());
}

#[tokio::test]
async fn no_selection_and_an_empty_broker_catalog_bind_no_model() {
    // No selection and a broker that lists nothing: nothing to resolve,
    // so the roles stay unbound rather than the run failing.
    let model = current_model(&HostSnapshot::default(), &EmptyBroker)
        .await
        .expect("an empty list is not a failure");
    assert!(model.is_none());
}
