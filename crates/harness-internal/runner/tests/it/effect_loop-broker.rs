//! The effect loop answers every `Chat` effect through the inference
//! broker: the broker receives each round's `Round`, with origin `Infer`
//! for a nested `models.infer` and `Chat` for a section's own round, and
//! the run resumes with the broker's completion as each round's answer.

use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};

use harness_runner::RunSetup;
use harness_runner::effect_loop::drive_run;
use harness_runner::performers::{BoxFuture, InferenceBroker};
use harness_runner::recorder::{RecordKind, RunRecorder};
use runvil_engine::Error;
use runvil_engine::cancel::CancelHandle;
use runvil_engine::effect::Round;
use runvil_engine::event::ReplyOrigin;
use runvil_engine::ids::RoundId;
use runvil_engine::model::{
    Completion, CompletionOptions, CompletionResult, Message, ModelBinding, ModelCatalog, ModelId,
    ModelInfo, ModelRole, ThinkingMode, ToolSchema,
};
use runvil_engine::timestamp::Timestamp;
use runvil_engine::{Prompt, Run, RunParams};
use serde_json::{Value, json};

use super::begun_log;
use crate::support::{TEST_SEED, Unused};

/// A program that infers once and then chats once: the nested round
/// first, then the section's own round over its message list.
const INFERS_THEN_CHATS: &str = "---\nname: runner-broker\ndescription: infers then chats\n\
    runvil: 0\n---\n\n\
    # Broker\n\n\
    ## Only\n\n```lua\n\
    local inferred = models.infer('infer round')\n\
    messages:user('chat round')\n\
    models.loop()\n\
    return inferred .. '|' .. messages[#messages].content\n\
    ```\n";

/// A run over [`INFERS_THEN_CHATS`] with the `default` role filled by one
/// current model.
fn infers_then_chats() -> Run {
    let (prompt, _parse_events) =
        Prompt::parse(&|_: &str| Ok(INFERS_THEN_CHATS.to_owned()), "prompt");
    let prompt = Arc::new(prompt.expect("the broker fixture parses"));
    let model = ModelInfo::new(
        ModelId::gateway("m").expect("the model id is valid"),
        "The host's current model",
        NonZeroU32::new(131_072).expect("131072 is non-zero"),
        ThinkingMode::Never,
    );
    let ctx = RunParams::new(TEST_SEED, Timestamp::UNIX_EPOCH)
        .with_models(BTreeMap::from([(ModelRole::Default, model)]));
    Run::new(prompt, "", ctx)
}

/// One round as the broker saw it: the last message's text and the round.
type Seen = (String, Round);

/// Replies `re: <last message>` to every round and keeps each round it
/// was handed.
#[derive(Default)]
struct RecordingBroker {
    rounds: Arc<Mutex<Vec<Seen>>>,
}

impl InferenceBroker for RecordingBroker {
    fn models(&self) -> BoxFuture<Result<ModelCatalog, Error>> {
        unreachable!("the effect loop never lists models")
    }

    fn chat(
        &self,
        _binding: ModelBinding,
        messages: Vec<Message>,
        _tools: Vec<ToolSchema>,
        _options: CompletionOptions,
        round: Round,
    ) -> BoxFuture<Result<Box<Completion>, Error>> {
        let asked = messages
            .last()
            .map(|message| message.content().to_owned())
            .unwrap_or_default();
        self.rounds.lock().unwrap().push((asked.clone(), round));
        Box::pin(async move {
            Completion::from_result(CompletionResult::Text(format!("re: {asked}")), "m")
                .map(Box::new)
        })
    }
}

/// A setup recorded through `recorder` whose broker is the recording
/// broker, beside what it saw.
fn recording(recorder: Arc<dyn RunRecorder>) -> (RunSetup, Arc<Mutex<Vec<Seen>>>) {
    let broker = RecordingBroker::default();
    let rounds = Arc::clone(&broker.rounds);
    (RunSetup::new(recorder, Arc::new(broker)), rounds)
}

#[tokio::test]
async fn the_broker_receives_each_rounds_round() {
    let (recorder, run_id) = begun_log().await;
    let (setup, rounds) = recording(recorder);

    drive_run(
        infers_then_chats(),
        &setup,
        Arc::new(Unused),
        Arc::new(Unused),
        run_id,
        CancelHandle::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        *rounds.lock().unwrap(),
        vec![
            (
                "infer round".to_owned(),
                Round::new(RoundId::new(0), ReplyOrigin::Infer)
            ),
            (
                "chat round".to_owned(),
                Round::new(RoundId::new(1), ReplyOrigin::Chat)
            ),
        ],
        "the infer round is round 0 from `Infer`, and the chat round is round 1 from `Chat`"
    );
}

#[tokio::test]
async fn each_round_resumes_the_run_with_the_brokers_completion() {
    let (recorder, run_id) = begun_log().await;
    let (setup, _rounds) = recording(recorder.clone());

    let result = drive_run(
        infers_then_chats(),
        &setup,
        Arc::new(Unused),
        Arc::new(Unused),
        run_id,
        CancelHandle::new(),
    )
    .await;
    assert_eq!(
        result,
        Ok("re: infer round|re: chat round".to_owned()),
        "the program saw each round's reply as the broker wrote it"
    );
    let replies: Vec<Value> = recorder
        .records(run_id)
        .iter()
        .filter(|record| record.kind == RecordKind::Answer)
        .map(|record| record.payload["Chat"]["Ok"]["reply"].clone())
        .collect();
    assert_eq!(
        replies,
        vec![json!("re: infer round"), json!("re: chat round")],
        "each round's answer record holds the broker's completion"
    );
}
