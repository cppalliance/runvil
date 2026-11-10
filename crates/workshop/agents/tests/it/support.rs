//! Shared fixtures: an offline broker, and a Host and a per-run setup
//! built around the recorder tee a conversation hands it.

use std::sync::Arc;

use runvil::harness::{BoxFuture, InferenceBroker, MemoryRecorder, Packs};
use runvil::plugin::PackRegistry;
use runvil::{Host, RunSetup};
use runvil_engine::effect::Round;
use runvil_engine::model::{
    Completion, CompletionOptions, Message, ModelBinding, ModelCatalog, ToolSchema,
    make_completion_error, phrase,
};
use runvil_engine::{Error, ErrorCode};
use workshop_agents::{Conversation, TokioTimer};

/// The name the fixture Host finds its one agent by.
pub(crate) const AGENT: &str = "fixture";

/// Lists no model and refuses every round as `Unavailable`.
struct Offline;

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

/// A prompt whose one section runs `lua`, declaring the user Pack, as a
/// conversation's agent does.
pub(crate) fn prompt(lua: &str) -> String {
    format!(
        "---\nname: fixture\ndescription: a conversation fixture\nrunvil: 0\n\
         uses:\n  - user\n---\n\n\
         # Fixture\n\n## Conversation\n\n```lua\n{lua}\n```\n"
    )
}

/// A Host that finds `source` as [`AGENT`], beside the setup of a run with
/// a user, recording into `recorder` through the conversation's tee.
pub(crate) fn run_for(
    conversation: &Conversation,
    recorder: Arc<MemoryRecorder>,
    source: String,
) -> (Arc<Host>, RunSetup) {
    let host = Arc::new(Host {
        packs: Packs::new(PackRegistry::new()),
        prompts: Arc::new(move |name: &str| {
            if name == AGENT {
                return Ok(source.clone());
            }
            let code = ErrorCode::NotFound;
            Err(Error::new(
                code,
                format!("no agent named {name}"),
                code.prose(),
            ))
        }),
        timer: Arc::new(TokioTimer),
    });
    let setup = RunSetup {
        has_user: true,
        ..RunSetup::new(conversation.recorder(recorder), Arc::new(Offline))
    };
    (host, setup)
}
