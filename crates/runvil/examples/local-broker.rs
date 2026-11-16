//! Connects a deterministic local inference broker to the Harness.

use std::num::NonZeroU32;

use runvil::effect::Round;
use runvil::harness::InferenceBroker;
use runvil::model::{
    Completion, CompletionOptions, CompletionResult, Message, ModelBinding, ModelCatalog, ModelId,
    ModelInfo, ThinkingMode, ToolSchema,
};
use runvil::{BoxFuture, Error, Host};

const MODEL: &str = "local-echo";

struct EchoBroker {
    catalog: ModelCatalog,
}

impl InferenceBroker for EchoBroker {
    fn models(&self) -> BoxFuture<'static, Result<ModelCatalog, Error>> {
        let catalog = self.catalog.clone();
        Box::pin(async move { Ok(catalog) })
    }

    fn chat(
        &self,
        _binding: ModelBinding,
        messages: Vec<Message>,
        _tools: Vec<ToolSchema>,
        _options: CompletionOptions,
        _round: Round,
    ) -> BoxFuture<'static, Result<Box<Completion>, Error>> {
        let text = messages
            .last()
            .map(|message| message.content().to_owned())
            .unwrap_or_default();
        Box::pin(async move {
            Completion::from_result(CompletionResult::Text(format!("Echo: {text}")), MODEL)
                .map(Box::new)
        })
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let broker = EchoBroker {
        catalog: ModelCatalog::new([ModelInfo::new(
            ModelId::gateway(MODEL)?,
            "Local echo demonstration",
            NonZeroU32::new(8_192).ok_or("context window must be positive")?,
            ThinkingMode::Never,
        )])?,
    };
    let prompts = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/prompts");
    let host = Host::new(prompts).with_broker(broker);
    let answer = host
        .run("echo.md")
        .args("Hello from the Harness")
        .model(MODEL)
        .text()
        .await?;
    assert_eq!(answer, "Echo: Hello from the Harness");
    println!("{answer}");
    Ok(())
}
