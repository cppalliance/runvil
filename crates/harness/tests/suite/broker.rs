//! The inference broker a Host hands `Harness::new`: a section's own
//! round reaches it with origin `Chat`, and a nested `models.infer` round
//! with origin `Infer`, each under its run-wide round id. A broker that
//! does not stream answers `chat_streaming` with its `chat` round and
//! hands the callback no piece.

use std::num::NonZeroU32;
use std::sync::{Arc, Mutex, PoisonError};

use harness::pack::{HostContext, HostServices, PackRegistry};
use harness::record::{MemoryRecorder, RunOutcome};
use harness::vfs::VfsRef;
use harness::{BoxFuture, Harness, HostSnapshot, InferenceBroker, RunRequest, StreamDelta};
use runvil::effect::Round;
use runvil::error::Error;
use runvil::event::ReplyOrigin;
use runvil::ids::RoundId;
use runvil::model::{
    Completion, CompletionOptions, CompletionResult, Message, ModelBinding, ModelCatalog, ModelId,
    ModelInfo, ModelInvocation, ThinkingMode, ToolSchema,
};

use crate::support::Clock;

/// The one model the scripted broker lists and the Host selects.
const MODEL: &str = "scripted-model";

/// A program that infers once and then chats once: the nested round
/// first, then the section's own round over its message list.
const INFERS_THEN_CHATS: &str = "---\nname: rounds\ndescription: infers then chats\n\
    runvil: 0\n---\n\n\
    # Rounds\n\n\
    ## Only\n\n```lua\n\
    local inferred = models.infer('infer round')\n\
    messages:user('chat round')\n\
    models.loop()\n\
    return inferred .. '|' .. messages[#messages].content\n\
    ```\n";

/// Lists its catalog, notes each round it serves, and replies
/// `re: <last message>` to every round.
struct ScriptedBroker {
    catalog: ModelCatalog,
    rounds: Mutex<Vec<Round>>,
}

impl InferenceBroker for ScriptedBroker {
    fn models(&self) -> BoxFuture<Result<ModelCatalog, Error>> {
        let catalog = self.catalog.clone();
        Box::pin(async move { Ok(catalog) })
    }

    fn chat(
        &self,
        _binding: ModelBinding,
        messages: Vec<Message>,
        _tools: Vec<ToolSchema>,
        _options: CompletionOptions,
        round: Round,
    ) -> BoxFuture<Result<Box<Completion>, Error>> {
        self.rounds
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(round);
        let asked = messages
            .last()
            .map(|message| message.content().to_owned())
            .unwrap_or_default();
        Box::pin(async move {
            Completion::from_result(CompletionResult::Text(format!("re: {asked}")), MODEL)
                .map(Box::new)
        })
    }
}

#[tokio::test]
async fn a_sections_round_reaches_the_broker_as_chat_and_a_nested_infer_round_as_infer() {
    let catalog = ModelCatalog::new([ModelInfo::new(
        ModelId::gateway(MODEL).expect("a literal model name is valid"),
        "the scripted model",
        NonZeroU32::new(131_072).expect("131072 is non-zero"),
        ThinkingMode::Never,
    )])
    .expect("one model is a valid catalog");
    let broker = Arc::new(ScriptedBroker {
        catalog,
        rounds: Mutex::new(Vec::new()),
    });
    let harness = Harness::new(
        Arc::new(MemoryRecorder::new()),
        broker.clone(),
        Arc::new(Clock),
        Arc::new(HostContext::new(PackRegistry::new(), HostServices::new())),
        HostServices::new(),
    );
    let report = harness
        .run(RunRequest {
            name: "rounds-1".to_owned(),
            source: INFERS_THEN_CHATS.to_owned(),
            file: "prompt".to_owned(),
            args: String::new(),
            input_text: None,
            vfs: VfsRef::default(),
            host: HostSnapshot {
                selected_model: Some(MODEL.to_owned()),
                ..HostSnapshot::default()
            },
        })
        .await
        .expect("the run reaches an outcome");

    assert_eq!(
        report.outcome,
        RunOutcome::Completed {
            final_text: "re: infer round|re: chat round".to_owned()
        }
    );
    assert_eq!(
        *broker.rounds.lock().expect("the round log is healthy"),
        [
            Round::new(RoundId::new(0), ReplyOrigin::Infer),
            Round::new(RoundId::new(1), ReplyOrigin::Chat),
        ],
        "each round reaches the broker numbered in dispatch order, under the path that sent it"
    );
}

#[tokio::test]
async fn a_broker_that_does_not_stream_answers_chat_streaming_with_its_chat_round() {
    let broker = ScriptedBroker {
        catalog: ModelCatalog::new([]).expect("an empty catalog is valid"),
        rounds: Mutex::new(Vec::new()),
    };
    let pieces: Arc<Mutex<Vec<StreamDelta>>> = Arc::default();
    let sink = Arc::clone(&pieces);
    let round = Round::new(RoundId::new(3), ReplyOrigin::Chat);
    let binding = ModelBinding::new(
        "writer",
        "the round's model",
        ModelId::gateway(MODEL).expect("a literal model name is valid"),
        ModelInvocation::new(),
        NonZeroU32::new(131_072).expect("131072 is non-zero"),
    );

    let completion = broker
        .chat_streaming(
            binding,
            vec![Message::user("hi")],
            Vec::new(),
            CompletionOptions::new(MODEL),
            round,
            Arc::new(move |piece| {
                sink.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(piece);
            }),
        )
        .await
        .expect("the scripted round completes");

    assert_eq!(
        completion.result(),
        &CompletionResult::Text("re: hi".to_owned()),
        "the default runs the broker's own chat round"
    );
    assert_eq!(
        *broker.rounds.lock().expect("the round log is healthy"),
        [round],
        "the chat round ran once, under the round it was handed"
    );
    assert!(
        pieces.lock().expect("the piece log is healthy").is_empty(),
        "a broker that does not stream hands the callback no piece"
    );
}
