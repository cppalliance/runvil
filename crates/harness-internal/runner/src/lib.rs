//! harness-runner - the per-run Harness and the effect loop it drives. A
//! Host builds one [`Host`] at startup, holding its installed [`Packs`],
//! how it finds prompts, and its timer, and [`Host::parse`] finds a prompt
//! by its name, parses it, and checks its `uses:` list against those
//! Packs. A launcher hands each run a [`RunSetup`] with everything that
//! run gets on top of the Host. A [`Harness`] over the Host, the parsed
//! prompt, and the setup begins the run at the setup's recorder with the
//! inputs the Engine refuses to draw itself, resolves the run's model
//! through the setup's broker, prepares an Engine `Run` (putting the
//! declared input file in place, making the run's instance of each of its
//! Packs and taking the run's snapshot of them), steps it, performs each
//! chat, render, tool-call, and timer effect through its performer and
//! answers each Vfs effect inline, feeds the answers back, delivers each
//! message the launcher sends through the run's [`RunControl`] to the
//! main chain, hands every event, effect, and answer to the run's
//! recorder, ends the run there with its result, the final text or the
//! error that ended it, and reads the declared output file, all inside the
//! one future [`Harness::run`] returns. A cancel is the error whose code
//! is `canceled`. A recorder
//! that refuses a write is the only way a run ends without a recorded
//! result, and that refusal, a `RecorderFailed` error, is the only error
//! [`Harness::run`] returns. A prompt that fails to parse is its caller's
//! error and leaves no run record.
//!
//! ## Invariants
//!
//! - The Harness polls every effect inside the run's own future and starts
//!   no task, so any executor can drive a run. Performers must not block
//!   while polled: one that blocks stalls every other effect of the run,
//!   and a performer with blocking or CPU-heavy work hands it to the
//!   Host's own runtime. The crate's async needs come from `futures-util`,
//!   and `tokio` and `tokio-util` appear only under `[dev-dependencies]`
//!   (enforced by the Harness tokio ban in `cargo test -p build-xtask`).
//! - The Harness reads each [`RunSetup`] field once per run: the input is
//!   staged in, the run works in, and the declared output is read from
//!   the one filesystem the setup holds.
//! - The recorder is written in loop order: a step's events before the
//!   step's effects are issued, each effect before its performer starts,
//!   each answer before the run resumes with it, and each sent message's
//!   `InboxMessage` event as the loop delivers it, after the events of
//!   the answers resumed before it and before the step that applies it.
//!   Every effect record has exactly one answer record; a dropped
//!   effect's answer is `Dropped`.
//! - The loop never reads an event to decide anything; control comes
//!   from the run's own word (`Step` and the effects it withdraws), the
//!   cancel flag, the Host's stop, and the messages the Host sends.
//! - A Host's [`Packs`] keeps the factory of every Pack its
//!   `PackRegistry` holds, and only those. The registry holds the
//!   built-ins `runvil/subagents` and `runvil/user` from the start and
//!   refuses a duplicate FQN and a built-in's Pack id to every other
//!   Pack, so no other Pack can take `subagents` or `user`. A run whose
//!   [`RunSetup`] has no user holds `runvil/user` unavailable, with the
//!   reason "this run has no user", and never makes it.
//! - A run holds only the Packs its prompt declares and every `runvil`
//!   Pack whose Pack id no declaration takes. It makes its own instance of
//!   each of them and of no other Pack, waits on no other Pack's `ready`,
//!   and drops its instances when its future ends.
//! - Host services reach only a Plugin's `install`, and a tool call gets
//!   only facts about that call.
//! - A factory that refuses or panics leaves its Pack unavailable to that
//!   run alone: a refusal is logged at debug and a panic at error, and the
//!   next run calls the factory again.

mod display_chain;
pub mod effect_loop;
mod environment;
pub mod files;
mod harness;
mod host;
mod packs;
pub mod performers;
mod prepare;
pub mod recorder;
mod recorder_memory;
mod setup;

pub use display_chain::display_chain;
pub use harness::{Harness, RunControl, RunReport};
pub use host::Host;
pub use packs::Packs;
pub use recorder::{
    MemoryRecorder, Record, RecordKind, RecorderFuture, RunId, RunMeta, RunRecorder,
    make_recorder_error,
};
pub use setup::RunSetup;
