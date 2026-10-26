//! Effects as values: what the Engine asks its caller to perform, and what
//! the caller answers with.
//!
//! A leaf request a section VM yields - a model round, a bound tool call,
//! a store operation, a timer - is not performed where it is dispatched.
//! The arm builds an [`Effect`], a plain description of the work, and the
//! run returns it from `step` for the caller to perform; the caller's
//! [`EffectAnswer`] comes back through `resume` keyed by the effect's
//! [`EffectId`], and the scheduler applies it on the caller's thread,
//! emitting the round's events there. The Engine thus decides *what* to do
//! and *what it means*; the Harness performs it. A section entry's first
//! model round that offers catalog tools first issues a render, whose
//! answer the Engine merges into the entry's frozen sets before it issues
//! the round.
//!
//! An [`Effect`] may hold a live handle (a filesystem access) and so does
//! not serialize itself. [`Effect::record`] projects it onto an
//! [`EffectRecord`], the effect minus its handles, which round-trips
//! through serde: a run log stores records, and a later replay compares a
//! re-executed run's records against them. An [`EffectAnswer`] likewise
//! contains values a log cannot hold whole (a completion's bodies, an
//! error's boxed cause); [`EffectAnswer::record`] projects it onto an
//! [`AnswerRecord`], the answer's outcome as a log stores it. The record
//! types sit in the `record` child module and are re-exported here.

#[path = "effect-record.rs"]
mod record;

use std::sync::Arc;
use std::time::Duration;

use runvil_types::event::ReplyOrigin;
use runvil_types::ids::RoundId;
pub use runvil_types::tools::{ToolCallOrigin, ToolCaller};
use runvil_types::tools::{ToolError, ToolId, ToolInfo, ToolOutput};
use runvil_vfs::{Access, VfsError};
use serde::Serialize;
use serde_json::Value;

use crate::execute::protocol::{VfsOp, VfsOutcome};
use crate::model::{Completion, CompletionError, Message, ToolSchema};
use crate::model::{CompletionOptions, ModelBinding, Temperature};

pub use record::{
    AnswerRecord, ChatAnswerRecord, EffectRecord, FailureRecord, PackToolSets, ToolAnswerRecord,
};

/// The identifier of one in-flight effect, which pairs an issued
/// [`Effect`] with its [`EffectAnswer`].
///
/// The id is opaque. The run allocates it from a run-wide counter, so the
/// same effect may get a different id in another run. Ids order by issue
/// within a run: an effect issued later has a greater id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EffectId(pub(crate) u64);

impl EffectId {
    /// Returns the id as a raw number. The caller can use it as a key, for
    /// example in its log or its task table. The number is meaningful only
    /// within the run that issued it.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for EffectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The identity of one model round: its id and the path that dispatched
/// it. Every [`Effect::Chat`] carries one.
///
/// Ids number the run's model rounds from 0 in dispatch order. Chat rounds
/// and nested `models.infer` rounds share one sequence. The thinking,
/// reply, and tool-call events reported from the round's answer carry the
/// same id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Round {
    /// The round's run-wide id.
    pub id: RoundId,
    /// The path that dispatched the round. [`ReplyOrigin::Chat`] marks a
    /// section's `chat` round, the kind `models.loop` runs, whose live
    /// deltas the caller can stream. [`ReplyOrigin::Infer`] marks a nested
    /// `models.infer` round, where only the completed reply is consumed.
    pub origin: ReplyOrigin,
}

impl Round {
    /// Builds the identity of round `id`, dispatched from `origin`.
    #[must_use]
    pub fn new(id: RoundId, origin: ReplyOrigin) -> Round {
        Round { id, origin }
    }
}

/// One piece of work the Engine asks the caller to perform.
///
/// The enum is closed on purpose: it has no `#[non_exhaustive]`, so the
/// caller's `match` names every kind and needs no wildcard arm. A new
/// effect kind breaks every caller's build, rather than reaching a caller
/// that cannot perform it.
#[derive(Debug)]
pub enum Effect {
    /// One model round: send `messages` to the model with `tools`
    /// advertised, using the frozen `options` built from `binding`. A
    /// nested `models.infer` round sends one user message with an empty
    /// tool list, and the Engine consumes only its completed reply.
    Chat {
        /// The binding the round runs under.
        binding: ModelBinding,
        /// The conversation to send, in wire order.
        messages: Vec<Message>,
        /// The earlier round whose request this one extends: the previous
        /// send of the same section's `messages` list. `None` on a
        /// section's first send and on a nested `models.infer` round.
        after: Option<RoundId>,
        /// How many leading messages this request shares with `after`'s
        /// request; 0 when `after` is `None`.
        keep: u64,
        /// The tool schemas advertised to the model for this round.
        tools: Vec<ToolSchema>,
        /// The per-request completion options, built from `binding`.
        options: CompletionOptions,
        /// The round's id and origin. The caller needs to forward live
        /// deltas only for a round whose origin is [`ReplyOrigin::Chat`].
        /// The effect's record keeps the id and drops the origin, because
        /// the request sent to the model is the same for either origin.
        round: Round,
    },
    /// The render of the catalog Packs a section entry offered, issued at
    /// the entry's first model round before that round's request is
    /// built. The caller answers with the tools each Pack shows the model
    /// for each set, and the Engine merges them with the section's adhoc
    /// tools into the entry's frozen sets, which every round of the entry
    /// then sends. A caller that changes nothing answers with the offered
    /// tools. The effect's record keeps the binding's alias and each
    /// offered tool's id.
    Render {
        /// The section entry, the value its Lua reads as `sys.id`, which
        /// the origin of each of the entry's tool calls also names.
        entry: String,
        /// The binding the entry's first round runs under.
        binding: ModelBinding,
        /// Each offered catalog Pack once, in the order its first tool was
        /// offered, with the tools the section offered from it into each
        /// set.
        offered: Vec<PackToolSets<ToolInfo>>,
    },
    /// One call to a Pack's tool: a run catalog tool, or a tool only the
    /// model sees, from the section entry's render. The caller resolves
    /// `tool`, the tool's stable identity, to an implementation. `alias`
    /// is the tool's wire name for a model call and its id for a script
    /// call, and `origin` says who made the call and where. The effect's
    /// record keeps both and leaves out `access`, a live handle, and
    /// `survives_stop`, which the run catalog and the render's answer
    /// already give.
    ToolCall {
        /// The tool's stable live identity.
        tool: ToolId,
        /// The tool's wire name for a model call, or its id for a script call.
        alias: String,
        /// The call's arguments.
        args: Value,
        /// Who made the call: the run, the section and its entry, and the
        /// section's script or the model whose round asked for it.
        origin: ToolCallOrigin,
        /// The call's own identity, forked from the calling chain's
        /// access when the call is issued and joined back into it when the
        /// answer is applied or the call is aborted, so the chain's earlier
        /// work happens before the tool's and the tool's before the chain's
        /// next step. It is rooted at `/`. Once the answer is applied or the
        /// call is aborted, every operation through it is refused.
        access: Arc<Access>,
        /// Whether a stop leaves the call in flight, from the binding the
        /// call resolved to: the run catalog's flag for a catalog tool, and
        /// the flag the section entry's render gave a tool only the model
        /// sees.
        survives_stop: bool,
    },
    /// One operation on the run's store view, issued for one of the eight
    /// `store.*` calls a prompt can make.
    ///
    /// `access` is an ordinary access that the Engine derives from the
    /// chain's capability at dispatch. It is rooted at the store declared
    /// by the handle that the chain's access came from. Only the `store.*`
    /// calls produce this effect. A tool reaches files through the access
    /// its [`ToolCall`](Effect::ToolCall) effect carries.
    ///
    /// The caller must use the access exactly as given and within the
    /// scope it carries. Correctness holds whenever the caller drops the
    /// access. Store claims follow happens-before order within the
    /// run's scope. The run ends that scope at `Done` or when the run is
    /// dropped, and after that the view refuses every operation.
    Vfs {
        /// The chain's store view: an access with the chain's identity
        /// that reaches only the store root.
        access: Arc<Access>,
        /// The validated operation.
        op: VfsOp,
    },
    /// One sleep of `duration`, which the Engine uses as the timeout
    /// behind a timed wait.
    Timer {
        /// How long the sleep lasts.
        duration: Duration,
    },
}

/// One value's serde wire form. Every type recorded here serializes
/// infallibly (strings, numbers, and JSON values), so the `Null` fallback
/// is unreachable in practice and stands only so the projection stays
/// total.
fn wire_value<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// One offered Pack's tools as their ids, the form a render's record
/// keeps.
fn offered_ids(sets: &PackToolSets<ToolInfo>) -> PackToolSets<ToolId> {
    let ids = |tools: &[ToolInfo]| tools.iter().map(|tool| tool.id.clone()).collect();
    PackToolSets::new(
        sets.pack.clone(),
        ids(&sets.static_set),
        ids(&sets.dynamic_set),
    )
}

impl Effect {
    /// Returns the effect's record: the same request with its live handles
    /// removed, in a form that a run log stores and a replay compares.
    #[must_use]
    pub fn record(&self) -> EffectRecord {
        match self {
            Effect::Chat {
                binding,
                messages,
                after,
                keep,
                tools,
                round,
                ..
            } => {
                let invocation = binding.invocation();
                let kept = usize::try_from(*keep).unwrap_or(usize::MAX);
                EffectRecord::Chat {
                    round: round.id,
                    alias: binding.alias().to_owned(),
                    after: *after,
                    keep: *keep,
                    messages: messages.iter().skip(kept).map(wire_value).collect(),
                    tools: tools
                        .iter()
                        .map(|schema| schema.name().to_owned())
                        .collect(),
                    temperature: invocation.temperature.map(Temperature::get),
                    max_tokens: invocation.max_tokens.map(std::num::NonZeroU32::get),
                    thinking: invocation.thinking,
                }
            }
            Effect::Render {
                entry,
                binding,
                offered,
            } => EffectRecord::Render {
                alias: binding.alias().to_owned(),
                entry: entry.clone(),
                offered: offered.iter().map(offered_ids).collect(),
            },
            Effect::ToolCall {
                tool,
                alias,
                args,
                origin,
                ..
            } => EffectRecord::ToolCall {
                tool: tool.clone(),
                alias: alias.clone(),
                args: args.clone(),
                origin: origin.clone(),
            },
            Effect::Vfs { op, .. } => EffectRecord::Vfs { op: op.clone() },
            Effect::Timer { duration } => EffectRecord::Timer {
                seconds: duration.as_secs_f64(),
            },
        }
    }
}

/// The answer to one [`Effect`]: one variant per effect kind, plus
/// [`Dropped`](EffectAnswer::Dropped) for an effect the caller gave up on.
/// Every effect receives exactly one answer.
///
/// Closed on purpose, like [`Effect`]: a new answer kind arrives with a
/// new effect kind, and both break every caller's build.
#[derive(Debug)]
pub enum EffectAnswer {
    /// The model round's completion or its failure.
    Chat(std::result::Result<Box<Completion>, CompletionError>),
    /// Each offered Pack's rendered tools for each set, in the order the
    /// render asked: the same Packs, with each set the Pack was not
    /// offered into left empty. Every rendered tool's id starts with its
    /// Pack's id, and a run catalog tool may appear only in a set the
    /// section offered it in; an answer that breaks either rule, or the
    /// order, fails the round. A catalog tool keeps the catalog's output
    /// kind, listing flag, and stop flag, so a render changes only its
    /// wire name, description, and schema, and any other tool is one only
    /// the model can call, with the flags the render gave it. A tool
    /// repeated within a set keeps its first copy.
    Render(Vec<PackToolSets<ToolInfo>>),
    /// The tool's own output or its own failure, before the Engine applies
    /// its trust rule.
    ToolCall(std::result::Result<ToolOutput, ToolError>),
    /// The outcome of the operation on the run's store view, or the
    /// store's own structured failure.
    Vfs(std::result::Result<VfsOutcome, VfsError>),
    /// The timer fired.
    Timer,
    /// The caller gave up on the effect and dropped it, for example
    /// because the run was cancelled or the effect's task ended first. If
    /// the chain still waits on the effect, it resumes with a cancelled
    /// error. A drop is an answer like any other, so it counts as the
    /// effect's one answer.
    Dropped,
}

impl EffectAnswer {
    /// Returns the answer's record: the parts of its outcome that a log can
    /// hold whole. A failure is recorded as a [`FailureRecord`]: its kind's
    /// tag and its display text. A completion is recorded as its reply or
    /// its requested tool names, and a render as its rendered tools whole.
    /// The round's request and response bodies travel as debug events, and
    /// its metrics travel in the turn's event.
    #[must_use]
    pub fn record(&self) -> AnswerRecord {
        match self {
            EffectAnswer::Chat(result) => AnswerRecord::Chat(match result {
                Ok(completion) => Ok(ChatAnswerRecord::from(completion.as_ref())),
                Err(error) => Err(FailureRecord::new(error.kind().tag(), error.to_string())),
            }),
            EffectAnswer::Render(rendered) => AnswerRecord::Render(rendered.clone()),
            EffectAnswer::ToolCall(result) => AnswerRecord::ToolCall(match result {
                Ok(output) => Ok(ToolAnswerRecord::new(output.text(), output.trust())),
                Err(error) => Err(FailureRecord::new(error.kind().tag(), error.to_string())),
            }),
            EffectAnswer::Vfs(result) => AnswerRecord::Vfs(match result {
                Ok(outcome) => Ok(outcome.clone()),
                Err(error) => Err(FailureRecord::new(error.tag(), error.to_string())),
            }),
            EffectAnswer::Timer => AnswerRecord::Timer,
            EffectAnswer::Dropped => AnswerRecord::Dropped,
        }
    }
}

#[cfg(test)]
#[path = "effect-tests.rs"]
mod tests;
