//! The run: the Engine's boundary with its caller, which `Run`'s methods
//! cross by exchanging effects and events as values. The caller's loop is
//! documented on the `runvil-engine` facade's crate page and its `effect` and
//! `cancel` modules.
//!
//! The effect vocabulary - [`Effect`] with its [`Round`] and
//! [`PackToolSets`], [`EffectAnswer`], [`EffectId`], and the
//! [`ToolCallOrigin`] and [`ToolCaller`] it re-exports from
//! `runvil-types` - comes from the `effect` child module and is
//! re-exported here. The records of effects and answers stay in that
//! module; the caller reads them as JSON.

use std::fmt;
use std::sync::Arc;

use runvil_types::error::Error;
use runvil_types::event::Event;
use runvil_types::ids::Provenance;

mod effect;

#[cfg(test)]
pub(crate) use effect::{AnswerRecord, ChatAnswerRecord, EffectRecord};
pub use effect::{Effect, EffectAnswer, EffectId, PackToolSets, Round, ToolCallOrigin, ToolCaller};

use crate::prompt::Prompt;

use super::context::ChainState;
use super::params::RunParams;
use super::scheduler::Scheduler;

/// The outcome of one call to [`Run::step`]: the run either continues or
/// is over.
///
/// Closed on purpose, like [`Effect`]: the caller's loop names both
/// outcomes, and a new one breaks every caller's build.
#[derive(Debug)]
pub enum Step {
    /// The run continues.
    Pending {
        /// The effects this step issued, in issue order, each with the
        /// provenance of the task that built it, for the caller to perform
        /// and answer through [`Run::resume`]. An empty list means the run
        /// waits on effects an earlier step issued, or, when
        /// `waiting_for_input` is set, on the main chain's next message.
        effects: Vec<IssuedEffect>,
        /// The events reported since the previous step, in order, less
        /// those [`Run::deliver`] already returned.
        events: Vec<Event>,
        /// The effects this step withdrew, in effect-id order. Their
        /// answers will be discarded, so the caller may drop each one
        /// unperformed, or abort it, and answer it
        /// [`EffectAnswer::Dropped`]; it still owes each one answer. A step
        /// withdraws every effect whose chain stopped waiting on it, and
        /// the step that decides the run's result withdraws every effect
        /// still outstanding, so a caller that follows this list drops
        /// everything it holds for the run. It may name an effect that
        /// `effects` lists, because one step can issue an effect and then
        /// withdraw it.
        withdrawn: Vec<EffectId>,
        /// The main chain waits on its inbox, so only [`Run::deliver`] or
        /// a cancel can move it.
        waiting_for_input: bool,
    },
    /// The run is over.
    ///
    /// Carries the run's result and its last events. A step returns `Done`
    /// only after every issued effect has been answered. The event that
    /// marks the run's end comes with `Done`, or with an earlier `Pending`
    /// step when effects were still outstanding as the run ended, or from
    /// a [`Run::deliver`] made after an answer ended the run.
    Done {
        /// The run's result. `Ok` holds the run's final text, which for a
        /// prompt that declines its task is its reason for declining. `Err`
        /// holds the error that ended the run: a cancel is the error whose
        /// code is `ErrorCode::Canceled`, and any other error is a failure.
        /// An `Err` never holds a caller code, one in the 8xx family.
        result: Result<String, Error>,
        /// The events reported since the previous step, in order, less
        /// those [`Run::deliver`] already returned.
        events: Vec<Event>,
        /// The main chain's unread messages, oldest first: still in its
        /// inbox when the run ended.
        unread: Vec<String>,
    },
}

/// One effect a step issued, which the caller performs and answers
/// through [`Run::resume`].
#[derive(Debug)]
#[non_exhaustive]
pub struct IssuedEffect {
    /// The effect's id, which the caller passes back with its answer.
    pub id: EffectId,
    /// The provenance of the task that built the effect.
    pub provenance: Provenance,
    /// The outside work the run asks for.
    pub effect: Effect,
}

/// One run of one prompt, which the caller drives through
/// [`step`](Self::step) and [`resume`](Self::resume), and cancels through
/// the flag it supplies with
/// [`RunParams::with_cancel`](super::RunParams::with_cancel).
///
/// `Run` is `Send`: one caller drives it at a time, and the thread may
/// change between calls. It holds its prompt through an `Arc`, so the
/// caller can parse a prompt once and run it many times.
pub struct Run {
    /// The scheduler that steps every chain of the run.
    scheduler: Box<Scheduler>,
}

impl fmt::Debug for Run {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Run")
            .field("phase", &self.scheduler.phase_name())
            .field("cancel", self.scheduler.cancel_handle())
            .finish()
    }
}

impl Run {
    /// Builds a run of `prompt` with the arguments `args` and the params
    /// `params`.
    ///
    /// Building a run fills its five model roles from the params' role
    /// map, as [`RunParams::with_models`](super::RunParams::with_models)
    /// describes, and an empty map leaves the model set empty. Its tool set
    /// holds whatever catalog and roster the params' builders set.
    ///
    /// Building a run never fails. The first `step` checks, in order,
    /// that the prompt declares a supported `runvil:` version, that the
    /// params' filesystem declares a store whose backend passes its
    /// probe, and that the params meet what the prompt requires: each
    /// Pack the prompt declares is on the roster and can serve (a run given
    /// no roster skips this check), each declared role's requirements hold
    /// for the model that fills it, a non-empty role map has a `default`
    /// entry, and no tool of the catalog and no Pack of the roster has a
    /// Pack id the Engine reserves, such as `adhoc`. When a check fails,
    /// that step returns `Done` with the failure and no events. The
    /// requirements failure has the code `ErrorCode::RequirementsUnmet`,
    /// and its message names every gap. When the run's cancel flag is set
    /// as a check fails, the run ends with the cancel instead.
    #[must_use]
    pub fn new(prompt: Arc<Prompt>, args: &str, params: RunParams) -> Run {
        Self::from_state(ChainState::root(prompt, args, params))
    }

    /// [`Run::new`], with `edit` applied to the assembled state before the
    /// run starts: how a test fills in what the H1 pass would
    /// otherwise leave in the state.
    #[cfg(test)]
    pub(crate) fn new_for_test(
        prompt: Arc<Prompt>,
        args: &str,
        params: RunParams,
        edit: impl FnOnce(ChainState) -> ChainState,
    ) -> Run {
        Self::from_state(edit(ChainState::root(prompt, args, params)))
    }

    /// Builds the run over its assembled state.
    #[must_use]
    fn from_state(state: ChainState) -> Run {
        Run {
            scheduler: Box::new(Scheduler::new(state)),
        }
    }

    /// Advances the run until it needs an answer, and returns the effects
    /// it issued and the events it reported.
    ///
    /// It returns [`Step::Pending`] while the run waits on an answer or a
    /// message, and [`Step::Done`] once the run is over and every issued
    /// effect is answered. Calling `step` after `Done` is a caller error:
    /// it returns `Done` again with an internal failure.
    ///
    /// The first call checks that the run can start, as
    /// [`new`](Self::new) describes.
    ///
    /// `step` cannot fail: a run's failures are values, the error in the
    /// result of [`Step::Done`]. So the caller drives the loop and owns the
    /// retry policy without catching a panic.
    pub fn step(&mut self) -> Step {
        self.scheduler.step()
    }

    /// Delivers the answer to one effect the run issued.
    ///
    /// The chain waiting on the effect resumes with the answer at the next
    /// `step`. For [`EffectAnswer::Dropped`], it resumes with a
    /// cancellation error. Any events the answer produces come back from
    /// that next step, or from a [`deliver`](Self::deliver) made before it.
    ///
    /// Each answer must match the kind of its effect: a chat answer for a
    /// `Chat` effect, a tool answer for a `ToolCall` effect, and so on. A
    /// mismatch is an internal error that ends the run. So is an answer for
    /// an id the run never issued, or a second answer for one effect. An
    /// answer for an effect whose chain stopped waiting is discarded.
    ///
    /// `resume` cannot fail: a bad answer ends the run with an internal
    /// error, which the result of [`Step::Done`] holds, instead of
    /// returning an error or panicking.
    pub fn resume(&mut self, id: EffectId, answer: EffectAnswer) {
        self.scheduler.resume(id, answer);
    }

    /// Queues `text` for the main chain's inbox and returns the events to
    /// record before the next `step`, in order: those the answers resumed
    /// since the last step produced, then the [`Event::InboxMessage`] that
    /// reports the message. The next `step` applies it: a waiting
    /// `tasks.receive()` resumes with it; a `models.loop` request with no
    /// answer yet is withdrawn, listed in `withdrawn`, and sent again with
    /// it; otherwise it waits, and a running loop takes it before its next
    /// request or before it returns, unless its round cap is spent. A
    /// message the run never reads comes back in [`Step::Done`]'s
    /// `unread`.
    ///
    /// Call it only before `step` returns `Done`.
    pub fn deliver(&mut self, text: String) -> Vec<Event> {
        self.scheduler.deliver_message(text)
    }

    /// Whether the run's result is decided: it has reported its end
    /// event, or found at its first step that it cannot start. For the
    /// suites that pin when a run is decided apart from what its steps
    /// return.
    #[cfg(test)]
    pub(crate) fn decided_for_test(&self) -> bool {
        self.scheduler.decided()
    }

    /// The scheduler behind the run, for the suites that inspect its
    /// arena.
    #[cfg(test)]
    pub(crate) fn scheduler_for_test(&mut self) -> &mut Scheduler {
        &mut self.scheduler
    }

    /// The run's model-turn counter, for the suites that count turns.
    #[cfg(test)]
    pub(crate) fn turns_for_test(&self) -> u32 {
        self.scheduler.turns_for_test()
    }
}

#[cfg(test)]
mod tests;
