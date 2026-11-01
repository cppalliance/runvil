//! The run: the Engine's boundary with its caller, which `Run`'s methods
//! cross by exchanging effects and events as values. The caller's loop is
//! documented on the `runvil` facade's crate page and its `effect` and
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

use runvil_types::event::Event;
use runvil_types::ids::Provenance;

mod effect;

#[cfg(test)]
pub(crate) use effect::{AnswerRecord, ChatAnswerRecord, EffectRecord};
pub use effect::{Effect, EffectAnswer, EffectId, PackToolSets, Round, ToolCallOrigin, ToolCaller};

use crate::parser::Prompt;

use super::RunOutcome;
use super::config::RunConfig;
use super::context::ChainState;
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
        /// and answer through [`Run::resume`]. An empty list means every
        /// chain is waiting on an effect that an earlier step issued.
        effects: Vec<IssuedEffect>,
        /// The events the step reported, in order.
        events: Vec<Event>,
        /// The effects this step withdrew, in effect-id order. Their
        /// answers will be discarded, so the caller may drop each one
        /// unperformed, or abort it, and answer it
        /// [`EffectAnswer::Dropped`]; it still owes each one answer. A step
        /// withdraws a tool call whose chain stopped waiting on it. The
        /// step that decides the run's outcome withdraws every effect still
        /// outstanding, of every kind, so a caller that follows this list
        /// drops everything it holds for the run. It may name an effect
        /// that `effects` lists, because one step can issue an effect and
        /// then withdraw it.
        withdrawn: Vec<EffectId>,
    },
    /// The run is over.
    ///
    /// Carries the run's outcome and its last events. A step returns `Done`
    /// only after every issued effect has been answered. The event that
    /// marks the run's end comes with `Done`, or with an earlier `Pending`
    /// step when effects were still outstanding as the run ended.
    Done {
        /// The run's outcome.
        outcome: RunOutcome,
        /// The events reported since the previous step.
        events: Vec<Event>,
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
/// [`RunConfig::with_cancel`](super::RunConfig::with_cancel).
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
    /// Builds a run of `prompt` with the arguments `args` and the config
    /// `config`.
    ///
    /// Building a run fills its five model roles from the config's role
    /// map, as [`RunConfig::with_models`](super::RunConfig::with_models)
    /// describes, and an empty map leaves the model set empty. Its tool set
    /// holds whatever catalog and roster the config's builders set.
    ///
    /// Building a run never fails. The first `step` checks, in order,
    /// that the prompt declares a supported `runvil:` version, that the
    /// config's filesystem declares a store whose backend passes its
    /// probe, and that the config meets what the prompt requires: each
    /// Pack the prompt declares is on the roster and can serve (a run given
    /// no roster skips this check), each declared role's requirements hold
    /// for the model that fills it, a non-empty role map has a `default`
    /// entry, and no tool of the catalog and no Pack of the roster has a
    /// Pack id the Engine reserves, such as `adhoc`. When a check fails,
    /// that step returns `Done` with the failure and no events. The
    /// requirements failure is a
    /// [`RequirementsUnmet`](super::RunErrorKind::RequirementsUnmet) whose
    /// message names every gap. When the run's cancel flag is set as a
    /// check fails, the run ends [`Cancelled`](RunOutcome::Cancelled)
    /// instead.
    #[must_use]
    pub fn new(prompt: Arc<Prompt>, args: &str, config: RunConfig) -> Run {
        Self::from_state(ChainState::root(prompt, args, config))
    }

    /// [`Run::new`], with `edit` applied to the assembled state before the
    /// run starts: how a test fills in what the H1 pass would
    /// otherwise leave in the state.
    #[cfg(test)]
    pub(crate) fn new_for_test(
        prompt: Arc<Prompt>,
        args: &str,
        config: RunConfig,
        edit: impl FnOnce(ChainState) -> ChainState,
    ) -> Run {
        Self::from_state(edit(ChainState::root(prompt, args, config)))
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
    /// It returns [`Step::Pending`] while the run waits on an answer, and
    /// [`Step::Done`] once the run is over and every issued effect is
    /// answered. Calling `step` after `Done` is a caller error: it returns
    /// `Done` again with an internal failure.
    ///
    /// The first call checks that the run can start, as
    /// [`new`](Self::new) describes.
    ///
    /// `step` cannot fail: a run's failures are values in
    /// [`RunOutcome::Failed`]. So the caller drives the loop and owns the
    /// retry policy without catching a panic.
    pub fn step(&mut self) -> Step {
        self.scheduler.step()
    }

    /// Delivers the answer to one effect the run issued.
    ///
    /// The chain waiting on the effect resumes with the answer at the next
    /// `step`. For [`EffectAnswer::Dropped`], it resumes with a
    /// cancellation error. Any events the answer produces are reported by
    /// that next step.
    ///
    /// Each answer must match the kind of its effect: a chat answer for a
    /// `Chat` effect, a tool answer for a `ToolCall` effect, and so on. A
    /// mismatch is an internal error that ends the run. So is an answer for
    /// an id the run never issued, or a second answer for one effect. An
    /// answer for an effect whose chain stopped waiting is discarded.
    ///
    /// `resume` cannot fail: a bad answer ends the run with a
    /// [`RunOutcome::Failed`] instead of returning an error or panicking.
    pub fn resume(&mut self, id: EffectId, answer: EffectAnswer) {
        self.scheduler.resume(id, answer);
    }

    /// Whether the run's outcome is decided: it has reported its end
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

    /// Takes the Engine error behind the failed outcome the last `Done`
    /// carried, for the suites that match on its variant.
    #[cfg(test)]
    pub(crate) fn take_failure_for_test(&mut self) -> Option<crate::Error> {
        self.scheduler.take_failure_for_test()
    }
}

#[cfg(test)]
mod tests;
