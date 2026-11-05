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
//! it, and an uncaught one ends the run cancelled. The loop also looks for
//! a stop before it starts a step's effects, and lowers each stop it sees
//! once it has dropped what the stop reached, none included, so a stop
//! never reaches an effect started after the loop saw it. A drop made
//! there resumes the run as an inline answer does, so once the step's
//! effects start the loop steps again instead of awaiting an answer. A Vfs
//! effect is never in flight at a cancel or a stop: it is answered before
//! the loop waits again. A drop is an answer, recorded like any other, so
//! every effect record has exactly one answer record. Tool calls beyond the
//! performer's cap wait, oldest first, and a cancel, a stop, or a
//! withdrawal reaches them.
//!
//! A performer that panics is caught where its future is polled: its
//! effect is answered `Dropped` and the panic is logged, so a lost
//! performer can never leave the run waiting forever. A Vfs operation that
//! panics is caught where it runs and answered `Dropped` the same way.
//!
//! The run's `Done` ends the run with its outcome at the recorder. A run
//! that is pending with nothing issued and nothing in flight ends there as
//! an internal failure: the run reports a stall itself, so reaching this
//! means the loop lost an effect.

use std::future::{Future, poll_fn};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;

use runvil::cancel::CancelHandle;
use runvil::effect::{Effect, EffectAnswer, EffectId, IssuedEffect};
use runvil::error::Error;
use runvil::event::Event;
use runvil::ids::Provenance;
use runvil::vfs::{Access, VfsOp, perform_vfs_op};
use runvil::{Run, Step};

use crate::harness::StopSignal;
use crate::performers::{BoxFuture, Performers};
use crate::recorder::{
    Record, RecordKind, RunErrorKind, RunId, RunOutcome, RunRecorder, recorder_error,
};

#[path = "effect_loop-flight.rs"]
mod flight;

use flight::{Flights, Reach};

/// The message of the failure a stalled run ends with.
const STALLED: &str = "the effect loop has nothing to await for a pending run";

/// Drives `run` to its end inside the returned future: performs its
/// effects through `performers`, hands every event, effect, and answer to
/// `recorder` under `run_id`, and drops every effect in flight when
/// `cancel` fires. `cancel` must be the flag the run's params were built
/// with, so the run sees the same cancel. Ends the run at the recorder with
/// its one outcome and returns it.
///
/// A run that is pending with nothing to await ends as failed with the
/// kind `Internal`, once every future the loop still holds is torn down.
///
/// The future is boxed internally: the step machinery is large, and the
/// caller's own future stays small. It is `Send`, and it needs no runtime
/// of its own: every performer future is polled inside it.
///
/// # Errors
/// Returns the recorder's error when it refuses a write. That is the only
/// error: every effect still in flight is torn down with the future, the
/// run is abandoned mid-flight, and the recorder is not told it ended.
pub async fn drive_run(
    run: Run,
    performers: Performers,
    recorder: Arc<dyn RunRecorder>,
    run_id: RunId,
    cancel: CancelHandle,
) -> Result<RunOutcome, Error> {
    drive(run, performers, recorder, run_id, cancel, Arc::default()).await
}

/// Drives `run` as [`drive_run`] does, and also drops the effects in
/// flight, tool calls that survive stops excepted, each time `stop` is raised.
pub(crate) async fn drive(
    run: Run,
    performers: Performers,
    recorder: Arc<dyn RunRecorder>,
    run_id: RunId,
    cancel: CancelHandle,
    stop: Arc<StopSignal>,
) -> Result<RunOutcome, Error> {
    let cap = performers.tool.max_in_flight();
    let mut driver = Driver {
        run,
        performers,
        recorder,
        run_id,
        flights: Flights::new(cap),
        cancel,
        stop,
    };
    Box::pin(driver.drive()).await
}

/// One run being driven.
struct Driver {
    run: Run,
    performers: Performers,
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
    async fn drive(&mut self) -> Result<RunOutcome, Error> {
        loop {
            match self.run.step() {
                Step::Done { outcome, events } => {
                    self.commit_events(events).await?;
                    // `Done` is returned only once every effect is
                    // answered, so nothing is in flight.
                    return self.end(outcome).await;
                }
                Step::Pending {
                    effects,
                    events,
                    withdrawn,
                } => {
                    // The run reads this same flag, so its next step tears
                    // every chain down.
                    let cancelled = self.cancel.is_cancelled();
                    self.commit_events(events).await?;
                    if cancelled {
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
                            .end(RunOutcome::Failed {
                                kind: RunErrorKind::Internal,
                                message: STALLED.to_owned(),
                            })
                            .await;
                    }
                    self.await_answer().await?;
                }
            }
        }
    }

    /// Ends the run at the recorder with `outcome` and returns it.
    async fn end(&mut self, outcome: RunOutcome) -> Result<RunOutcome, Error> {
        self.recorder.end_run(self.run_id, outcome.clone()).await?;
        Ok(outcome)
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
        let mut cancelled = self.cancel.cancelled();
        let stop = &self.stop;
        let flights = &mut self.flights;
        poll_fn(|cx| {
            if Pin::new(&mut cancelled).poll(cx).is_ready() {
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
                let broker = Arc::clone(&self.performers.broker);
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
                let performer = Arc::clone(&self.performers.tool);
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
                let performer = Arc::clone(&self.performers.tool);
                let call = async move {
                    EffectAnswer::ToolCall(performer.call(tool, alias, access, origin, args).await)
                };
                self.flights
                    .queue(id, provenance.clone(), survives_stop, Box::pin(call));
                return None;
            }
            Effect::Vfs { access, op } => return Some((access, op)),
            Effect::Timer { duration } => {
                let timer = Arc::clone(&self.performers.timer);
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
            let record = record(
                event.provenance(),
                RecordKind::Event,
                None,
                serde_json::to_value(&event).map_err(recorder_error)?,
            );
            self.append(record).await?;
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
        self.recorder.append(self.run_id, record).await
    }
}

/// Answers one Vfs effect inline: performs `op` through the store view
/// the effect carries and returns the answer to record and resume the run
/// with. A panic out of the backend is caught here, logged against `id`
/// and `provenance`, and answered `Dropped`, so it ends the run as a
/// cancelled one and never unwinds the loop.
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
