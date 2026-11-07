//! What every suite Host hands a Harness beside its recorder: an offline
//! broker and a tokio-backed timer.

use std::time::Duration;

use harness::{BoxFuture, InferenceBroker, Timer};
use runvil::effect::Round;
use runvil::model::{
    Completion, CompletionOptions, Message, ModelBinding, ModelCatalog, ToolSchema,
    make_completion_error, phrase,
};
use runvil::{Error, ErrorCode};

/// Lists no model and refuses every round as `Unavailable`.
pub(crate) struct Offline;

impl InferenceBroker for Offline {
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

/// Sleeps on tokio's timer, as a Host on tokio does.
pub(crate) struct Clock;

impl Timer for Clock {
    fn sleep(&self, duration: Duration) -> BoxFuture<()> {
        Box::pin(tokio::time::sleep(duration))
    }
}
