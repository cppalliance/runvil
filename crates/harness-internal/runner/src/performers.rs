//! One performer trait for chat effects, one for tool-call and render
//! effects, and one for timer effects: the traits the effect loop
//! performs a run's effects through.
//!
//! The Engine issues an [`Effect`](runvil_engine::effect::Effect) as a
//! value and waits for its
//! [`EffectAnswer`](runvil_engine::effect::EffectAnswer); a performer is
//! the Harness code that turns the one into the other. Each trait takes the
//! effect's fields and returns the answer's payload for its kind, so a
//! performer never sees the run, the recorder, or another kind's effects. The
//! effect loop owns the correlation: it hands each result back to the run
//! under the effect's id and writes the answer's record.
//!
//! Each performer returns a boxed `'static` future that the loop polls
//! inside the run's own future, beside every other effect in flight, so a
//! performer must move what its future needs into it. A performer must not
//! block while polled: one that blocks stalls every other effect of the
//! run, and the run's stop and cancel with them. A performer with blocking
//! or CPU-heavy work hands it to the Host's own runtime and awaits the
//! result. A `Vfs` effect has no performer: the VFS is synchronous by
//! design, so the loop answers it inline through the Engine's store
//! operation.
//!
//! The runner supplies the tool performer itself: the run's snapshot of
//! the Host's Packs, taken at preparation. The Host supplies the
//! [`InferenceBroker`] and the [`Timer`].

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use runvil_engine::effect::{PackToolSets, Round, ToolCallOrigin};
use runvil_engine::model::{
    Completion, CompletionOptions, Message, ModelBinding, ModelCatalog, ModelInfo, ToolSchema,
};
use runvil_engine::vfs::Access;
use runvil_engine::{BoxFuture, Error, Timestamp};
use runvil_engine::{ToolId, ToolInfo, ToolOutput};
use serde_json::Value;

/// The model inference the Host supplies to the Harness.
///
/// A broker lists the models it serves and performs each `Chat` effect as
/// one model round.
///
/// The Harness polls each round inside the run's own future, so a broker
/// must not block while polled. A broker sends blocking or CPU-heavy work
/// to the Host's own runtime.
///
/// Each method returns a [`BoxFuture`] for the Harness to poll. The future
/// is `Send` and `'static`. It can move between threads, and it owns the
/// data it uses.
pub trait InferenceBroker: Send + Sync {
    /// Lists the models the broker serves.
    fn models(&self) -> BoxFuture<'static, Result<ModelCatalog, Error>>;

    /// Runs one model round and returns the finished reply.
    ///
    /// The round runs under `binding`. It sends `messages` to the model
    /// with `tools` advertised, using `options`, the fixed request settings
    /// built from `binding`. `round` holds the round's run-wide id and the
    /// path that dispatched it.
    ///
    /// The Harness takes only the finished reply. A broker that shows the
    /// reply as it forms streams it on its own.
    fn chat(
        &self,
        binding: ModelBinding,
        messages: Vec<Message>,
        tools: Vec<ToolSchema>,
        options: CompletionOptions,
        round: Round,
    ) -> BoxFuture<'static, Result<Box<Completion>, Error>>;
}

/// Performs a `ToolCall` effect: resolves `tool` to an implementation and
/// calls it with `args`, lending it the call's `access` and `origin`. Also
/// performs a `Render` effect: the tools each offered Pack shows the model.
///
/// Each method returns a [`BoxFuture`] for the Harness to poll. The future
/// is `Send` and `'static`. It can move between threads, and it owns the
/// data it uses.
pub trait ToolPerformer: Send + Sync {
    /// Calls the tool. `alias` is the tool's wire name for a model call and
    /// its canonical id for a script call, for the performer's own
    /// diagnostics; `tool` is the identity it resolves. `access` is the
    /// call's own filesystem access and `origin` says who made the call.
    /// The returned future owns both until it finishes, and drops them
    /// when it finishes or is dropped.
    fn call(
        &self,
        tool: ToolId,
        alias: String,
        access: Arc<Access>,
        origin: ToolCallOrigin,
        args: Value,
    ) -> BoxFuture<'static, Result<ToolOutput, Error>>;

    /// Renders the tools the section entry `entry` offered from each Pack
    /// in `offered`, for `model`, the model of the entry's first round, and
    /// answers every Pack asked, in the order asked, with the tools it
    /// shows the model for each set. The default passes every tool
    /// through.
    fn render(
        &self,
        _entry: String,
        _model: ModelInfo,
        offered: Vec<PackToolSets<ToolInfo>>,
    ) -> BoxFuture<'static, Vec<PackToolSets<ToolInfo>>> {
        Box::pin(async move { offered })
    }
}

/// The Host's clock, which performs each `Timer` effect as one sleep and
/// stamps each run's start.
///
/// The Harness polls each sleep inside the run's own future, so a timer
/// must not block while polled. It waits on the Host's own runtime.
pub trait Timer: Send + Sync {
    /// Resolves once `duration` has passed.
    ///
    /// The returned [`BoxFuture`] is `Send` and `'static`. It can move
    /// between threads, and it owns the data it uses.
    fn sleep(&self, duration: Duration) -> BoxFuture<'static, ()>;

    /// The time now. The Harness stamps each run's
    /// `started_at` with it. The default reads the
    /// system clock.
    fn now(&self) -> Timestamp {
        // A clock before the epoch or beyond `i64` milliseconds, neither
        // reachable on a real machine, saturates to the epoch rather than
        // refusing the run.
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok())
            .map_or(Timestamp::UNIX_EPOCH, Timestamp::from_unix_millis)
    }
}
