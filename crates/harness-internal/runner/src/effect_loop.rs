//! The effect loop: the Harness steps an Engine `Run` and performs its effects.
//!
//! The loop is `step -> record -> perform -> await an answer -> record ->
//! resume`. Every step's events are handed to the run's recorder before any
//! of the step's effects is issued, so an effect's record never precedes the
//! events its step reported. Each effect is appended as its
//! [`record`](runvil::effect::Effect::record) and then handled by
//! kind. A chat, render, tool-call, or timer effect's performer future
//! joins the effects in flight, which the loop polls inside the run's own
//! future: the loop starts no task, so a run is one future that any
//! executor can drive. When an effect's future lands, the loop appends the
//! answer's record and resumes the run with it, then steps again. A Vfs
//! effect has no future: the VFS is synchronous by design, so the loop
//! performs the operation inline, appends the answer's record, and resumes
//! the run at once. It resumes every Vfs effect of a step this way and then
//! steps once, so the loop awaits an answer only when it answered nothing
//! inline. Real-disk operations therefore block whatever polls the run, and
//! Vfs operations from different chains run one at a time in issue order.
//!
//! An effect the step withdraws is answered `Dropped` without being
//! performed, and a withdrawn effect an earlier step issued is aborted
//! and answered `Dropped`, so the step that decides the run leaves the
//! loop nothing to perform.
//!
//! The loop waits on three things at once: the run's cancel flag, the stop
//! a Host raises through [`RunControl::stop_round`](crate::RunControl::stop_round),
//! and the next effect to land. A cancel aborts every effect in flight,
//! answers each `Dropped`, and steps the run, which reads the same flag,
//! to `Done`. A stop aborts every effect in flight except tool calls that
//! survive stops and answers each `Dropped`, leaving the cancel flag
//! clear, so the run decides what a dropped call means: a `pcall` catches
//! it, and an uncaught one ends the run canceled. The loop also looks for
//! a stop before it starts a step's effects, and lowers each stop it sees
//! once it has dropped what the stop reached, none included, so a stop
//! never reaches an effect started after the loop saw it. A drop made
//! there resumes the run as an inline answer does, so once the step's
//! effects start the loop steps again instead of awaiting an answer. A Vfs
//! effect is never in flight at a cancel or a stop: it is answered before
//! the loop waits again. A drop is an answer, recorded like any other, so
//! every effect record has exactly one answer record. Tool calls beyond the
//! setup's cap wait unstarted, oldest first, so none starves, and a
//! cancel, a stop, or a withdrawal reaches them by the same reach as the
//! effects in flight. Chat, render, and timer effects never wait and never
//! count toward the cap.
//!
//! A performer that panics is caught where its future is polled: its
//! effect is answered `Dropped` and the panic is logged, so a lost
//! performer can never leave the run waiting forever. A Vfs operation that
//! panics is caught where it runs and answered `Dropped` the same way.
//!
//! The run's `Done` ends the run at the recorder with its result. A run
//! that is pending with nothing issued and nothing in flight ends there
//! with an `Internal` error: the run reports a stall itself, so reaching
//! this means the loop lost an effect.

use std::any::Any;
use std::collections::{BTreeMap, HashMap};
use std::future::{Future, poll_fn};
use std::num::NonZeroUsize;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use futures_util::future::{AbortHandle, Abortable, Aborted, FutureExt as _};
use futures_util::stream::{FuturesUnordered, StreamExt as _};
use runvil::cancel::CancelHandle;
use runvil::effect::{Effect, EffectAnswer, EffectId, IssuedEffect};
use runvil::event::Event;
use runvil::ids::Provenance;
use runvil::vfs::{Access, VfsOp, perform_vfs_op};
use runvil::{Error, ErrorCode};
use runvil::{Run, Step};
use tracing::Instrument as _;

use crate::harness::StopSignal;
use crate::performers::{BoxFuture, InferenceBroker, Timer, ToolPerformer};
use crate::recorder::{Record, RecordKind, RunId, RunRecorder, make_recorder_error, refused};
use crate::setup::RunSetup;

/// The message of the failure a stalled run ends with.
const STALLED: &str = "the effect loop has nothing to await for a pending run";

/// Drives `run` to its end: chat effects go to
/// `setup.broker`, renders and tool calls to `tool`
/// with at most `setup.max_tool_calls_in_flight`
/// calls in flight, and timers to `timer`. Every
/// event, effect, and answer is recorded at
/// `setup.recorder` under `run_id`. `cancel` must be
/// the flag `run` was built with.
///
/// Every effect in flight is dropped when `cancel` fires, and the run
/// sees the same cancel. Ends the run at the recorder with its result,
/// the final text or the error that ended it, and returns it.
///
/// A run that is pending with nothing to await ends with an `Internal`
/// error, once every future the loop still holds is torn down.
///
/// The future is boxed internally: the step machinery is large, and the
/// caller's own future stays small. It is `Send`, and it needs no runtime
/// of its own: every performer future is polled inside it.
///
/// # Errors
/// Returns the error that ended the run, or a `RecorderFailed` refusal
/// with the run's `Run` fact when the recorder refuses a write, a code no
/// result of the run has. A refusal ends nothing: every effect still in
/// flight is torn down with the future, the run is abandoned mid-flight,
/// and the recorder is not told it ended.
pub async fn drive_run(
    run: Run,
    setup: &RunSetup,
    timer: Arc<dyn Timer>,
    tool: Arc<dyn ToolPerformer>,
    run_id: RunId,
    cancel: CancelHandle,
) -> Result<String, Error> {
    drive(run, setup, timer, tool, run_id, cancel, Arc::default()).await
}

/// As `drive_run`, and drops the effects in flight
/// but stop survivors each time `stop` is raised.
pub(crate) async fn drive(
    run: Run,
    setup: &RunSetup,
    timer: Arc<dyn Timer>,
    tool: Arc<dyn ToolPerformer>,
    run_id: RunId,
    cancel: CancelHandle,
    stop: Arc<StopSignal>,
) -> Result<String, Error> {
    let mut driver = Driver {
        run,
        broker: Arc::clone(&setup.broker),
        tool,
        timer,
        recorder: Arc::clone(&setup.recorder),
        run_id,
        flights: Flights::new(setup.max_tool_calls_in_flight),
        cancel,
        stop,
    };
    Box::pin(driver.drive()).await
}

/// One run being driven.
struct Driver {
    run: Run,
    /// Performs `Chat` effects.
    broker: Arc<dyn InferenceBroker>,
    /// Performs `ToolCall` and `Render` effects.
    tool: Arc<dyn ToolPerformer>,
    /// Performs `Timer` effects.
    timer: Arc<dyn Timer>,
    recorder: Arc<dyn RunRecorder>,
    run_id: RunId,
    /// The chat, render, tool-call, and timer effects in flight and the
    /// tool calls waiting for a slot. Dropping the driver drops their futures, so
    /// nothing a performer holds outlives the drive however it ends.
    flights: Flights,
    cancel: CancelHandle,
    stop: Arc<StopSignal>,
}

/// What ended the loop's wait.
enum Woken {
    /// The run's cancel flag fired.
    Cancel,
    /// The Host raised a stop.
    Stop,
    /// An effect's future landed with its answer.
    Landed(EffectId, Provenance, EffectAnswer),
}

impl Driver {
    async fn drive(&mut self) -> Result<String, Error> {
        loop {
            match self.run.step() {
                Step::Done { result, events } => {
                    self.commit_events(events).await?;
                    // `Done` is returned only once every effect is
                    // answered, so nothing is in flight.
                    return self.end(result).await;
                }
                Step::Pending {
                    effects,
                    events,
                    withdrawn,
                } => {
                    // The run reads this same flag, so its next step tears
                    // every chain down.
                    let canceled = self.cancel.is_canceled();
                    self.commit_events(events).await?;
                    if canceled {
                        // Every effect issued in this step is moot before
                        // it is performed, and every effect in flight is
                        // moot too. Record each effect and answer it
                        // `Dropped` so the next step reaches `Done`.
                        for issued in effects {
                            self.commit_effect(&issued).await?;
                            self.drop_effect(issued.id, &issued.provenance).await?;
                        }
                        self.drop_in_flight(Reach::All).await?;
                        continue;
                    }
                    let mut answered = false;
                    if self.stop.is_raised() {
                        answered = self.act_on_stop().await?;
                    }
                    for issued in effects {
                        self.commit_effect(&issued).await?;
                        let (id, provenance) = (issued.id, &issued.provenance);
                        // The run's own word, not a scan of its events: the
                        // events are a report, not a control channel. A
                        // withdrawn effect is moot before it is performed,
                        // a Vfs effect included.
                        if withdrawn.binary_search(&id).is_ok() {
                            self.drop_effect(id, provenance).await?;
                            answered = true;
                            continue;
                        }
                        // A Vfs effect comes back from `perform` and is
                        // answered here; every other kind goes in flight.
                        // The run is not stepped between inline answers:
                        // the whole batch is resumed, then stepped once.
                        if let Some((access, op)) = self.perform(id, provenance, issued.effect) {
                            let answer = answer_vfs(id, provenance, access, op);
                            self.answer(id, provenance, answer).await?;
                            answered = true;
                        }
                    }
                    if !withdrawn.is_empty() {
                        self.drop_in_flight(Reach::Withdrawn(&withdrawn)).await?;
                        answered = true;
                    }
                    if answered {
                        // Stepping again, not awaiting: the answers just
                        // resumed, a stop's drops as well as the inline
                        // ones, may have made chains ready, and a step
                        // whose every effect was a Vfs effect has nothing
                        // in flight to await.
                        continue;
                    }
                    if self.flights.is_empty() {
                        // Nothing is owed an answer, but the set may still
                        // hold aborted futures it has not polled.
                        self.flights.clear();
                        return self
                            .end(Err(Error::new(
                                ErrorCode::Internal,
                                STALLED,
                                ErrorCode::Internal.prose(),
                            )))
                            .await;
                    }
                    self.await_answer().await?;
                }
            }
        }
    }

    /// Ends the run at the recorder with `result` and returns it.
    async fn end(&mut self, result: Result<String, Error>) -> Result<String, Error> {
        self.recorder
            .end_run(self.run_id, result.clone())
            .await
            .map_err(|error| refused(self.run_id, error))?;
        result
    }

    /// Waits for the next answer, then answers every effect already
    /// landed behind it, or acts on a cancel or a stop as soon as one is
    /// raised.
    async fn await_answer(&mut self) -> Result<(), Error> {
        match self.wake().await {
            Woken::Cancel => self.drop_in_flight(Reach::All).await,
            Woken::Stop => self.act_on_stop().await.map(drop),
            Woken::Landed(id, provenance, answer) => {
                self.answer(id, &provenance, answer).await?;
                self.answer_landed().await
            }
        }
    }

    /// Acts on a raised stop: drops whatever is in flight but the tool
    /// calls that survive stops, which may be nothing, then lowers the
    /// stop. Returns whether it answered any effect, since each answer
    /// resumes a chain the run must step.
    async fn act_on_stop(&mut self) -> Result<bool, Error> {
        let owed = self.flights.len();
        self.drop_in_flight(Reach::AllButSurvivors).await?;
        self.stop.lower();
        Ok(self.flights.len() < owed)
    }

    /// Waits for the cancel flag, a raised stop, or the next effect to
    /// land, checked in that order, so a stop drops a round whose answer
    /// arrived beside it. Every arm is event-driven: the cancel and the
    /// stop wake the loop as they are raised and a future wakes it as it
    /// lands, so a fully suspended run costs no wakeups while it waits.
    async fn wake(&mut self) -> Woken {
        let mut canceled = self.cancel.canceled();
        let stop = &self.stop;
        let flights = &mut self.flights;
        poll_fn(|cx| {
            if Pin::new(&mut canceled).poll(cx).is_ready() {
                return Poll::Ready(Woken::Cancel);
            }
            if stop.poll_raised(cx).is_ready() {
                return Poll::Ready(Woken::Stop);
            }
            flights
                .poll_landed(cx)
                .map(|(id, provenance, answer)| Woken::Landed(id, provenance, answer))
        })
        .await
    }

    /// Answers every effect that has already landed, without waiting.
    async fn answer_landed(&mut self) -> Result<(), Error> {
        while let Some((id, provenance, answer)) = self.flights.landed_now() {
            self.answer(id, &provenance, answer).await?;
        }
        Ok(())
    }

    /// Answers `Dropped`, in effect order, each effect waiting or in
    /// flight that `reach` takes. The aborted futures are then torn down,
    /// so whatever a dropped performer held is gone before the run steps
    /// on; an effect `reach` spared that landed meanwhile is answered too.
    async fn drop_in_flight(&mut self, reach: Reach<'_>) -> Result<(), Error> {
        for (id, provenance) in self.flights.abort(reach) {
            self.drop_effect(id, &provenance).await?;
        }
        self.answer_landed().await
    }

    /// Records the `Dropped` answer for one effect and resumes the run
    /// with it.
    async fn drop_effect(&mut self, id: EffectId, provenance: &Provenance) -> Result<(), Error> {
        self.answer(id, provenance, EffectAnswer::Dropped).await
    }

    /// Records one answer and resumes the run with it.
    async fn answer(
        &mut self,
        id: EffectId,
        provenance: &Provenance,
        answer: EffectAnswer,
    ) -> Result<(), Error> {
        self.commit_answer(id, provenance, &answer).await?;
        self.run.resume(id, answer);
        Ok(())
    }

    /// Puts the future of a chat, render, tool-call, or timer effect in
    /// flight, where a tool call may wait for a slot, and returns `None`.
    /// The performer is called inside its future, so a performer that
    /// panics as it is called is caught like one that panics as it runs.
    ///
    /// A Vfs effect goes nowhere and is handed back as the access and
    /// operation it carries, for the caller to answer inline.
    fn perform(
        &mut self,
        id: EffectId,
        provenance: &Provenance,
        effect: Effect,
    ) -> Option<(Arc<Access>, VfsOp)> {
        let answer: BoxFuture<EffectAnswer> = match effect {
            Effect::Chat {
                binding,
                messages,
                tools,
                round,
                ..
            } => {
                let broker = Arc::clone(&self.broker);
                let options = binding.completion_options();
                let round = async move {
                    let completion = broker.chat(binding, messages, tools, options, round).await;
                    EffectAnswer::Chat(completion)
                };
                Box::pin(round)
            }
            Effect::Render {
                entry,
                binding,
                offered,
            } => {
                let performer = Arc::clone(&self.tool);
                let model = binding.render_info();
                Box::pin(async move {
                    EffectAnswer::Render(performer.render(entry, model, offered).await)
                })
            }
            Effect::ToolCall {
                tool,
                alias,
                args,
                origin,
                access,
                survives_stop,
            } => {
                let performer = Arc::clone(&self.tool);
                let call = async move {
                    EffectAnswer::ToolCall(performer.call(tool, alias, access, origin, args).await)
                };
                self.flights
                    .queue(id, provenance.clone(), survives_stop, Box::pin(call));
                return None;
            }
            Effect::Vfs { access, op } => return Some((access, op)),
            Effect::Timer { duration } => {
                let timer = Arc::clone(&self.timer);
                let sleep = async move {
                    timer.sleep(duration).await;
                    EffectAnswer::Timer
                };
                Box::pin(sleep)
            }
        };
        self.flights.start(id, provenance.clone(), answer);
        None
    }

    /// Appends one step's events to the recorder.
    async fn commit_events(&mut self, events: Vec<Event>) -> Result<(), Error> {
        for event in events {
            let payload = serde_json::to_value(&event)
                .map_err(|error| refused(self.run_id, make_recorder_error(error)))?;
            self.append(record(event.provenance(), RecordKind::Event, None, payload))
                .await?;
        }
        Ok(())
    }

    /// Appends one issued effect's record.
    async fn commit_effect(&mut self, issued: &IssuedEffect) -> Result<(), Error> {
        self.append(record(
            &issued.provenance,
            RecordKind::Effect,
            Some(issued.id.get()),
            issued.effect.record(),
        ))
        .await
    }

    /// Appends one answer's record under its effect's provenance.
    async fn commit_answer(
        &mut self,
        id: EffectId,
        provenance: &Provenance,
        answer: &EffectAnswer,
    ) -> Result<(), Error> {
        self.append(record(
            provenance,
            RecordKind::Answer,
            Some(id.get()),
            answer.record(),
        ))
        .await
    }

    async fn append(&mut self, record: Record) -> Result<(), Error> {
        self.recorder
            .append(self.run_id, record)
            .await
            .map_err(|error| refused(self.run_id, error))
    }
}

/// How one future in flight ended: its answer or its panic's payload, or
/// the abort the loop applied.
type Landing = (EffectId, Result<std::thread::Result<EffectAnswer>, Aborted>);

/// Which held effects an abort reaches.
#[derive(Clone, Copy, Debug)]
enum Reach<'a> {
    /// Every effect, waiting or in flight: a cancel.
    All,
    /// Every effect but the tool calls that survive stops: a stop.
    AllButSurvivors,
    /// The listed effects, waiting or in flight: the tool calls a step
    /// withdrew, in effect order.
    Withdrawn(&'a [EffectId]),
}

impl Reach<'_> {
    /// Whether this reach takes effect `id`, which survives stops when
    /// `survives_stop` is set.
    fn takes(self, id: EffectId, survives_stop: bool) -> bool {
        match self {
            Reach::All => true,
            Reach::AllButSurvivors => !survives_stop,
            Reach::Withdrawn(ids) => ids.binary_search(&id).is_ok(),
        }
    }
}

/// One effect in flight: what the loop needs to answer or drop it.
struct InFlight {
    /// The provenance the effect was issued under, for its answer record.
    provenance: Provenance,
    /// Aborts the effect's future.
    abort: AbortHandle,
    /// Whether the effect is a tool call, which counts toward the cap.
    tool_call: bool,
    /// Whether the effect is a tool call that survives stops, which a
    /// stop leaves alone.
    survives_stop: bool,
}

/// A tool call waiting for a slot: its performer future, not yet polled.
struct Waiting {
    provenance: Provenance,
    survives_stop: bool,
    answer: BoxFuture<EffectAnswer>,
}

/// The effects in flight and their futures, and the tool calls waiting
/// for a slot.
///
/// Every future in flight sits in one `FuturesUnordered` as an `Abortable`
/// future under `catch_unwind`. An aborted future leaves the keyed set at
/// once, since the loop answers its effect as it aborts it, and is torn
/// down the next time the set is polled; whatever it yields then is
/// discarded, so the run never sees two answers for one effect.
struct Flights {
    futures: FuturesUnordered<BoxFuture<Landing>>,
    /// The effects in flight still owed an answer. A landing for an id not
    /// here is an aborted future's late end and is discarded.
    effects: HashMap<EffectId, InFlight>,
    /// Tool calls waiting for a slot; the lowest id starts first.
    waiting: BTreeMap<EffectId, Waiting>,
    /// The most tool calls in flight at once.
    cap: NonZeroUsize,
}

impl Flights {
    fn new(cap: NonZeroUsize) -> Self {
        Self {
            futures: FuturesUnordered::new(),
            effects: HashMap::new(),
            waiting: BTreeMap::new(),
            cap,
        }
    }

    /// Whether no effect is owed an answer.
    fn is_empty(&self) -> bool {
        self.effects.is_empty() && self.waiting.is_empty()
    }

    /// How many effects are owed an answer, waiting calls included.
    fn len(&self) -> usize {
        self.effects.len() + self.waiting.len()
    }

    /// Tears down every future the set holds, aborted ones not yet polled
    /// included, and every waiting call, with no answer for any of them.
    fn clear(&mut self) {
        self.futures.clear();
        self.effects.clear();
        self.waiting.clear();
    }

    /// Puts `answer`, the future of chat, render, or timer effect `id`, in
    /// flight under `provenance`. It never waits and never counts toward
    /// the cap.
    fn start(&mut self, id: EffectId, provenance: Provenance, answer: BoxFuture<EffectAnswer>) {
        self.launch(id, provenance, false, false, answer);
    }

    /// Queues `answer`, the performer future of tool call `id`, under
    /// `provenance`, then starts waiting calls, lowest id first, while
    /// fewer than `cap` tool calls are in flight. `survives_stop` marks a
    /// call that survives stops.
    fn queue(
        &mut self,
        id: EffectId,
        provenance: Provenance,
        survives_stop: bool,
        answer: BoxFuture<EffectAnswer>,
    ) {
        self.waiting.insert(
            id,
            Waiting {
                provenance,
                survives_stop,
                answer,
            },
        );
        self.fill();
    }

    /// Starts waiting calls, lowest id first, while fewer than `cap` tool
    /// calls are in flight.
    fn fill(&mut self) {
        let mut running = self
            .effects
            .values()
            .filter(|in_flight| in_flight.tool_call)
            .count();
        while running < self.cap.get() {
            let Some((id, call)) = self.waiting.pop_first() else {
                return;
            };
            self.launch(id, call.provenance, true, call.survives_stop, call.answer);
            running += 1;
        }
    }

    /// Puts `answer`, the performer future of effect `id`, in flight under
    /// `provenance`, inside a span that records the effect id, the task
    /// path, and the task-local sequence, so a run's effects trace as a
    /// group. `tool_call` marks a tool call, and `survives_stop` one that
    /// survives stops.
    fn launch(
        &mut self,
        id: EffectId,
        provenance: Provenance,
        tool_call: bool,
        survives_stop: bool,
        answer: BoxFuture<EffectAnswer>,
    ) {
        let span = tracing::info_span!(
            "effect",
            effect = %id,
            task = %provenance.task,
            seq = provenance.seq
        );
        let (abort, registration) = AbortHandle::new_pair();
        let caught = AssertUnwindSafe(answer.instrument(span)).catch_unwind();
        let landing = Abortable::new(caught, registration).map(move |landed| (id, landed));
        self.futures.push(Box::pin(landing));
        self.effects.insert(
            id,
            InFlight {
                provenance,
                abort,
                tool_call,
                survives_stop,
            },
        );
    }

    /// Polls for the next effect whose future landed, with its provenance
    /// and answer. A future that panicked lands as `Dropped`, and the
    /// panic is logged against its effect. A tool call that lands frees
    /// its slot for the next waiting call.
    fn poll_landed(&mut self, cx: &mut Context<'_>) -> Poll<(EffectId, Provenance, EffectAnswer)> {
        loop {
            let Poll::Ready(Some((id, landed))) = self.futures.poll_next_unpin(cx) else {
                return Poll::Pending;
            };
            let Some(in_flight) = self.effects.remove(&id) else {
                continue;
            };
            if in_flight.tool_call {
                self.fill();
            }
            let answer = match landed {
                Ok(Ok(answer)) => answer,
                Ok(Err(panic)) => {
                    tracing::error!(
                        effect = %id,
                        task = %in_flight.provenance.task,
                        panic = panic_message(&*panic),
                        "a performer panicked; its effect is dropped"
                    );
                    EffectAnswer::Dropped
                }
                // Only an aborted effect's future yields `Aborted`, and an
                // aborted effect left `effects` as it was aborted.
                Err(Aborted) => EffectAnswer::Dropped,
            };
            return Poll::Ready((id, in_flight.provenance, answer));
        }
    }

    /// The next effect that has already landed, without waiting. Polling
    /// also tears down every aborted future the set still holds.
    fn landed_now(&mut self) -> Option<(EffectId, Provenance, EffectAnswer)> {
        match self.poll_landed(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(landed) => Some(landed),
            Poll::Pending => None,
        }
    }

    /// Takes every effect waiting or in flight that `reach` takes:
    /// drops each waiting call's future unstarted, aborts each effect in
    /// flight, and returns each one's id and provenance in effect order,
    /// for the loop to answer `Dropped`. Then starts waiting calls in the
    /// slots it freed, so it never starts a call the same reach takes.
    fn abort(&mut self, reach: Reach<'_>) -> Vec<(EffectId, Provenance)> {
        let in_flight = self
            .effects
            .iter()
            .filter(|(id, in_flight)| reach.takes(**id, in_flight.survives_stop))
            .map(|(id, _)| *id);
        let waiting = self
            .waiting
            .iter()
            .filter(|(id, call)| reach.takes(**id, call.survives_stop))
            .map(|(id, _)| *id);
        let mut chosen: Vec<EffectId> = in_flight.chain(waiting).collect();
        chosen.sort_unstable();
        let mut aborted = Vec::with_capacity(chosen.len());
        for id in chosen {
            if let Some(in_flight) = self.effects.remove(&id) {
                in_flight.abort.abort();
                aborted.push((id, in_flight.provenance));
            } else if let Some(call) = self.waiting.remove(&id) {
                aborted.push((id, call.provenance));
            }
        }
        self.fill();
        aborted
    }
}

/// Answers one Vfs effect inline: performs `op` through the store view
/// the effect carries and returns the answer to record and resume the run
/// with. A panic out of the backend is caught here, logged against `id`
/// and `provenance`, and answered `Dropped`, so it ends the run as a
/// canceled one and never unwinds the loop.
///
/// The access is this function's own parameter, so it drops when the
/// function returns. The drop is hygiene only: claims follow
/// happens-before within the run's scope, and the run ends that scope at
/// `Done` however long any access is held.
fn answer_vfs(
    id: EffectId,
    provenance: &Provenance,
    access: Arc<Access>,
    op: VfsOp,
) -> EffectAnswer {
    let result = catch_unwind(AssertUnwindSafe(|| perform_vfs_op(&access, op)));
    drop(access);
    match result {
        Ok(outcome) => EffectAnswer::Vfs(outcome),
        Err(_panic) => {
            tracing::error!(
                effect = %id,
                task = %provenance.task,
                "a Vfs operation panicked; its effect is dropped"
            );
            EffectAnswer::Dropped
        }
    }
}

/// A panic payload's message, when it carries one.
fn panic_message(payload: &(dyn Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("a panic with no message")
}

/// One record under `provenance`.
fn record(
    provenance: &Provenance,
    kind: RecordKind,
    effect_id: Option<u64>,
    payload: serde_json::Value,
) -> Record {
    Record {
        task_id: provenance.task.to_string(),
        task_seq: provenance.seq,
        kind,
        effect_id,
        payload,
    }
}
