//! The scripted Host the Harness suites run against: a broker that lists
//! one model and plays each round from a script, performers whose every
//! future is held until it is torn down, and a Pack whose one tool is
//! held.

use std::num::NonZeroU32;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_runner::Packs;
use harness_runner::performers::{InferenceBroker, Timer};
use runvil_engine::effect::Round;
use runvil_engine::model::{
    Completion, CompletionOptions, CompletionResult, Message, ModelBinding, ModelCatalog, ModelId,
    ModelInfo, ThinkingMode, ToolCall, ToolSchema, make_completion_error, phrase,
};
use runvil_engine::{Error, ErrorCode};
use runvil_plugin::{
    BoxFuture, Pack, PackFactory, PackId, PackInfo, ToolContext, ToolId, ToolInfo, ToolOutput,
};
use serde_json::{Value, json};

use crate::support::registry;

/// The one model the scripted broker lists.
pub(crate) const MODEL: &str = "m";

/// What a fake keeps, shared with the test that reads it.
pub(crate) type Kept<T> = Arc<Mutex<Vec<T>>>;

/// Plays one model round: the round and its messages in, its answer out.
type ChatScript =
    dyn Fn(Round, Vec<Message>) -> BoxFuture<'static, Result<Box<Completion>, Error>> + Send + Sync;

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
        script: impl Fn(Round, Vec<Message>) -> BoxFuture<'static, Result<Box<Completion>, Error>>
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
    fn models(&self) -> BoxFuture<'static, Result<ModelCatalog, Error>> {
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
    ) -> BoxFuture<'static, Result<Box<Completion>, Error>> {
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
    fn hold<T: Send + 'static>(self: &Arc<Self>) -> BoxFuture<'static, T> {
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
    fn sleep(&self, _duration: Duration) -> BoxFuture<'static, ()> {
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

/// The hold Pack a second time, as `keep`: one tool, `keep/hold`, which
/// always survives a stop.
const KEEP: PackInfo = PackInfo::new("tests/keep");

struct HoldPack {
    held: Arc<Held>,
    tools: Vec<ToolInfo>,
}

/// The hold Pack's factory over `held`, its tool marked to survive a stop
/// when `survives_stop` is set.
fn holding(held: &Arc<Held>, survives_stop: bool) -> impl PackFactory + use<> {
    let held = Arc::clone(held);
    move |name: &PackId| -> Result<Arc<dyn Pack>, Error> {
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
    ) -> BoxFuture<'a, Result<ToolOutput, Error>> {
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

/// A Host holding the hold Pack over `held`, and as `keep` the hold Pack
/// over `kept`, whose tool survives a stop.
pub(crate) fn keeping_hold_host(held: &Arc<Held>, kept: &Arc<Held>) -> Packs {
    let mut packs = registry(&[]);
    packs.insert(HOLD, holding(held, false)).unwrap();
    packs.insert(KEEP, holding(kept, true)).unwrap();
    Packs::new(packs)
}

/// A Host that installed the hold Pack over `held`, which reaches only a
/// run that declares it.
fn host_holding(held: &Arc<Held>, survives_stop: bool) -> Packs {
    let mut packs = registry(&[]);
    packs.insert(HOLD, holding(held, survives_stop)).unwrap();
    Packs::new(packs)
}
