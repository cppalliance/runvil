//! The chain scheduler: the coroutine protocol's state machine.
//!
//! One [`Scheduler`] per run, owned by the [`Run`](super::run::Run) that
//! the caller steps: no `Arc`, no `Mutex`, no sharing. One caller at a time
//! runs every chain step, and the Lua shims only yield (they never call
//! into Rust for suspending operations), so the scheduler state is
//! unreachable from Lua. Nothing here awaits, spawns, or sleeps: a leaf
//! request becomes an [`Effect`](super::run::Effect) the step hands out,
//! and the caller's [`EffectAnswer`] comes back through `resume`. The
//! scheduler is `Send` and moves between threads between calls.
//!
//! The loop is `resume -> match request -> dispatch -> resume with answer`.
//! A chain whose coroutine yields a leaf request (`infer`) awaits its
//! answer in the effect table while its [`Effect`](super::run::Effect) is
//! out with the caller: the arm builds the effect as a value, `issue`
//! stamps it with the chain's task provenance and queues it for the step's
//! return, and `apply_answer` turns the caller's answer into the chain's
//! protocol answer on the caller's thread, emitting the round's events
//! there. A chain that
//! yields a structural request (`call`) blocks while its child chain runs,
//! and the child's finish delivers its final text as the parent's answer.
//! When no chain is ready the step returns and the caller performs the effects.
//! Every `dispatch_*` arm with a fallible half resumes that half's error
//! into the calling chain as the call's answer (`tool_calls` into the
//! failing call's batch slot), so an author `pcall` catches it. An
//! `Error::Internal` from a scheduler method marks a state the scheduler's
//! own invariants rule out: a scheduler or shim bug.
//!
//! [`RunState`] stays the ambient shared read-mostly context, cloned into
//! chains and callbacks; the scheduler owns the run's copy and is the
//! exclusively owned mutable counterpart. The two are deliberately not
//! merged: `RunState` is cloned into callbacks, while the scheduler must
//! stay unreachable from the callback layer. The prompt tree is shared
//! through the context's `Arc`, so a chain names its position in it by
//! path ([`SectionPos`](crate::heading_address::SectionPos)) rather than by
//! borrow, and the scheduler owns itself outright.
//!
//! This file holds the scheduler core: the ready queue, the admission
//! queues (resumed chains, admitted first, and fresh spawns, which wait
//! there until the run's concurrency limits admit them), and the task
//! arena. The core owns two sub-objects that each hold their state and
//! enforce their own rules, and neither writes the other: the chain arena
//! and the effect table. A request that touches only its own chain runs
//! in a view of that chain, which borrows the chain, the effect table, and
//! the run context and nothing else, and holds the one `issue` path every
//! leaf arm hands its effect through; the core keeps every request that
//! coordinates more than one owner. The submodules hold the rest:
//! `effects` the effect table and its entries (the `Continuation` an
//! answer is applied by), `chain_ops` the one-chain view, `drive` the
//! run-level step (the ready-queue drain, the terminal rules, and the
//! teardown), `apply` the answer application, `chain` the chain lifecycle
//! (chain start and the two chain-end paths), `chain_record` the chain
//! record and the arena that holds it, `step` one chain's step to its next
//! suspension point, `walk` the section walk rules, `h1` the H1 pass and
//! its hand-off to the walk, `dispatch` the request arms, `chat` the one-round `chat` arm and its
//! answer application, `tool_call` the script and model-issued `tool_call`
//! arm (the two arms the section-visible `models.loop` shim drives),
//! `tasks` the task arena and the `spawn` arm, `admission` the admission
//! limits, `task_end` the chain-end rules for tasks, `waits` the `join_any`
//! wait and the `ready`, `status`, `pending`, `note`, `concurrency`, and
//! `cancel` arms over the arena, `timer` the wait shims' internal timeout
//! as an effect-backed entry, and `test_hooks` (test builds only) the seams
//! the suites inspect the arena through. A fanout is Lua over those arms
//! (the `fanout` shim spawns one task per member, all up front, and waits
//! on the live set).

mod admission;
mod apply;
mod chain;
mod chain_ops;
mod chain_record;
mod chat;
mod dispatch;
mod drive;
mod effects;
mod h1;
mod step;
mod task_end;
mod tasks;
#[cfg(test)]
pub(crate) mod test_hooks;
mod timer;
mod tool_call;
mod waits;
mod walk;

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use runvil_types::event::lifecycle;
use runvil_types::ids::TaskId;
use runvil_vfs::Origin;
use runvil_vfs::detail::ScopeHandle;

use crate::lua::Answer;
use crate::parser::{Block, Prompt};
use crate::{Error, Result};

use super::context::RunState;
use super::run::{EffectAnswer, EffectId};
use chain_ops::ChainOps;
use chain_record::{Chain, ChainArena, ChainIndex, ChatAnchor, Counters};
use effects::{CallSite, Continuation, EffectTable, Pending, ToolCallContinuation};
use tasks::TaskEntry;

/// The deepest nesting a chain may run at, counting `call` children and
/// spawned tasks, fanout arms included; the first level counts.
const MAX_CALL_DEPTH: usize = 8;

/// The run's final result when no section produced a reply, which three
/// fallback sites share: a prompt with neither H1 blocks nor sections, a
/// top-level walk that ends without a scalar, and an H1 pass with no walk
/// after it.
const GENERIC_COMPLETION: &str = "done";

/// The most precise prompt-source line known for `blocks`: the first
/// compiled chunk's absolute source line, else the prompt's opening line.
fn first_chunk_line(blocks: &[Block]) -> u32 {
    blocks
        .iter()
        .find_map(|block| match block {
            Block::Lua(program) => Some(program.source_line().get()),
            _ => None,
        })
        .unwrap_or(1)
}

/// The observability origin for a capability the run acquires: `label` is
/// the section or pass the capability serves, and the prompt's title
/// stands in for a file name - a prompt's name is its title, since the
/// source may never have existed on disk.
fn prompt_origin(prompt: &Prompt, label: &str, blocks: &[Block]) -> Origin {
    Origin::at(label, prompt.title(), first_chunk_line(blocks))
}

/// A queued task chain's spawn record, held from spawn until its
/// admission: the input override and the spawning section the start event
/// reports. The `var` snapshot lives in the chain's `var` slot from spawn
/// on, moved once out of the yield - never copied - so a queued chain
/// shares it until admission installs it into the first section.
#[derive(Debug)]
struct SpawnRecord {
    /// The `opts.input` override, reported in the start event.
    input: Option<String>,
    /// The spawner's section at spawn, reported as the start event's
    /// section: a task admitted after its spawner moved on still reports
    /// where it was spawned, not where the spawner is now.
    section: String,
}

/// The run's phase, as the step loop reads it.
enum Phase {
    /// No chain has started: the next step starts the H1 pass or the walk.
    Fresh,
    /// Chains run; the outcome is undecided.
    Running,
    /// The outcome is decided and the run's end boundary reported; `Done`
    /// waits on the outstanding effects.
    Ending(Result<String>),
    /// `Done` was returned.
    Done,
}

/// The coroutine protocol's driver: the chain arena, ready queue, effect
/// table, and task arena, owned outright by the run.
pub(crate) struct Scheduler {
    /// The ambient run context, cloned into chains and forked by call
    /// chains.
    ctx: RunState,
    /// The chain arena: append-only, indexed by [`ChainIndex`].
    chains: ChainArena,
    /// Chains eligible to resume (FIFO); empty whenever a step returns,
    /// because `drain` runs it dry or the run's end clears it.
    ready: VecDeque<ChainIndex>,
    /// Chains parked on a task wait that gave their admission slots back,
    /// waiting to take them again: a task chain, or a call chain that gave
    /// back its nearest holding ancestor's slots. Each keeps its Lua VM and
    /// holds its delivered answer; readmission marks the holder holding and
    /// requeues the chain. Admitted ahead of every fresh spawn.
    resuming: VecDeque<ChainIndex>,
    /// Freshly spawned task chains waiting for admission, in spawn order;
    /// admitted after every resuming chain. A queued spawn holds no Lua VM
    /// yet: admission is what starts it.
    spawned: VecDeque<ChainIndex>,
    /// The effect table: every issued effect the caller has not answered
    /// yet, and the step's outgoing queue of issued and withdrawn effects.
    effects: EffectTable,
    /// The task arena: one entry per task the run has started, keyed by the
    /// task's id: its backing chain's id, or, for an internal timer, the id
    /// it took from its owner's child counter. An entry outlives its chain:
    /// it holds the terminal state and the undelivered outcome at least
    /// until the owner takes the result or ends, and in fact for the run -
    /// the arena is append-only like the chain arena, so `status` can report
    /// a terminal state at any later time. Ids order as paths, and one
    /// owner's tasks are its direct children, so one owner's entries iterate
    /// in spawn order.
    tasks: BTreeMap<TaskId, TaskEntry>,
    /// Where the step loop stands: not started, running, ending with the
    /// outcome held until the orphans are answered, or done.
    phase: Phase,
    /// The run's scope, taken where the run acquires its root identity:
    /// closed when the run reaches `Done` or is dropped before it, so a
    /// store view the caller still holds never outlives the run.
    scope: Option<ScopeHandle>,
}

impl Drop for Scheduler {
    fn drop(&mut self) {
        self.end_scope();
    }
}

impl Scheduler {
    /// Builds the scheduler for one run over `ctx`'s prompt and reports
    /// the run's start, so the first step's events open with it.
    pub(super) fn new(ctx: RunState) -> Self {
        // The run's boundaries are events like every other report: pushed
        // into the buffer under the root task, so the caller sees them in
        // order with the sections between them.
        ctx.emitter()
            .report(ctx.prompt().title(), lifecycle::RUN_STARTED);
        Self {
            ctx,
            chains: ChainArena::new(),
            ready: VecDeque::new(),
            resuming: VecDeque::new(),
            spawned: VecDeque::new(),
            effects: EffectTable::default(),
            tasks: BTreeMap::new(),
            phase: Phase::Fresh,
            scope: None,
        }
    }

    /// Ends the run's scope, once: every access still held in it refuses
    /// its next operation, and its claims stop conflicting.
    fn end_scope(&mut self) {
        if let Some(scope) = self.scope.take() {
            runvil_vfs::detail::end_scope(&scope);
        }
    }

    /// The prompt's shared handle: a chain resolves its walk position
    /// against this clone so the tree borrow never pins the arena.
    fn prompt(&self) -> Arc<Prompt> {
        Arc::clone(self.ctx.prompt())
    }

    /// Whether the run's outcome is decided: the end boundary is reported
    /// and only the orphans' answers stand between the run and `Done`.
    pub(super) fn decided(&self) -> bool {
        matches!(self.phase, Phase::Ending(_) | Phase::Done)
    }

    /// Resumes `id` at once with `answer`: the inline-answer path of every
    /// request the core keeps, and of a chain end answering another chain.
    ///
    /// # Errors
    /// Returns [`Error::Internal`] when the chain has no suspended block
    /// to take the answer.
    fn answer_inline(&mut self, id: ChainIndex, answer: Answer<Error>) -> Result<()> {
        self.chains[id].deliver(answer)?;
        self.ready.push_back(id);
        Ok(())
    }
}
