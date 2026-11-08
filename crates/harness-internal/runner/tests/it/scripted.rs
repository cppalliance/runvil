//! The scripted Host the Harness suites run against: a broker that lists
//! one model and plays each round from a script, performers whose every
//! future is held until it is torn down, a Pack whose one tool is
//! held, and an operator who answers each question with the next text a
//! test sends.

use std::num::NonZeroU32;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_runner::Packs;
use harness_runner::performers::{BoxFuture, InferenceBroker, Timer};
use runvil::effect::Round;
use runvil::make_tool_error;
use runvil::model::{
    Completion, CompletionOptions, CompletionResult, Message, ModelBinding, ModelCatalog, ModelId,
    ModelInfo, ThinkingMode, ToolCall, ToolSchema, make_completion_error, phrase,
};
use runvil::{Error, ErrorCode};
use runvil_plugin::{
    HostServices, Pack, PackFactory, PackFuture, PackId, PackInfo, ToolContext, ToolId, ToolInfo,
    ToolOutput,
};
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::asker::{ASKER, AskBroker, broker_services};
use crate::support::registry;

/// The one model the scripted broker lists.
pub(crate) const MODEL: &str = "m";

/// What a fake keeps, shared with the test that reads it.
pub(crate) type Kept<T> = Arc<Mutex<Vec<T>>>;

/// Plays one model round: the round and its messages in, its answer out.
type ChatScript =
    dyn Fn(Round, Vec<Message>) -> BoxFuture<Result<Box<Completion>, Error>> + Send + Sync;

/// What a scripted broker's `models()` does.
#[derive(Clone, Copy)]
pub(crate) enum Listing {
    /// Lists the one scripted model.
    Lists,
    /// Fails under this code with the code's fixed phrase.
    Fails(ErrorCode),
    /// Never answers.
    Holds,
}

/// Lists the scripted model and plays every round through its script,
/// keeping each round it was handed and counting the listings it was
/// asked for.
pub(crate) struct ScriptedBroker {
    listing: Listing,
    script: Arc<ChatScript>,
    rounds: Kept<Round>,
    listings: Arc<AtomicUsize>,
}

impl ScriptedBroker {
    /// A broker that plays every round through `script`.
    pub(crate) fn new(
        script: impl Fn(Round, Vec<Message>) -> BoxFuture<Result<Box<Completion>, Error>>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self {
            listing: Listing::Lists,
            script: Arc::new(script),
            rounds: Kept::default(),
            listings: Arc::default(),
        }
    }

    /// A broker that replies `re: <last message>` to every round.
    pub(crate) fn replying() -> Self {
        Self::new(|_round, messages| {
            let asked = messages
                .last()
                .map(|message| message.content().to_owned())
                .unwrap_or_default();
            Box::pin(async move { reply(format!("re: {asked}")) })
        })
    }

    /// This broker with `listing` as what its `models()` does.
    pub(crate) fn listing(self, listing: Listing) -> Self {
        Self { listing, ..self }
    }

    /// The rounds this broker keeps, readable after it moved into a
    /// Harness.
    pub(crate) fn rounds(&self) -> Kept<Round> {
        Arc::clone(&self.rounds)
    }

    /// The count of listings this broker was asked for.
    pub(crate) fn listings(&self) -> Arc<AtomicUsize> {
        Arc::clone(&self.listings)
    }
}

impl InferenceBroker for ScriptedBroker {
    fn models(&self) -> BoxFuture<Result<ModelCatalog, Error>> {
        self.listings.fetch_add(1, Ordering::SeqCst);
        match self.listing {
            Listing::Lists => Box::pin(async { Ok(catalog()) }),
            Listing::Fails(code) => {
                Box::pin(async move { Err(make_completion_error(code, phrase(code))) })
            }
            Listing::Holds => Box::pin(std::future::pending()),
        }
    }

    fn chat(
        &self,
        _binding: ModelBinding,
        messages: Vec<Message>,
        _tools: Vec<ToolSchema>,
        _options: CompletionOptions,
        round: Round,
    ) -> BoxFuture<Result<Box<Completion>, Error>> {
        self.rounds.lock().unwrap().push(round);
        (self.script)(round, messages)
    }
}

/// The one model the scripted broker lists, with a 131072-token window.
pub(crate) fn scripted_model() -> ModelInfo {
    ModelInfo::new(
        ModelId::gateway(MODEL).expect("the model id is valid"),
        "The scripted model",
        NonZeroU32::new(131_072).expect("131072 is non-zero"),
        ThinkingMode::Never,
    )
}

/// The scripted catalog: the one model.
fn catalog() -> ModelCatalog {
    ModelCatalog::new([scripted_model()]).expect("one model is a valid catalog")
}

/// A text completion of `text` from the scripted model.
pub(crate) fn reply(text: impl Into<String>) -> Result<Box<Completion>, Error> {
    Completion::from_result(CompletionResult::Text(text.into()), MODEL).map(Box::new)
}

/// The round that asks for the held tool: the model's first round calls
/// its wire name `harness_hold`, and every later round replies with text.
pub(crate) fn calls_hold_once() -> ScriptedBroker {
    ScriptedBroker::new(|round, _messages| {
        Box::pin(async move {
            if round.id.get() > 0 {
                return reply("done");
            }
            let call = ToolCall::from_parts("call-1", "harness_hold", json!({}))?;
            Completion::from_result(CompletionResult::ToolCalls(vec![call]), MODEL).map(Box::new)
        })
    })
}

/// Counts the futures a held performer started and the ones torn down. A
/// held future never resolves, so only a drop ends it.
#[derive(Default)]
pub(crate) struct Held {
    started: AtomicUsize,
    dropped: AtomicUsize,
}

impl Held {
    pub(crate) fn started(&self) -> usize {
        self.started.load(Ordering::SeqCst)
    }

    pub(crate) fn dropped(&self) -> usize {
        self.dropped.load(Ordering::SeqCst)
    }

    /// A future that counts itself started on its first poll and dropped
    /// when torn down, and never resolves.
    fn hold<T: Send + 'static>(self: &Arc<Self>) -> BoxFuture<T> {
        let held = Arc::clone(self);
        Box::pin(async move {
            held.started.fetch_add(1, Ordering::SeqCst);
            let _dropped = CountOnDrop(held);
            std::future::pending::<T>().await
        })
    }
}

/// Counts one drop on its [`Held`].
struct CountOnDrop(Arc<Held>);

impl Drop for CountOnDrop {
    fn drop(&mut self) {
        self.0.dropped.fetch_add(1, Ordering::SeqCst);
    }
}

/// A timer whose every sleep is held.
pub(crate) struct HeldTimer(pub(crate) Arc<Held>);

impl Timer for HeldTimer {
    fn sleep(&self, _duration: Duration) -> BoxFuture<()> {
        self.0.hold()
    }
}

/// A broker whose every round is held.
pub(crate) fn held_broker(held: &Arc<Held>) -> ScriptedBroker {
    let held = Arc::clone(held);
    ScriptedBroker::new(move |_round, _messages| held.hold())
}

/// The fixture Pack `harness`: one tool, `harness/hold`, whose every
/// call is held, and which a stop leaves in flight when its factory says
/// so.
const HOLD: PackInfo = PackInfo::new("tests/harness");

struct HoldPack {
    held: Arc<Held>,
    tools: Vec<ToolInfo>,
}

/// The hold Pack's factory over `held`, its tool marked to survive a stop
/// when `survives_stop` is set.
fn holding(held: &Arc<Held>, survives_stop: bool) -> impl PackFactory + use<> {
    let held = Arc::clone(held);
    move |name: &PackId, _run: &HostServices| -> Result<Arc<dyn Pack>, Error> {
        let info = ToolInfo::new(
            ToolId::parse(&format!("{name}/hold")).unwrap(),
            "Hold until the call is dropped.",
            json!({ "type": "object", "properties": {} }),
        )
        .survives_stop(survives_stop);
        Ok(Arc::new(HoldPack {
            held: Arc::clone(&held),
            tools: vec![info],
        }))
    }
}

impl Pack for HoldPack {
    fn tools(&self) -> Vec<ToolInfo> {
        self.tools.clone()
    }

    fn call<'a>(
        &'a self,
        _cx: ToolContext<'a>,
        _args: Value,
    ) -> PackFuture<'a, Result<ToolOutput, Error>> {
        self.held.hold()
    }
}

/// A Host holding the hold Pack over `held`.
pub(crate) fn hold_host(held: &Arc<Held>) -> Packs {
    host_holding(held, false)
}

/// A Host holding the hold Pack over `held`, its tool marked to
/// survive a stop.
pub(crate) fn surviving_hold_host(held: &Arc<Held>) -> Packs {
    host_holding(held, true)
}

/// A Host that installed the hold Pack over `held` and the fixture ask
/// Pack, which reach only a run that declares them.
fn host_holding(held: &Arc<Held>, survives_stop: bool) -> Packs {
    let mut packs = registry(&[ASKER]);
    packs.insert(HOLD, holding(held, survives_stop)).unwrap();
    Packs::new(packs)
}

/// The operator: counts each question asked and each one torn down
/// unanswered, and answers each with the next text the test sends.
pub(crate) struct Operator {
    pub(crate) asked: Arc<AtomicUsize>,
    pub(crate) abandoned: Arc<AtomicUsize>,
    answers: tokio::sync::Mutex<mpsc::UnboundedReceiver<String>>,
}

impl Operator {
    /// The operator, beside the sender the test answers through.
    pub(crate) fn new() -> (Arc<Self>, mpsc::UnboundedSender<String>) {
        let (answer, answers) = mpsc::unbounded_channel();
        let operator = Arc::new(Self {
            asked: Arc::default(),
            abandoned: Arc::default(),
            answers: tokio::sync::Mutex::new(answers),
        });
        (operator, answer)
    }

    /// Run services whose input broker is this operator.
    pub(crate) fn services(self: &Arc<Self>) -> HostServices {
        broker_services(Arc::clone(self) as Arc<dyn AskBroker>)
    }
}

#[async_trait::async_trait]
impl AskBroker for Operator {
    async fn wait(&self) -> Result<String, Error> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        let mut unanswered = Unanswered(Some(Arc::clone(&self.abandoned)));
        let text = self.answers.lock().await.recv().await;
        unanswered.0 = None;
        text.ok_or_else(|| make_tool_error(ErrorCode::ToolOther, "the operator left"))
    }
}

/// Counts a question torn down before its answer arrived.
struct Unanswered(Option<Arc<AtomicUsize>>);

impl Drop for Unanswered {
    fn drop(&mut self) {
        if let Some(abandoned) = &self.0 {
            abandoned.fetch_add(1, Ordering::SeqCst);
        }
    }
}
