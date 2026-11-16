//! The default broker for runs that call no model.

use runvil_engine::effect::Round;
use runvil_engine::model::{
    Completion, CompletionOptions, Message, ModelBinding, ModelCatalog, ToolSchema,
    make_completion_error,
};
use runvil_engine::{BoxFuture, Error, ErrorCode};

use crate::performers::InferenceBroker;

pub(crate) struct Offline;

impl InferenceBroker for Offline {
    fn models(&self) -> BoxFuture<'static, Result<ModelCatalog, Error>> {
        Box::pin(async { Ok(ModelCatalog::empty()) })
    }

    fn chat(
        &self,
        _binding: ModelBinding,
        _messages: Vec<Message>,
        _tools: Vec<ToolSchema>,
        _options: CompletionOptions,
        _round: Round,
    ) -> BoxFuture<'static, Result<Box<Completion>, Error>> {
        Box::pin(async {
            Err(make_completion_error(
                ErrorCode::Unavailable,
                "this run has no inference broker",
            ))
        })
    }
}
