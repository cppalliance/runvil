//! Shared fixtures: an offline broker, and a Host and a per-run setup
//! built from what a conversation hands it.

// clippy.toml's allow-expect-in-tests covers #[test] functions only, not
// the helpers they share; failing a test by panicking with the invariant
// named is exactly what these are for.
#![expect(
    clippy::expect_used,
    reason = "test helpers fail by panicking with the invariant named"
)]

use std::sync::Arc;

use harness::pack::{HostServices, PackRegistry, Packs};
use harness::record::MemoryRecorder;
use harness::{BoxFuture, Host, InferenceBroker, RunSetup};
use runvil::effect::Round;
use runvil::model::{
    Completion, CompletionOptions, Message, ModelBinding, ModelCatalog, ToolSchema,
    make_completion_error, phrase,
};
use runvil::{Error, ErrorCode};
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

/// A prompt whose one section runs `lua`, declaring the operator-input
/// Pack.
pub(crate) fn prompt(lua: &str) -> String {
    format!(
        "---\nname: fixture\ndescription: a conversation fixture\nrunvil: 0\n\
         uses:\n  - user-input\n---\n\n\
         # Fixture\n\n## Conversation\n\n```lua\n{lua}\n```\n"
    )
}

/// A Host with the operator-input Pack that finds `source` as [`AGENT`],
/// beside the run's setup, recording into `recorder` through the
/// conversation's tee.
pub(crate) fn run_for(
    conversation: &Conversation,
    recorder: Arc<MemoryRecorder>,
    source: String,
) -> (Arc<Host>, RunSetup) {
    let mut packs = PackRegistry::new();
    plugin_user_input::install(&mut packs, serde_json::Value::Null, &HostServices::new())
        .expect("a new registry takes the Pack");
    let host = Arc::new(Host {
        packs: Packs::new(packs),
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
        services: conversation.run_services(),
        ..RunSetup::new(conversation.recorder(recorder), Arc::new(Offline))
    };
    (host, setup)
}
