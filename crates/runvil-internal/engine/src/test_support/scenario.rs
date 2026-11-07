//! The run-level test driver. A [`Scenario`] holds a run's setup as data:
//! its source, args, run name, seed, params edits, store, tools, model,
//! the sets it fills in before the run starts, the effects it holds, and
//! the effect that cancels it. It parses the source, builds the run, and
//! steps it on the calling thread, answering every effect it does not hold
//! the moment it is issued, and returns a [`Trace`] of what the run
//! produced. Nothing awaits or sleeps: a timer fires at once, a tool is a
//! closure, and the model is a script or a closure. Only a canceller runs
//! on a second thread, and the scenario joins it.
//!
//! The serial helpers the scheduler-level suites drive a bare [`Run`] with
//! sit beside it in `scenario_serial`.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use runvil_lua::ToolOutputKind;
use runvil_model_client::model::ModelInvocation;
use runvil_types::cancel::CancelHandle;
use runvil_types::error::ErrorCode;
use runvil_types::event::Event;
use runvil_types::seed::RunSeed;
use runvil_types::timestamp::Timestamp;
use runvil_types::tools::{ToolCatalog, ToolId, ToolInfo, tool_error};
use runvil_vfs::{Origin, VfsError, VfsRef};

use super::recording::{
    DebugCapture, DebugEvent, NullObserver, Observation, Observer, forward_one,
};
use super::scripted_chat::ScriptedChat;
use super::{FnTool, drive_step, forward};
use crate::Error;
use crate::execute::{
    AnswerRecord, Effect, EffectAnswer, EffectId, EffectRecord, Run, RunLimits, RunParams,
};
use crate::lua::{ToolBinding, run_store_op};
use crate::model::{
    ModelBinding, ModelId, ModelInfo, ModelRole, ModelSet, ThinkingMode, completion_error,
};
use crate::prompt::Prompt;

pub(crate) use super::scenario_serial::{
    Batching, chat_answer, drive_batched, echo_chat, infer_prompt, only_effect, pass_through,
    round_trip, text_of,
};

/// The run name a test that does not choose one runs under.
pub(crate) const EXECUTION: &str = "execute-test";

/// The fixed caller inputs every test run shares: a seed and a start instant
/// a test that does not care about them never has to choose. The tests of
/// the inputs themselves (`run_inputs`) build their params directly.
pub(crate) const TEST_SEED: RunSeed = RunSeed::from_bytes([1; 32]);
pub(crate) const TEST_STARTED_AT: Timestamp = Timestamp::from_unix_millis(1_700_000_000_000);

/// The gateway model a bound scenario's `default` role resolves to.
pub(crate) const TEST_MODEL: &str = "claude-sonnet-4-6";

/// The gateway model `id` as the tests bind it: a 131072-token window and
/// switchable thinking.
#[track_caller]
pub(crate) fn test_model(id: &str) -> ModelInfo {
    test_model_with(id, 131_072, ThinkingMode::Switchable)
}

/// The gateway model `id` on a `window`-token context window, with
/// `thinking`.
#[track_caller]
pub(crate) fn test_model_with(id: &str, window: u32, thinking: ThinkingMode) -> ModelInfo {
    ModelInfo::new(
        ModelId::gateway(id).expect("the test model id is valid"),
        "A general model for tests",
        NonZeroU32::new(window).expect("the test model's window is non-zero"),
        thinking,
    )
}

/// The binding `alias` to the gateway model `model` as a test fills a
/// model set by hand: described as `description`, on a 4096-token window,
/// and with no request settings.
#[track_caller]
pub(crate) fn test_binding(alias: &str, description: &str, model: &str) -> ModelBinding {
    ModelBinding::new(
        alias,
        description,
        ModelId::gateway(model).expect("the test model id is valid"),
        ModelInvocation::new(),
        NonZeroU32::new(4096).expect("4096 is non-zero"),
    )
}

/// Asserts `error` is a model failure of `code` whose prose is `prose`.
#[track_caller]
pub(crate) fn assert_model_failure(error: &Error, code: ErrorCode, prose: &str) {
    assert_eq!(
        (error.code, error.prose.as_str()),
        (code, prose),
        "{error:?}"
    );
}

/// The edit a scenario applies to its run's params before the run is
/// built.
type ParamsEdit = Box<dyn FnOnce(RunParams) -> RunParams>;

/// A test over an effect: which effects a scenario holds, or which answer
/// signals its canceller.
type Matcher = Box<dyn Fn(&Effect) -> bool>;

/// How a scenario answers its run's model rounds.
enum Chat {
    /// From a script.
    Script(ScriptedChat),
    /// Through a closure handed each round's effect.
    Closure(Box<dyn Fn(&Effect) -> EffectAnswer>),
}

/// How a scenario answers its run's effects: a store operation on the
/// effect's own access, a timer at once, a render with every offered tool
/// passed through, a tool call through the scenario's tool of that id,
/// and a model round from its script or closure, unless the scenario
/// holds the effect.
struct Answers {
    tools: BTreeMap<ToolId, FnTool>,
    chat: Option<Chat>,
    hold: Option<Matcher>,
    /// Which effect wakes the canceller, and the wake.
    signal: Option<(Matcher, Sender<()>)>,
    /// Whether the canceller was woken.
    signalled: Cell<bool>,
}

impl Answers {
    /// Answers `effect` at once, or `None` when the scenario holds it,
    /// waking the canceller either way when `effect` is its signal.
    fn answer(&self, effect: &Effect) -> Option<EffectAnswer> {
        let held = self.hold.as_ref().is_some_and(|held| held(effect));
        let answer = (!held).then(|| self.perform(effect));
        if let Some((signal, wake)) = &self.signal
            && signal(effect)
        {
            // Only the first signal wakes the canceller; a later send
            // finds it gone.
            let _ = wake.send(());
            self.signalled.set(true);
        }
        answer
    }

    /// The answer `effect` gets when the scenario does not hold it. A call
    /// to a tool the scenario lacks is refused, and so is a model round
    /// when the scenario has no model.
    fn perform(&self, effect: &Effect) -> EffectAnswer {
        match effect {
            Effect::Chat {
                binding,
                messages,
                tools,
                ..
            } => match &self.chat {
                Some(Chat::Script(chat)) => {
                    let answer = chat.complete_now(messages, tools, &binding.completion_options());
                    EffectAnswer::Chat(answer.map(Box::new))
                }
                Some(Chat::Closure(answer)) => answer(effect),
                None => EffectAnswer::Chat(Err(completion_error(
                    ErrorCode::Unavailable,
                    "model access is turned off or not configured",
                ))),
            },
            Effect::Render { offered, .. } => EffectAnswer::Render(offered.clone()),
            Effect::ToolCall { tool, args, .. } => match self.tools.get(tool) {
                Some(tool) => EffectAnswer::ToolCall(tool.call_now(args.clone())),
                None => EffectAnswer::ToolCall(Err(tool_error(
                    ErrorCode::ToolOther,
                    "the tool the call names has no implementation in the test driver's tool table",
                ))),
            },
            Effect::Vfs { access, op } => EffectAnswer::Vfs(run_store_op(access, op.clone())),
            Effect::Timer { .. } => EffectAnswer::Timer,
        }
    }
}

/// The test store: a VFS handle, a fresh default one (a memory store at
/// `/`) unless the test supplies its own, whose `read` and `glob` helpers
/// each go through a fresh, immediately dropped access and its store view.
/// A short-lived access per call is what keeps seeding and post-run
/// assertions conflict-free: each access is its own scope, and its claims
/// die with it, so a held seeder access would meet the run's own scope as
/// a false race.
pub(crate) struct TestStore(Mutex<VfsRef>);

impl TestStore {
    pub(crate) fn new() -> TestStore {
        TestStore(Mutex::new(VfsRef::default()))
    }

    /// Wraps a caller-built handle - a gated backend, say - in the test
    /// store's seeding and post-run assertion helpers.
    pub(crate) fn from_vfs(vfs: VfsRef) -> TestStore {
        TestStore(Mutex::new(vfs))
    }

    /// The handle the run and the params builders take: the run's whole
    /// filesystem, store included.
    pub(crate) fn vfs(&self) -> VfsRef {
        self.0
            .lock()
            .expect("the store lock is not poisoned")
            .clone()
    }

    pub(crate) fn read(&self, path: &str) -> Result<String, VfsError> {
        self.vfs()
            .acquire_store(Origin::new("TestStore::read"))?
            .read_string(path)
    }

    pub(crate) fn glob(&self, pattern: &str) -> Result<Vec<String>, VfsError> {
        self.vfs()
            .acquire_store(Origin::new("TestStore::glob"))?
            .glob(pattern)
    }
}

/// A run's setup as data. [`run`](Scenario::run) drives it to its end;
/// [`start`](Scenario::start) hands back the run to step by hand.
#[must_use]
pub(crate) struct Scenario {
    source: String,
    args: String,
    name: String,
    seed: RunSeed,
    params: ParamsEdit,
    store: TestStore,
    /// The catalog's tools.
    tools: Vec<FnTool>,
    /// The catalog's tools with no implementation behind them.
    listed: Vec<ToolInfo>,
    prefill: Prefill,
    chat: Option<Chat>,
    hold: Option<Matcher>,
    signal: Option<Matcher>,
    /// The role map the run fills its model set from.
    roles: BTreeMap<ModelRole, ModelInfo>,
    /// The cancel flag the run's params take, which the scenario keeps.
    cancel: CancelHandle,
}

impl Scenario {
    /// A run of `source` named [`EXECUTION`], under [`TEST_SEED`] and
    /// [`TEST_STARTED_AT`], with empty args, a fresh memory store, no tools,
    /// no model, nothing filled in, and no held effect.
    pub(crate) fn new(source: impl Into<String>) -> Scenario {
        Scenario {
            source: source.into(),
            args: String::new(),
            name: EXECUTION.to_owned(),
            seed: TEST_SEED,
            params: Box::new(|ctx| ctx),
            store: TestStore::new(),
            tools: Vec::new(),
            listed: Vec::new(),
            prefill: Prefill::default(),
            chat: None,
            hold: None,
            signal: None,
            roles: BTreeMap::new(),
            cancel: CancelHandle::new(),
        }
    }

    /// Sets the args string the run receives.
    pub(crate) fn args(mut self, args: &str) -> Scenario {
        args.clone_into(&mut self.args);
        self
    }

    /// Sets the run name, which the source is also parsed under.
    pub(crate) fn name(mut self, name: &str) -> Scenario {
        name.clone_into(&mut self.name);
        self
    }

    /// Sets the seed the run's untrusted-envelope nonce derives from.
    pub(crate) fn seed(mut self, seed: RunSeed) -> Scenario {
        self.seed = seed;
        self
    }

    /// Makes the [`TEST_MODEL`] the run's `default` model, so the run
    /// fills all five roles with it.
    pub(crate) fn bound(self) -> Scenario {
        self.current_model(test_model(TEST_MODEL))
    }

    /// Makes `model` the run's `default` model, which fills every role the
    /// map gives no model of its own.
    pub(crate) fn current_model(self, model: ModelInfo) -> Scenario {
        self.role_model(ModelRole::Default, model)
    }

    /// Gives `role` its own model in the role map the run fills its model
    /// set from.
    pub(crate) fn role_model(mut self, role: ModelRole, model: ModelInfo) -> Scenario {
        self.roles.insert(role, model);
        self
    }

    /// Runs under `limits` in place of the default limits.
    pub(crate) fn limits(self, limits: RunLimits) -> Scenario {
        self.params(move |ctx| ctx.with_limits(limits))
    }

    /// Adds `edit` to the edits applied to the run's params, after the
    /// scenario's own settings, its role map and cancel flag included. An
    /// edit that replaces the cancel flag leaves [`cancel_when`] and
    /// [`Driving::cancel`] setting a flag the run never reads.
    ///
    /// [`cancel_when`]: Scenario::cancel_when
    pub(crate) fn params(
        mut self,
        edit: impl FnOnce(RunParams) -> RunParams + 'static,
    ) -> Scenario {
        let earlier = self.params;
        self.params = Box::new(move |ctx| edit(earlier(ctx)));
        self
    }

    /// Runs over the caller-built handle `vfs` in place of a fresh store.
    pub(crate) fn vfs(mut self, vfs: VfsRef) -> Scenario {
        self.store = TestStore::from_vfs(vfs);
        self
    }

    /// Adds `tool` to the run's catalog and answers its calls with it.
    pub(crate) fn tool(mut self, tool: FnTool) -> Scenario {
        self.tools.push(tool);
        self
    }

    /// Fills the run's model set with `set` before the run starts, as the
    /// H1 pass would leave it, in place of the set the role map fills.
    pub(crate) fn models(mut self, set: ModelSet) -> Scenario {
        self.prefill.models = Some(set);
        self
    }

    /// Adds each tool of `bindings` to the run's catalog under the
    /// binding's alias as its wire name, as [`offer`](Scenario::offer)
    /// does, with no scenario tool behind it, so a call to it is refused.
    pub(crate) fn offering(mut self, bindings: Vec<ToolBinding>) -> Scenario {
        self.listed.extend(bindings.into_iter().map(|binding| {
            ToolInfo::new(
                binding.id().clone(),
                binding.description(),
                binding.schema().clone(),
            )
            .wire_name(binding.alias())
            .structured(binding.output_kind() == ToolOutputKind::Structured)
            .describes_tools(binding.describes_tools())
        }));
        self
    }

    /// Adds `tool` to the run's catalog under the wire name `alias` and
    /// answers its calls with it.
    pub(crate) fn offer(mut self, alias: &str, tool: FnTool) -> Scenario {
        self.tools.push(tool.wire_name(alias));
        self
    }

    /// Exposes the raw `tools.call_as_model` shim in every section VM, so
    /// a section can issue a model-issued tool call itself.
    pub(crate) fn raw_shims(mut self) -> Scenario {
        self.prefill.raw_shims = true;
        self
    }

    /// Answers the run's model rounds from `chat`, which shares its script
    /// and its record of every round with the caller's copy.
    pub(crate) fn chat(mut self, chat: &ScriptedChat) -> Scenario {
        self.chat = Some(Chat::Script(chat.clone()));
        self
    }

    /// Answers the run's model rounds through `answer`, handed each round's
    /// effect, for a model a script cannot play.
    pub(crate) fn chat_with(
        mut self,
        answer: impl Fn(&Effect) -> EffectAnswer + 'static,
    ) -> Scenario {
        self.chat = Some(Chat::Closure(Box::new(answer)));
        self
    }

    /// Leaves every effect `held` matches unanswered while the run keeps
    /// stepping, so a cancel can land between steps. Each held effect the
    /// run withdraws is answered `Dropped`, so the step that decides the
    /// run's result drops them all.
    pub(crate) fn hold(mut self, held: impl Fn(&Effect) -> bool + 'static) -> Scenario {
        self.hold = Some(Box::new(held));
        self
    }

    /// Cancels the run from a second thread once the scenario answers or
    /// holds an effect `signal` matches. The thread waits for that effect
    /// untimed, so the cancel lands at some step after the signal, not
    /// necessarily the next. The stepper joins the thread once a step
    /// answers nothing and holds nothing new, so a run left waiting only on
    /// held effects always sees the cancel before it moves again. Use it
    /// only where the outcome is the same whichever step the cancel lands
    /// in. Either way, the scenario joins the thread before it hands back
    /// the trace.
    pub(crate) fn cancel_when(mut self, signal: impl Fn(&Effect) -> bool + 'static) -> Scenario {
        self.signal = Some(Box::new(signal));
        self
    }

    /// Parses the source, builds the run, and starts it.
    ///
    /// # Panics
    /// When the source fails to parse, naming the error.
    #[track_caller]
    pub(crate) fn start(mut self) -> Driving {
        let (prompt, events, ctx) = self.params_of();
        let (signal, signalled) = match self.signal {
            Some(signal) => {
                let (sender, receiver) = mpsc::channel();
                (Some((signal, sender)), Some(receiver))
            }
            None => (None, None),
        };
        let answers = Answers {
            tools: self
                .tools
                .into_iter()
                .map(|tool| (tool.id(), tool))
                .collect(),
            chat: self.chat,
            hold: self.hold,
            signal,
            signalled: Cell::new(false),
        };
        let run = self.prefill.start(Arc::new(prompt), &self.args, ctx);
        let canceller = signalled.map(|signalled| {
            let cancel = self.cancel.clone();
            thread::spawn(move || {
                if signalled.recv().is_ok() {
                    cancel.cancel();
                }
            })
        });
        Driving {
            run,
            result: None,
            answers,
            parsed: events.len(),
            events,
            effects: Vec::new(),
            held: Vec::new(),
            store: self.store,
            cancel: self.cancel,
            canceller,
        }
    }

    /// Drives the run to its end and returns what it produced.
    ///
    /// # Panics
    /// When the source fails to parse, naming the error.
    #[track_caller]
    pub(crate) fn run(self) -> Trace {
        self.start().finish()
    }

    /// Parses the source, builds the run, and hands it back unstarted, for
    /// a test that steps it and answers its effects by hand. The
    /// scenario's tools still shape the catalog, but its answers, holds,
    /// and canceller play no part.
    ///
    /// # Panics
    /// When the source fails to parse, naming the error.
    #[track_caller]
    pub(crate) fn into_run(mut self) -> Run {
        let (prompt, _, ctx) = self.params_of();
        self.prefill.start(Arc::new(prompt), &self.args, ctx)
    }

    /// Parses the source and builds the run's params, beside the parse
    /// events.
    #[track_caller]
    fn params_of(&mut self) -> (Prompt, Vec<Event>, RunParams) {
        let (parsed, events) = Prompt::parse(&self.source, "prompt", &self.name);
        let prompt = match parsed {
            Ok(prompt) => prompt,
            Err(error) => panic!("expected the scenario source to parse, got {error}"),
        };
        let mut ctx = RunParams::new(self.name.clone(), self.seed, TEST_STARTED_AT)
            .with_vfs(self.store.vfs())
            .with_models(std::mem::take(&mut self.roles))
            .with_cancel(self.cancel.clone());
        if !self.tools.is_empty() || !self.listed.is_empty() {
            let infos = self.tools.iter().map(FnTool::info);
            let catalog = ToolCatalog::new(infos.chain(self.listed.iter().cloned()))
                .expect("the scenario's tool ids differ");
            ctx = ctx.with_tools(catalog);
        }
        let edit = std::mem::replace(&mut self.params, Box::new(|ctx| ctx));
        (prompt, events, edit(ctx))
    }
}

/// What a scenario fills into its run's state before the run starts, in
/// place of what the role map and the H1 pass would leave there.
#[derive(Default)]
struct Prefill {
    models: Option<ModelSet>,
    raw_shims: bool,
}

impl Prefill {
    /// Builds the run of `prompt` under `params` with the state filled in,
    /// or as [`Run::new`] builds it when there is nothing to fill.
    fn start(self, prompt: Arc<Prompt>, args: &str, params: RunParams) -> Run {
        if self.models.is_none() && !self.raw_shims {
            return Run::new(prompt, args, params);
        }
        Run::new_for_test(prompt, args, params, |mut state| {
            if let Some(set) = self.models {
                state = state.with_models_for_test(set);
            }
            if self.raw_shims {
                state.expose_raw_shims_for_test();
            }
            state
        })
    }
}

/// A started scenario, stepped by hand or to its end.
pub(crate) struct Driving {
    /// The run, kept once it is done.
    run: Run,
    /// The run's result, once it is done.
    result: Option<Result<String, Error>>,
    answers: Answers,
    /// The parse events, then every event the run reported, in order.
    events: Vec<Event>,
    /// How many of the events the parse reported.
    parsed: usize,
    effects: Vec<(EffectRecord, AnswerRecord)>,
    /// The effects the scenario holds, unanswered until a test releases
    /// them or the run withdraws them.
    held: Vec<(EffectId, Effect)>,
    store: TestStore,
    /// The cancel flag the scenario handed the run's params.
    cancel: CancelHandle,
    /// The thread that cancels the run on the scenario's signal.
    canceller: Option<JoinHandle<()>>,
}

impl Driving {
    /// Steps the run once, answering every effect the step issued that the
    /// scenario does not hold, and returns whether stepping again can move
    /// the run: `false` once it is done, and while it waits only on held
    /// effects with no woken canceller to wait for.
    pub(crate) fn step(&mut self) -> bool {
        self.step_with(|_| {})
    }

    /// [`step`](Driving::step), handing each effect the step issues to
    /// `issued` in issue order, before it is answered or held.
    pub(crate) fn step_with(&mut self, issued: impl FnMut(&Effect)) -> bool {
        self.step_ordered(issued, |_| {})
    }

    /// [`step_with`](Driving::step_with), answering the effects the step
    /// issues in the order `order` leaves them in, in place of issue order.
    pub(crate) fn step_ordered(
        &mut self,
        mut issued: impl FnMut(&Effect),
        order: impl FnOnce(&mut [(EffectId, Effect)]),
    ) -> bool {
        if self.result.is_some() {
            return false;
        }
        let run = &mut self.run;
        let effects = &mut self.effects;
        let mut moved = false;
        let mut answered = |effect: &Effect, answer: &EffectAnswer| {
            moved = true;
            effects.push((effect.to_record(), answer.to_record()));
        };
        let holding = self.held.len();
        let kept: Vec<EffectId> = self.held.iter().map(|(id, _)| *id).collect();
        let stepped = drive_step(
            run,
            &mut issued,
            &mut |_, _| None,
            &mut answered,
            &mut self.held,
            &mut self.events,
        );
        if let Some(result) = stepped {
            self.result = Some(result);
            return false;
        }
        // The step dropped what it withdrew, so what it left held beyond
        // the earlier holds is the wave this step issued.
        let (held, mut wave): (Vec<_>, Vec<_>) = std::mem::take(&mut self.held)
            .into_iter()
            .partition(|(id, _)| kept.contains(id));
        self.held = held;
        order(&mut wave);
        for (id, effect) in wave {
            match self.answers.answer(&effect) {
                Some(answer) => {
                    moved = true;
                    self.effects.push((effect.to_record(), answer.to_record()));
                    run.resume(id, answer);
                }
                None => self.held.push((id, effect)),
            }
        }
        moved || self.held.len() > holding || self.await_cancel()
    }

    /// Answers the first held effect `which` matches, in issue order, as
    /// the scenario answers an effect it does not hold.
    ///
    /// # Panics
    /// When no held effect matches.
    #[track_caller]
    pub(crate) fn release(&mut self, which: impl Fn(&Effect) -> bool) {
        let Some(at) = self.held.iter().position(|(_, effect)| which(effect)) else {
            panic!("none of the {} held effects matches", self.held.len());
        };
        let (id, effect) = self.held.remove(at);
        let answer = self.answers.perform(&effect);
        self.effects.push((effect.to_record(), answer.to_record()));
        self.run.resume(id, answer);
    }

    /// The parse events, then every event the run reported so far, in
    /// order.
    pub(crate) fn events(&self) -> &[Event] {
        &self.events
    }

    /// The run, done or not.
    pub(crate) fn run_mut(&mut self) -> &mut Run {
        &mut self.run
    }

    /// Joins the canceller once it is woken, so the next step sees its
    /// cancel, and returns whether it did. A run waiting only on held
    /// effects moves again only through that cancel.
    fn await_cancel(&mut self) -> bool {
        if !self.answers.signalled.get() {
            return false;
        }
        let Some(canceller) = self.canceller.take() else {
            return false;
        };
        canceller
            .join()
            .expect("the canceller thread does not panic");
        true
    }

    /// Sets the run's cancel flag between steps, as a caller canceling a
    /// run it steps would. A run that is done ignores it.
    pub(crate) fn cancel(&mut self) {
        if self.result.is_none() {
            self.cancel.cancel();
        }
    }

    /// Steps the run to its end and returns what it produced.
    ///
    /// # Panics
    /// When the run waits only on held effects, which nothing releases
    /// until the run withdraws them.
    #[track_caller]
    pub(crate) fn finish(mut self) -> Trace {
        while self.step() {}
        self.end().0
    }

    /// Ends a run already stepped as far as it goes and returns what it
    /// produced, beside the run itself.
    ///
    /// # Panics
    /// When the run is not done: it waits only on held effects, which
    /// nothing releases until the run withdraws them.
    #[track_caller]
    pub(crate) fn end(self) -> (Trace, Run) {
        let Some(result) = self.result else {
            panic!(
                "the run waits only on {} held effects, and no cancel has withdrawn them",
                self.held.len()
            );
        };
        // Dropping the answers hangs up on a canceller whose signal never
        // came, so the join below returns.
        drop(self.answers);
        if let Some(canceller) = self.canceller {
            canceller
                .join()
                .expect("the canceller thread does not panic");
        }
        let trace = Trace {
            result,
            events: self.events,
            parsed: self.parsed,
            effects: self.effects,
            store: self.store,
            turns: self.run.turns_for_test(),
        };
        (trace, self.run)
    }
}

/// What a scenario's run produced: its result, its events, every effect it
/// issued paired with its answer, its store, and its turn count.
pub(crate) struct Trace {
    result: Result<String, Error>,
    /// The parse events, then every event the run reported, in order.
    events: Vec<Event>,
    /// How many of the events the parse reported.
    parsed: usize,
    effects: Vec<(EffectRecord, AnswerRecord)>,
    store: TestStore,
    turns: u32,
}

impl Trace {
    /// The run's result.
    pub(crate) fn result(&self) -> &Result<String, Error> {
        &self.result
    }

    /// The text of a run that succeeded.
    ///
    /// # Panics
    /// When the run ended with an error, naming it.
    #[track_caller]
    pub(crate) fn text(&self) -> &str {
        match &self.result {
            Ok(text) => text,
            Err(error) => panic!("expected the run to succeed, got {error:?}"),
        }
    }

    /// The error a run ended with: a failure, or a cancel.
    ///
    /// # Panics
    /// When the run succeeded, naming its text.
    #[track_caller]
    pub(crate) fn error(&self) -> &Error {
        match &self.result {
            Err(error) => error,
            Ok(text) => panic!("expected the run to end with an error, got the text {text:?}"),
        }
    }

    /// The parse events, then every event the run reported, in order.
    pub(crate) fn events(&self) -> &[Event] {
        &self.events
    }

    /// Every event the run reported, in order, without the parse events
    /// ahead of them.
    pub(crate) fn run_events(&self) -> &[Event] {
        &self.events[self.parsed..]
    }

    /// Every effect the run issued, beside its answer, in the order the
    /// answers went, which is issue order for every effect the scenario
    /// does not hold. An effect the run withdrew before the scenario
    /// answered it is paired with [`AnswerRecord::Dropped`].
    pub(crate) fn effects(&self) -> &[(EffectRecord, AnswerRecord)] {
        &self.effects
    }

    /// Replays the events onto `observer`, parse events first.
    pub(crate) fn replay(&self, observer: &dyn Observer) {
        forward(self.events.clone(), observer);
    }

    /// The events as the suites' observer sees them, parse events first.
    pub(crate) fn records(&self) -> Vec<Record> {
        records_of(&self.events)
    }

    /// The run's own events as the suites' observer sees them, without the
    /// parse events: what an observer on the run alone would have seen.
    pub(crate) fn run_records(&self) -> Vec<Record> {
        records_of(self.run_events())
    }

    /// The debug pair of every model turn the run reported, request before
    /// response, in order. The run reports the pair only when its params
    /// turn debug reporting on.
    pub(crate) fn captures(&self) -> Vec<Capture> {
        let captures = Captures::default();
        for event in self.events.iter().cloned() {
            forward_one(event, &NullObserver, Some(&captures));
        }
        captures
            .0
            .into_inner()
            .expect("the capture lock is not poisoned")
    }

    /// The run's model-turn counter at its end: one per round that produced
    /// a reply, an empty one included. Zero for a run that never started.
    pub(crate) fn turns(&self) -> u32 {
        self.turns
    }

    /// Reads a store path after the run, through a fresh access.
    pub(crate) fn read(&self, path: &str) -> Result<String, VfsError> {
        self.store.read(path)
    }
}

/// `events` as the suites' observer sees them, in order.
fn records_of(events: &[Event]) -> Vec<Record> {
    let recorder = RecordObserver::default();
    forward(events.to_vec(), &recorder);
    recorder
        .0
        .into_inner()
        .expect("the record lock is not poisoned")
}

/// One observation: which execution and section emitted it, plus the
/// rendered event detail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Record {
    pub(crate) execution: String,
    pub(crate) section: String,
    pub(crate) detail: String,
}

impl Record {
    /// Builds an expected record from borrowed parts, for assertions.
    pub(crate) fn new(execution: &str, section: &str, detail: &str) -> Record {
        Record {
            execution: execution.to_owned(),
            section: section.to_owned(),
            detail: detail.to_owned(),
        }
    }
}

/// The observer [`Trace::records`] replays the events onto.
#[derive(Default)]
struct RecordObserver(Mutex<Vec<Record>>);

impl Observer for RecordObserver {
    fn observe(&self, execution: &str, section: &str, event: Observation) {
        self.0
            .lock()
            .expect("the record lock is not poisoned")
            .push(Record::new(execution, section, &event.to_string()));
    }
}

/// One debug capture: which execution and section the model turn ran in,
/// its 1-based turn, and the body captured.
#[derive(Clone, Debug)]
pub(crate) struct Capture {
    pub(crate) execution: String,
    pub(crate) section: String,
    pub(crate) turn: u32,
    pub(crate) event: DebugEvent,
}

/// The debug capture [`Trace::captures`] replays the events onto.
#[derive(Default)]
struct Captures(Mutex<Vec<Capture>>);

impl DebugCapture for Captures {
    fn on_event(&self, execution: &str, section: &str, turn_index: u32, event: DebugEvent) {
        self.0
            .lock()
            .expect("the capture lock is not poisoned")
            .push(Capture {
                execution: execution.to_owned(),
                section: section.to_owned(),
                turn: turn_index,
                event,
            });
    }
}
