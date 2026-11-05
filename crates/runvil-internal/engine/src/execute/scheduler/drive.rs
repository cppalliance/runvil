//! The run-level step: `resume -> match request -> dispatch -> resume with
//! answer`, run until no chain can proceed without an answer from the
//! caller. The step checks that the run can start and starts its first
//! chain on its first call, drains the ready queue, and returns the
//! effects the drain issued with the events it reported.
//! An empty ready queue with no parked effect and no queued chain that can
//! be admitted is a stall, which fails loudly rather than hangs.
//! The run's `Done` is withheld until
//! every issued effect has its answer, so every effect the caller was
//! handed has exactly one answer.

use runvil_types::error::{ErrorCode, place};
use runvil_types::event::lifecycle;
use runvil_types::ids::AbandonReason;
use runvil_vfs::Origin;

use crate::execute::run::{EffectAnswer, EffectId, Step};
use crate::{Error, Result, error};

use super::chain::ChainStep;
use super::effects::Arrival;
use super::{GENERIC_COMPLETION, Phase, Scheduler};

impl Scheduler {
    /// Drains the ready queue and returns the step's outcome: the effects
    /// issued and the events reported while chains ran, or the run's
    /// result once it is over and every issued effect is answered. The
    /// first call checks that the run can start, then starts the H1 pass
    /// when the prompt has H1 blocks, else the root chain over the prompt's
    /// sections. A run that cannot start ends with that failure, or as
    /// canceled when its cancel flag is already set, and reports no run
    /// events.
    pub(crate) fn step(&mut self) -> Step {
        if matches!(self.phase, Phase::Fresh) {
            if let Err(error) = self.check_start() {
                let error = if self.ctx.cancel().is_canceled() {
                    Error::canceled()
                } else {
                    error
                };
                self.phase = Phase::Ending(Err(error));
            } else {
                self.phase = Phase::Running;
                // The run's boundaries are events like every other report:
                // pushed into the buffer under the root task, so the caller
                // sees them in order with the sections between them.
                self.ctx
                    .emitter()
                    .report(self.ctx.prompt().title(), lifecycle::RUN_STARTED);
                if let Err(error) = self.start() {
                    self.end(Err(error));
                }
            }
        }
        if matches!(self.phase, Phase::Running) {
            self.drain();
        }
        // Every unfinished chain is ready, parked on an effect, blocked on
        // a child, waiting on a task, or queued for admission, and a
        // blocked or waiting chain transitively bottoms out in a ready or
        // parked chain, so an empty ready queue with no parked effect and
        // no admissible queued chain can only be a scheduler bug (nothing
        // ready, nothing parked, no slot that will ever free, and whatever
        // is waiting can never be woken) - fail loudly rather than hang. A
        // running drain returns only after `admit` left the ready queue
        // empty, so no queued chain is admissible here. Those two
        // conditions are the whole guard; the admission queue needs no
        // third, because a chain that can be admitted is a queued chain the
        // drain already offered.
        if matches!(self.phase, Phase::Running)
            && self.ready.is_empty()
            && !self.effects.has_awaited()
        {
            self.end(Err(error::internal(
                "the scheduler stalled with no ready chain, no in-flight request, \
                 and no queued chain that can be admitted",
            )));
        }
        let (effects, withdrawn) = self.effects.take_step();
        let events = self.ctx.take_events();
        let answered = self.effects.all_answered();
        match std::mem::replace(&mut self.phase, Phase::Done) {
            Phase::Ending(result) if answered => {
                // Only here, with every issued effect answered, so an
                // orphan's store effect still runs in the live scope.
                self.end_scope();
                Step::Done {
                    result: finished(result),
                    events,
                }
            }
            Phase::Done => Step::Done {
                result: Err(error::internal("a finished run cannot be stepped again")),
                events,
            },
            phase @ (Phase::Fresh | Phase::Running | Phase::Ending(_)) => {
                self.phase = phase;
                Step::Pending {
                    effects,
                    events,
                    withdrawn,
                }
            }
        }
    }

    /// Applies one effect's answer. An answer for an orphaned effect (its
    /// chain stopped waiting) is discarded; an answer for an id the run
    /// never issued, or a second answer for one effect, ends the run with
    /// an internal error, as does a fatal outcome of the answer itself (a
    /// claims-model conflict). A run that has returned `Done` ignores
    /// every answer.
    pub(crate) fn resume(&mut self, id: EffectId, answer: EffectAnswer) {
        if matches!(self.phase, Phase::Done) {
            return;
        }
        let parked = match self.effects.arrive(id) {
            Arrival::Awaited(parked) => parked,
            Arrival::Orphaned => return,
            Arrival::Unknown => {
                self.end(Err(error::internal(
                    "an answer arrived for an effect the run did not issue or already answered",
                )));
                return;
            }
        };
        // Every answer is applied here, on the caller's thread: the
        // round's events fire against the parked chain's own reporting
        // handles, a chat round's tool calls are checked against its
        // section's frozen sets, a timer's firing wakes its waiter, and a
        // fatal store conflict ends the run.
        if let Err(error) = self.apply_answer(parked, answer) {
            self.end(Err(error));
        }
    }

    /// Starts the run's first chain: the H1 pass when the prompt has H1
    /// blocks; an H1-less prompt goes straight to the walk, so no VM is set
    /// up for a throwaway section 0. A prompt with neither ends at once
    /// with the generic completion.
    fn start(&mut self) -> Result<()> {
        let prompt = self.prompt();
        if crate::prompt::detail::h1_blocks(&prompt).is_empty() {
            if crate::prompt::detail::sections(&prompt).is_empty() {
                self.end(Ok(GENERIC_COMPLETION.to_owned()));
                return Ok(());
            }
            self.start_root_walk()?;
        } else {
            self.start_h1()?;
        }
        Ok(())
    }

    /// Checks that the run can start: the version gate, then the declared
    /// store, then what the prompt requires of the params.
    ///
    /// # Errors
    /// Returns an `UnsupportedVersion` failure on the `runvil:` line, or a
    /// structural parse failure for a prompt with no `runvil:` key, either
    /// naming the prompt; a store failure when the handle declares no store
    /// or the store's backend fails the probe; or `RequirementsUnmet` with
    /// the refusal notice when the params lack a declared Pack or a model
    /// the prompt's roles require, or names a Pack id the Engine reserves,
    /// placed on the frontmatter key of the first gap the prompt caused.
    fn check_start(&self) -> Result<()> {
        let prompt = self.ctx.prompt();
        let frontmatter = prompt.frontmatter();
        let declined = match frontmatter.runvil() {
            Some(0) => None,
            Some(other) => Some(place(
                error::unsupported_version(other),
                prompt.file(),
                crate::prompt::detail::key_line(prompt, "runvil"),
            )),
            None => Some(crate::prompt::detail::parse_error(
                ErrorCode::StructureInvalid,
                "not a runvil prompt: no runvil version",
            )),
        };
        if let Some(error) = declined {
            return Err(crate::prompt::detail::with_prompt_context(
                error,
                frontmatter.name(),
                prompt.file(),
            ));
        }
        // Every run and every store call needs a declared store: the probe
        // stats the store root through a throwaway capability, so a handle
        // with no declaration fails here rather than on the first store call,
        // and a declared-but-failing backend's error fails the run.
        let probe = self
            .ctx
            .vfs()
            .acquire(Origin::new("store mount probe"))
            .map_err(|cause| error::store(&cause))?;
        runvil_vfs::detail::probe_store(&probe).map_err(|cause| error::store(&cause))?;
        if let Some(refusal) = self.ctx.refusal() {
            return Err(refusal.clone());
        }
        Ok(())
    }

    /// Runs every ready chain to its next suspension point. Cancellation
    /// is polled before each chain step: the instruction hook covers
    /// running Lua, and a cancel that arrives while every chain is
    /// suspended is observed on the next `step`. When the queue empties,
    /// the queued tasks that now fit are admitted and drained in turn:
    /// admission is the one place a queued chain's Lua VM comes to be, and
    /// deferring it to the drain's edge lets a woken owner cancel its
    /// queued siblings before they start.
    fn drain(&mut self) {
        loop {
            if self.ctx.cancel().is_canceled() {
                self.end(Err(Error::canceled()));
                return;
            }
            let Some(id) = self.ready.pop_front() else {
                if let Err(error) = self.admit() {
                    self.end(Err(error));
                    return;
                }
                if self.ready.is_empty() {
                    return;
                }
                continue;
            };
            let step = match self.step_chain(id) {
                Ok(step) => step,
                Err(error) => self.finish(id, Err(error)),
            };
            if let ChainStep::RunEnded(result) = step {
                self.end(result);
                return;
            }
        }
    }

    /// Decides the run: settles every live task exactly once (each
    /// reports `TaskAbandoned` - with `RunTerminated` for a task the run's
    /// end stranded directly, `OwnerAborted` for one nested under it and
    /// ended through `abort_subtree` - so a task stranded by a run cancel
    /// or a fatal answer keeps the one-terminal contract; a run that ended
    /// well has none left, its root chain having settled its own), tears
    /// every chain down (the suspended chains' frames
    /// drop unarmed - no `SECTION_FINISHED` - and every effect still out
    /// with the caller becomes an orphan the caller still answers, listed
    /// as withdrawn), reports the run's end boundary after every task
    /// terminal, and holds `result` until the orphans are answered. A
    /// second decision keeps the first: the result that ended the run is
    /// the record. A run that has not started ends like one that could not
    /// start: it holds `result` and reports nothing.
    fn end(&mut self, result: Result<String>) {
        match self.phase {
            Phase::Ending(_) | Phase::Done => return,
            Phase::Fresh => {
                self.phase = Phase::Ending(result);
                return;
            }
            Phase::Running => {}
        }
        self.settle_all_tasks(AbandonReason::RunTerminated);
        self.teardown();
        self.ctx.emitter().report(
            self.ctx.prompt().title(),
            if result.is_ok() {
                lifecycle::RUN_SUCCEEDED
            } else {
                lifecycle::RUN_FAILED
            },
        );
        self.phase = Phase::Ending(result);
    }

    /// Drops every chain's live state in the teardown order (the suspended
    /// coroutine, then the frame unarmed, then the access capability),
    /// orphans every awaited effect, and withdraws every outstanding
    /// effect. The task entries keep their terminal state for inspection;
    /// every entry is terminal by now, `end` having
    /// settled the live ones, so nothing here reports. Each chain joins
    /// the tasks it owns before its access drops, closing the run's
    /// happens-before record.
    fn teardown(&mut self) {
        self.ready.clear();
        self.admission.clear();
        for (owner, chain) in self.chains.iter() {
            let Some(access) = chain.access.as_ref() else {
                continue;
            };
            self.join_owned_tasks(owner, access);
        }
        for chain in self.chains.iter_mut() {
            chain.parked = None;
            chain.entered = None;
            chain.access = None;
        }
        self.effects.orphan_all();
    }
}

/// The result `Done` reports for a run that ended with `result`: `result`
/// as it is, except that an error with a caller code, one in the 8xx
/// family, becomes an `Internal` error with the same message, location,
/// and facts, and with the `Internal` prose. A caller code names a failure
/// of the caller's own machinery, never of the run, yet a run can end with
/// one: a raised error table's `code` field names any valid tag, and a
/// model error from the caller keeps its code.
fn finished(result: Result<String>) -> Result<String> {
    result.map_err(|error| {
        if error.code.number() / 100 != 8 {
            return error;
        }
        Error {
            code: ErrorCode::Internal,
            prose: ErrorCode::Internal.prose().to_owned(),
            ..error
        }
    })
}
