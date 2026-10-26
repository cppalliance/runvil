//! harness-runner - the per-run Harness and the effect loop it drives: a
//! [`Harness`] resolves the launch model through the Host's broker,
//! prepares an Engine `Run` from the prompt's source (drawing the inputs
//! the Engine refuses to draw itself, putting the declared input file in
//! place, taking the run's snapshot of the [`HostContext`]'s Packs,
//! beginning the run at its recorder), steps it, performs each chat,
//! render, tool-call, and timer effect through its performer and answers
//! each Vfs effect inline, feeds the answers back, hands every event,
//! effect, and answer to the run's recorder, and reads the declared output
//! file, all inside the one future [`Harness::run`] returns.
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
//! - The recorder is written in loop order: a step's events before the
//!   step's effects are issued, each effect before its performer starts,
//!   each answer before the run resumes with it. Every effect record has
//!   exactly one answer record; a dropped effect's answer is `Dropped`.
//! - The loop never reads an event to decide anything; control comes
//!   from the run's own word (`Step`, `Run::decided`), the cancel flag,
//!   and the Host's stop.
//! - A [`HostContext`] builds every Pack its `PackRegistry` holds, and
//!   only those. The registry holds the built-in `runvil/subagents` from
//!   the start and refuses a duplicate FQN and the built-in's Pack id to
//!   every other Pack, so no other Pack can take `subagents`.
//! - A run holds only the Packs its prompt declares and every `runvil`
//!   Pack whose Pack id no declaration takes, and waits on no other
//!   Pack's `ready`.

mod display_chain;
pub mod effect_loop;
pub mod environment;
pub mod files;
mod harness;
mod host;
pub mod performers;
pub mod prepare;
pub mod recorder;

pub use display_chain::display_chain;
pub use harness::{Harness, HarnessError, RunControl, RunReport, RunRequest};
pub use host::HostContext;
pub use recorder::{
    MemoryRecorder, Record, RecordKind, RecorderError, RecorderFuture, RunId, RunMeta, RunOutcome,
    RunRecorder,
};
