//! The Engine's test drivers: each steps a [`Run`] and answers its effects,
//! in this crate's suites and, under the `test-support` feature, in its
//! bench.
//!
//! [`drive`] is the serial sans-IO driver: it steps a run on the calling
//! thread and answers every effect the moment it is issued, through a
//! closure the caller supplies. Nothing there awaits, spawns, or sleeps; a
//! timer effect is answered however the closure sees fit, so a test's
//! timeouts are instant. [`perform_locally`] answers every effect in
//! process, a model round through a closure of the caller's, and
//! [`ScriptedChat`](scripted_chat::ScriptedChat) plays that model from a
//! script of replies.
//!
//! The `test-support` feature compiles those three alone, for
//! `benches/models_loop.rs`, its only consumer: a bench is a separate
//! target that cannot see `#[cfg(test)]` code. Only the items that bench
//! names stay `pub`; the reply builders and the request accessors are
//! `pub(crate)`, so the in-crate suites reach them and the bench cannot.
//! The rest is this crate's own test plumbing. `scenario` builds a whole
//! run from its setup as data
//! and drives it on the serial driver, and `rig` drives a scenario's run
//! with its effects tapped, its held effects released in the order a test
//! chooses or its answers shuffled under a seed, and its scheduler in
//! reach. `recording` is the suites' observation vocabulary, and its
//! `forward` replays returned events onto an observer, so the observation
//! suites hold without rewriting their assertions. `FnTool` is the
//! fixture tool, a closure answered at once.

use runvil_types::error::{Error, ErrorCode};
use runvil_types::event::Event;
use runvil_types::tools::tool_error;

use crate::execute::{Effect, EffectAnswer, EffectId, IssuedEffect, Run, Step};
use crate::lua::run_store_op;

#[cfg(test)]
pub(crate) mod recording;
#[cfg(test)]
pub(crate) mod rig;
#[cfg(test)]
pub(crate) mod scenario;
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the reply builders and the request accessors are read only by the in-crate suites; the non-test `test-support` build compiles this module for the bench, which names only `ScriptedChat`, `ScriptedReply`, its variant fields, `new`, `complete_now`, `call_count`, `drive`, and `perform_locally`"
    )
)]
pub mod scripted_chat;
#[cfg(test)]
pub(crate) mod seeded;
#[cfg(test)]
pub(crate) mod temp_root;
#[cfg(test)]
mod tools;

#[cfg(test)]
pub(crate) use recording::forward;
#[cfg(test)]
pub(crate) use tools::FnTool;

/// The author prose of a failure with `code` whose own text is `what`.
#[cfg(test)]
pub(crate) fn authored(code: ErrorCode, what: &str) -> String {
    format!("{what}. {}", code.prose())
}

/// What `tostring` gives for a caught `LuaRuntime` refusal whose own text
/// is `what`, placed on `line` of a prompt that came from no file: the
/// line's coordinates, then the [`authored`] prose.
#[cfg(test)]
pub(crate) fn refused(line: u32, what: &str) -> String {
    format!("prompt:{line}: {}", authored(ErrorCode::LuaRuntime, what))
}

/// `error` placed on `line` of a prompt that came from no file, as it
/// arrives once it crossed into a section's Lua there.
#[cfg(test)]
pub(crate) fn placed(error: crate::Error, line: u32) -> crate::Error {
    runvil_types::error::place(error, "prompt", std::num::NonZeroU32::new(line))
}

/// The location of all of `line` of a prompt that came from no file, the
/// `at` of a failure [`placed`] there.
#[cfg(test)]
pub(crate) fn at_line(line: u32) -> Option<runvil_types::error::Location> {
    std::num::NonZeroU32::new(line).map(|line| runvil_types::error::Location::line("prompt", line))
}

/// `error` as it crossed into a section's Lua on `line` of a prompt that
/// came from no file: [`placed`] there, with the [`authored`] prose of its
/// message.
#[cfg(test)]
pub(crate) fn with_author_prose(error: crate::Error, line: u32) -> crate::Error {
    let mut error = placed(error, line);
    error.prose = authored(error.code, &error.message);
    error
}

/// A fresh default handle's access capability beside the handle on its
/// scope, for tests that inject Engine values into a standalone VM.
#[cfg(test)]
#[track_caller]
pub(crate) fn fresh_access() -> (std::sync::Arc<runvil_vfs::Access>, runvil_vfs::ScopeHandle) {
    let (access, scope) = runvil_vfs::VfsRef::default()
        .acquire_scope(runvil_vfs::Origin::new("test fixture"))
        .expect("the stock backend acquires");
    (std::sync::Arc::new(access), scope)
}

/// Performs one effect locally, with no I/O: a store operation runs on the
/// effect's own access handle, a timer fires at once, a render passes
/// every offered tool through, a bound tool is unbound, and a model round
/// is answered by `chat`, a function of the effect alone so the answer
/// never depends on arrival order.
///
/// # Panics
///
/// Only when `chat` panics: a store operation's failure is answered as a
/// `Vfs` error, not raised.
pub fn perform_locally(
    effect: &Effect,
    chat: &mut impl FnMut(&Effect) -> EffectAnswer,
) -> EffectAnswer {
    match effect {
        Effect::Chat { .. } => chat(effect),
        Effect::Render { offered, .. } => EffectAnswer::Render(offered.clone()),
        Effect::ToolCall { alias, .. } => EffectAnswer::ToolCall(Err(tool_error(
            ErrorCode::ToolOther,
            format!("no tool is bound as {alias} in this test"),
        ))),
        Effect::Vfs { access, op } => EffectAnswer::Vfs(run_store_op(access, op.clone())),
        Effect::Timer { .. } => EffectAnswer::Timer,
    }
}

/// Drives `run` to its end on the calling thread, performing every effect
/// through `perform` as it is issued, and returns the run's result with
/// every event it reported, in order.
///
/// The driver is the simplest correct caller of a run. After each `step`
/// it answers the step's effects in issue order, each through `perform`,
/// and steps again. An effect the step also lists as withdrawn is answered
/// [`EffectAnswer::Dropped`] without reaching `perform`, as the run asks
/// of its caller.
///
/// `perform` is handed the effect's id beside the effect so a scripted
/// performer can correlate answers however it likes; it must return an
/// answer of the effect's own kind (or `Dropped`), as the run requires.
///
/// # Panics
///
/// Only when `perform` panics. An answer of the wrong kind ends the run
/// with an internal failure in its result, and so does a pending step
/// that issued no effect.
pub fn drive(
    mut run: Run,
    mut perform: impl FnMut(EffectId, &Effect) -> EffectAnswer,
) -> (Result<String, Error>, Vec<Event>) {
    let mut reported = Vec::new();
    let mut answer = |id: EffectId, effect: &Effect| Some(perform(id, effect));
    loop {
        let stepped = drive_step(
            &mut run,
            &mut |_| {},
            &mut answer,
            &mut |_, _| {},
            &mut Vec::new(),
            &mut reported,
        );
        if let Some(result) = stepped {
            return (result, reported);
        }
    }
}

/// One step of [`drive`]: steps `run`, appends the step's events to
/// `reported`, hands every effect the step issued to `issued` in issue
/// order, answers each through `perform`, hands each effect and its answer
/// to `answered`, `Dropped` ones included, and returns the run's result
/// once it has one.
///
/// An effect `perform` answers `None` joins `held`, unanswered. Every
/// effect the step withdraws, whether it issued the effect or an earlier
/// step did and `held` keeps it, is answered `Dropped`, so the step that
/// decides the run's result drops every held effect.
pub(crate) fn drive_step(
    run: &mut Run,
    issued: &mut impl FnMut(&Effect),
    perform: &mut impl FnMut(EffectId, &Effect) -> Option<EffectAnswer>,
    answered: &mut impl FnMut(&Effect, &EffectAnswer),
    held: &mut Vec<(EffectId, Effect)>,
    reported: &mut Vec<Event>,
) -> Option<Result<String, Error>> {
    match run.step() {
        Step::Done { result, events } => {
            reported.extend(events);
            Some(result)
        }
        Step::Pending {
            effects,
            events,
            withdrawn,
        } => {
            reported.extend(events);
            if effects.is_empty() && held.is_empty() {
                // Every effect is answered the step it is issued unless it
                // is held, so a pending step that issued nothing while
                // holding nothing has nothing to wait on: an invariant
                // failure reported rather than a hang.
                return Some(Err(crate::error::internal(
                    "the serial driver was handed a pending run with no effect to answer",
                )));
            }
            for IssuedEffect { id, effect, .. } in effects {
                issued(&effect);
                let answer = if withdrawn.contains(&id) {
                    Some(EffectAnswer::Dropped)
                } else {
                    perform(id, &effect)
                };
                match answer {
                    Some(answer) => {
                        answered(&effect, &answer);
                        run.resume(id, answer);
                    }
                    None => held.push((id, effect)),
                }
            }
            let (dropped, kept): (Vec<_>, Vec<_>) = std::mem::take(held)
                .into_iter()
                .partition(|(id, _)| withdrawn.contains(id));
            *held = kept;
            for (id, effect) in dropped {
                answered(&effect, &EffectAnswer::Dropped);
                run.resume(id, EffectAnswer::Dropped);
            }
            None
        }
    }
}
