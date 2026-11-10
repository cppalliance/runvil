//! What every suite Host hands a Harness: a Host over the suite's Packs
//! that finds the one prompt a test runs and sleeps on tokio's timer, an
//! offline broker, and that timer.

use std::sync::Arc;
use std::time::Duration;

use runvil::effect::Round;
use runvil::harness::{InferenceBroker, Packs, Timer};
use runvil::model::{
    Completion, CompletionOptions, Message, ModelBinding, ModelCatalog, ToolSchema,
    make_completion_error, phrase,
};
use runvil::plugin::PackRegistry;
use runvil::{BoxFuture, Error, ErrorCode, Harness, Host, RunSetup};

/// The name every suite Host finds its prompt by.
const PROMPT: &str = "prompt.md";

/// Lists no model and refuses every round as `Unavailable`.
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
        let code = ErrorCode::Unavailable;
        Box::pin(async move { Err(make_completion_error(code, phrase(code))) })
    }
}

/// Sleeps on tokio's timer, as a Host on tokio does.
pub(crate) struct Clock;

impl Timer for Clock {
    fn sleep(&self, duration: Duration) -> BoxFuture<'static, ()> {
        Box::pin(tokio::time::sleep(duration))
    }
}

/// A Host's Packs with none but the built-ins.
pub(crate) fn no_packs() -> Packs {
    Packs::new(PackRegistry::new())
}

/// A Harness running `source` with `setup` on a Host over `packs` that
/// finds `source` by its name and sleeps on [`Clock`].
pub(crate) fn launch(packs: Packs, source: &str, setup: RunSetup) -> Harness {
    let source = source.to_owned();
    let host = Arc::new(Host {
        packs,
        prompts: Arc::new(move |name: &str| {
            if name == PROMPT {
                return Ok(source.clone());
            }
            let code = ErrorCode::NotFound;
            Err(Error::new(
                code,
                format!("no prompt named {name}"),
                code.prose(),
            ))
        }),
        timer: Arc::new(Clock),
    });
    let prompt = match host.parse(PROMPT) {
        Ok(prompt) => prompt,
        Err(error) => panic!("the suite's prompt parses: {error:?}"),
    };
    Harness::new(host, prompt, setup)
}
