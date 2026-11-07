//! The chain record and the arena that holds it. A record is one chain's
//! identity, kind, walk stack, entered section, task bookkeeping, and
//! walk-scoped slots. The [`ChainArena`] holds one per chain the run has
//! started, indexed by [`ChainIndex`].

use std::fmt;
use std::num::{NonZeroU32, NonZeroUsize};
use std::ops::{Index, IndexMut};
use std::sync::{Arc, Mutex};

use mlua::Thread;
use runvil_types::ids::{ChainId, TaskId};
use runvil_types::metrics::Usage;
use runvil_vfs::Access;

use crate::execute::context::ChainState;
use crate::execute::section_context::{SectionContext, TaskSeed};
use crate::execute::walk_target::JumpTarget;
use crate::heading_address::SectionPos;
use crate::lua::{Answer, Blocked, UsageAnchor};
use crate::model::{Message, ModelId};
use crate::prompt::{Block, BodyText, Prompt, Section};
use crate::{Result, error};

/// A chain's local id counters: the indices its next child chain and its
/// next section entry take under its lineage. Set at chain start and
/// advanced as the chain hands out ids: zero for a fresh chain
/// (`Default`), or the values the chain continues from when it takes over
/// an earlier chain's identity (the walk after the H1 pass).
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Counters {
    /// The next child index under the chain's lineage, which `call`
    /// children, spawned tasks, and the wait shims' timers share.
    pub(super) next_child: u32,
    /// The next index a section entry takes under the chain's lineage as
    /// its `sys.id`.
    pub(super) next_entry: u32,
}

/// What kind of chain a record is: the main walk, the H1 pass, a `call`
/// child, or a task's backing chain, each with the links only that kind
/// has.
#[derive(Debug)]
pub(in crate::execute::scheduler) enum ChainKind {
    /// The main walk.
    Root,
    /// The H1 pass: the prompt's H1 blocks under its title. It runs the
    /// walk's rules with the deltas the `h1` module lists. Its `walk`
    /// stays at the first top-level section, unused: the walk after the
    /// pass is a separate root chain, started at the first section or at
    /// the target a jump out of H1 resolves to.
    H1,
    /// A `call` child, whose parent is blocked on it.
    Call {
        /// The call parent blocked on this chain.
        parent: ChainIndex,
    },
    /// A spawned task's backing chain, a fanout arm included. Its owner is
    /// recorded on its task entry.
    Task,
}

impl ChainKind {
    /// Whether the chain is the H1 pass.
    pub(in crate::execute::scheduler) fn is_h1(&self) -> bool {
        matches!(self, ChainKind::H1)
    }

    /// The call parent blocked on the chain, when it is a `call` child.
    pub(in crate::execute::scheduler) fn parent(&self) -> Option<ChainIndex> {
        match self {
            ChainKind::Call { parent } => Some(*parent),
            ChainKind::Root | ChainKind::H1 | ChainKind::Task => None,
        }
    }
}

/// A block's position in the section, or the H1 pass, a chain runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct BlockIndex(usize);

impl BlockIndex {
    /// The section's first block.
    pub(super) const FIRST: BlockIndex = BlockIndex(0);

    /// The block after this one.
    pub(super) fn next(self) -> BlockIndex {
        BlockIndex(self.0 + 1)
    }

    /// The line the last Lua block in `blocks` at or before this one
    /// starts on, which a failure of the whole block is placed on: the
    /// section's last Lua block once this index has moved past its end.
    /// `None` when no Lua block comes at or before it.
    pub(super) fn lua_line(self, blocks: &[Block]) -> Option<NonZeroU32> {
        let last = self.0.min(blocks.len().checked_sub(1)?);
        blocks[..=last].iter().rev().find_map(|block| match block {
            Block::Lua(program) => Some(program.source_line()),
            _ => None,
        })
    }
}

/// A Lua block that is running or suspended: its coroutine and the answer
/// delivered for it.
#[derive(Debug)]
pub(in crate::execute::scheduler) struct Running {
    /// The block's coroutine handle; a block that returns disposes of it.
    pub(in crate::execute::scheduler) coroutine: Thread,
    /// The answer delivered for the suspended coroutine, taken at resume.
    pub(in crate::execute::scheduler) answer: Option<Result<Answer>>,
}

/// The section a chain has entered: built at each section entry, the H1
/// pass's included, and taken at the section's end, at chain end, and at
/// an abort. The frame's drop is the teardown boundary on every path that
/// takes the `Entered`, and it reports `SECTION_FINISHED` only where the
/// section completed: `end_section`, `apply_jump`, and `finish` arm the
/// frame first, while the H1 pass's fall-through and scalar return, an
/// abort, and the run's teardown take it unarmed and are deliberately
/// silent.
pub(in crate::execute::scheduler) struct Entered {
    /// The section's running or suspended Lua block, `None` between
    /// blocks. Declared first: a suspended coroutine drops before the
    /// frame it runs in.
    pub(in crate::execute::scheduler) running: Option<Running>,
    /// The per-section frame (VM, `sys`, conversation, counts).
    pub(in crate::execute::scheduler) frame: SectionContext,
    /// The section's in-flight or next Lua/prose block: the running
    /// block's index while `running` is `Some`, otherwise the next block
    /// to start.
    pub(in crate::execute::scheduler) block: BlockIndex,
    /// The pending Markdown buffer: the prose block the next Lua fence
    /// consumes, beside the prompt line it starts on, installed as that
    /// block's lazy `prose` template when the coroutine starts. An
    /// unconsumed buffer drops with the section, never evaluated.
    pub(in crate::execute::scheduler) pending_prose: Option<(BodyText, NonZeroU32)>,
}

impl Entered {
    /// A fresh entry into `frame`'s section: no block running, the first
    /// block next, and no pending prose.
    pub(in crate::execute::scheduler) fn new(frame: SectionContext) -> Entered {
        Entered {
            running: None,
            frame,
            block: BlockIndex::FIRST,
            pending_prose: None,
        }
    }

    /// The block at the section's block position in `blocks`, or `None`
    /// once they are exhausted.
    pub(in crate::execute::scheduler) fn block_in<'p>(
        &self,
        blocks: &'p [Block],
    ) -> Option<&'p Block> {
        blocks.get(self.block.0)
    }
}

/// The deepest nesting a chain may run at, counting `call` children and
/// spawned tasks, fanout arms included; the first level counts.
const MAX_CALL_DEPTH: usize = 8;

/// A chain's call nesting depth: each `call` child and each spawned task
/// runs one level deeper than the chain that started it, so the cap holds
/// across a spawn boundary as well as a call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CallDepth(usize);

impl CallDepth {
    /// The depth of the root walk and the H1 pass.
    pub(super) const ROOT: CallDepth = CallDepth(0);

    /// The depth a chain started one level deeper runs at.
    ///
    /// # Errors
    /// Returns a `CallDepthExceeded` failure when that depth passes the cap.
    pub(super) fn deeper(self) -> Result<CallDepth> {
        let depth = self.0 + 1;
        if depth > MAX_CALL_DEPTH {
            return Err(error::make_call_depth_exceeded_error(MAX_CALL_DEPTH));
        }
        Ok(CallDepth(depth))
    }

    /// The depth as a count of nesting levels.
    pub(super) fn get(self) -> usize {
        self.0
    }
}

/// What a suspended chain is parked on, from its dispatch until its
/// answer resumes it. A wait keeps its data until the wake: the
/// `join_any` set until a member's end answers it, the `tool_calls` slots
/// until the last one lands. From that answer to the resume, the chain
/// reports what it was parked on, unless the wake requeued it for a slot.
#[derive(Debug)]
pub(super) enum Parked {
    /// A `chat` or `infer` round.
    Chat,
    /// A lone tool call.
    ToolCall,
    /// The answers of a `tool_calls` batch, in call order: `None` while a
    /// call is in flight, its text or failure once it lands. Empty from
    /// dispatch until the arm issues the calls, and once the full batch
    /// has answered the chain.
    Batch(Vec<Option<Result<String>>>),
    /// A store operation.
    Store,
    /// The tasks of a `join_any` wait. Empty from dispatch until the arm
    /// parks, and once a member's end has answered the wait, so no other
    /// member wakes the chain again.
    Tasks(Vec<TaskId>),
    /// A `call` child.
    Call,
    /// A concurrency slot: a fresh spawn before admission, or a wake that
    /// takes back the slots its wait gave up.
    Queued,
}

impl Parked {
    /// What `tasks.status` reports the chain blocked on.
    fn blocked(&self) -> Blocked {
        match self {
            Parked::Chat => Blocked::Chat,
            Parked::ToolCall | Parked::Batch(_) => Blocked::ToolCall,
            Parked::Store => Blocked::Store,
            Parked::Tasks(_) => Blocked::Tasks,
            Parked::Call => Blocked::Call,
            Parked::Queued => Blocked::Queued,
        }
    }
}

/// A chain's walk: a stack of positions that is never empty, with the
/// current position on top. Each level below the top is a suspended parent
/// walk, held at the section whose jump descended from it.
#[derive(Debug)]
pub(super) struct Walk {
    /// The section the chain is running, or the next entry candidate while
    /// the chain is between sections.
    top: SectionPos,
    /// The jumpers of the suspended parent walks, outermost first.
    below: Vec<SectionPos>,
}

impl Walk {
    /// A walk at `start`, with no suspended parent.
    pub(super) fn new(start: SectionPos) -> Walk {
        Walk {
            top: start,
            below: Vec::new(),
        }
    }

    /// The current position.
    pub(super) fn current(&self) -> &SectionPos {
        &self.top
    }

    /// Falls through to the next sibling.
    pub(super) fn advance(&mut self) {
        self.top = self.top.next();
    }

    /// Moves to `target`: a descent suspends the current position below the
    /// target, and a sibling jump replaces it.
    pub(super) fn jump(&mut self, target: JumpTarget) {
        let jumper = std::mem::replace(&mut self.top, target.pos);
        if target.descends {
            self.below.push(jumper);
        }
    }

    /// Drops an exhausted child level and moves its parent past the
    /// jumper. Returns `false` at the root level, leaving the position
    /// unchanged.
    pub(super) fn pop(&mut self) -> bool {
        let Some(jumper) = self.below.pop() else {
            return false;
        };
        self.top = jumper.next();
        true
    }
}

/// What the chain's `chat` rounds taught the context precheck about token
/// counts: the provider's numbers for the newest round that reported usage.
///
/// A chain has one parked `chat` round at a time. Dispatch holds that
/// round's projected messages in `in_flight`, beside the id of the model
/// they went to; the arrival of its answer settles them into `measured`
/// when the round reported usage, and drops them otherwise. A round with no
/// usage leaves the older measurement in place: it still describes a prefix
/// of the conversation, and the precheck uses it only while the request
/// extends that prefix on the same model.
///
/// The model is compared by [`ModelId`], the gateway server and model name
/// the binding resolved to, because that is what picks the tokenizer. The
/// alias is only a prompt-local label: two aliases of one model, with
/// other temperature or thinking settings, count tokens alike and share a
/// measurement.
#[derive(Debug, Default)]
pub(in crate::execute::scheduler) struct ChatAnchor {
    in_flight: Option<(ModelId, Vec<Message>)>,
    measured: Option<(ModelId, UsageAnchor)>,
}

impl ChatAnchor {
    /// The newest measurement, for the precheck of the next dispatch to
    /// `model`. A measurement from another model is not offered: its token
    /// counts come from another tokenizer.
    pub(in crate::execute::scheduler) fn measured(&self, model: &ModelId) -> Option<&UsageAnchor> {
        self.measured
            .as_ref()
            .filter(|(measured_by, _)| measured_by == model)
            .map(|(_, anchor)| anchor)
    }

    /// Records the projected messages of a round about to be sent to
    /// `model`.
    pub(in crate::execute::scheduler) fn sending(
        &mut self,
        model: ModelId,
        messages: Vec<Message>,
    ) {
        self.in_flight = Some((model, messages));
    }

    /// Settles the parked round: its model, sent messages, and `usage`
    /// replace the measurement when it reported usage; a failed round, or
    /// one with no usage, only releases the sent messages.
    pub(in crate::execute::scheduler) fn settle(&mut self, usage: Option<&Usage>) {
        let sent = self.in_flight.take();
        if let (Some((model, messages)), Some(usage)) = (sent, usage) {
            self.measured = Some((model, UsageAnchor::new(messages, usage)));
        }
    }
}

/// One chain: a contained line of section execution.
///
/// The chain owns its entered section (the frame, the block position, the
/// pending Markdown buffer, and the in-flight Lua block) and adds the
/// chain's [`Walk`] and the walk's `var` table. One section entry is one
/// [`Entered`]; the fall-through advance tears the old one down and the
/// next entry constructs the next.
pub(super) struct Chain {
    /// The chain's hierarchical [`ChainId`], allocated under the scheme
    /// [`runvil_types::ids`] states: every id the chain hands out extends
    /// it. The H1 pass and the walk after it are two arena records under
    /// the root lineage `0`.
    pub(super) lineage: ChainId,
    /// The chain's local child and entry counters under `lineage`. This is
    /// the normative `0.1` statement for the run: the root chain's entry 0
    /// is the H1 pass (section 0), consumed whether or not the prompt has
    /// H1 blocks, so the first walked section is always `0.1`. Every other
    /// site that names the first walked entry defers to this one.
    pub(super) counters: Counters,
    /// What kind of chain this is, with the links only that kind has.
    pub(super) kind: ChainKind,
    /// The nearest enclosing task: the chain's own id when the chain is a
    /// spawned task (a fanout arm included), its caller's task for a
    /// `call` child (a blocking child never interleaves with its caller,
    /// so the two share one task), and task `0` for the root chain. Every
    /// section the chain enters reads it as `sys.taskid`.
    pub(super) task: TaskId,
    /// A spawned chain's `item` and `sys.index` seeds, taken by its first
    /// section entry; empty afterward and on every other chain.
    pub(super) seed: TaskSeed,
    /// What the chain's suspended request is parked on: set at dispatch,
    /// at spawn, and when a wake requeues the chain for a slot; cleared
    /// when the chain is admitted or its answer resumes it. `None` while
    /// the chain runs or between blocks.
    pub(super) parked: Option<Parked>,
    /// The chain's fork of the run state: the run's own for the H1 pass and
    /// for a root walk with no pass before it, the hand-off fork holding
    /// H1's frozen `argv` (`with_walk_state`) for the walk after the pass,
    /// and the caller's for a `call` child or the spawner's for a task,
    /// through `with_args` when the call or spawn gave an input. A task's
    /// fork also reports under its own task id with its own turn counter
    /// (`with_task`).
    pub(super) ctx: ChainState,
    /// The chain's current model, shared with each section VM it enters:
    /// it starts on the chain's starting model; `models.use` rewrites it.
    pub(super) selection: Arc<Mutex<Option<crate::model::ModelBinding>>>,
    /// The chain's VFS access capability, installed into each section VM
    /// the chain enters: the walk and the H1 pass share one root
    /// identity, a call chain borrows its parent's (a blocking child is
    /// the same serial thread - no new identity, no false conflicts), and
    /// a task chain forks its own from its spawner's, joined back into
    /// the spawner at every delivery and at chain end. Installed at chain
    /// start and `None` only after the chain ends: the arena is
    /// append-only, so `finish` and `abort_subtree` take the slot to drop
    /// the identity's last reference at chain end rather than at
    /// scheduler drop.
    pub(super) access: Option<Arc<Access>>,
    /// The section the chain has entered: `Some` while a section is
    /// entered, `None` before the first entry and between sections.
    pub(super) entered: Option<Entered>,
    /// The chain's walk through the prompt tree the run shares, each
    /// position named by path. The H1 pass's walk is never read.
    pub(super) walk: Walk,
    /// The name of the section the chain most recently entered: the name
    /// its reports carry on the walk, including between sections and after
    /// the walk runs off its slice, where the walk names no running
    /// section. Before the first entry, the name of the section at the
    /// chain's start, or the prompt's title when the start is past its
    /// slice. While a section is entered, this and the walk position name
    /// the same section; this field is the only record between sections and
    /// after teardown, and it is written only after the frame is
    /// constructed.
    pub(super) entered_name: String,
    /// The walk's `var` table: seeds each section's VM at entry; the
    /// section's final `var` is read back before teardown and replaces the
    /// slot. A call chain's slot seeds from the caller's snapshot and
    /// is discarded with the chain, so the caller never sees the chain's
    /// writes.
    pub(super) var: serde_json::Value,
    /// The chain's call nesting depth, which the recursion cap checks.
    pub(super) call_depth: CallDepth,
    /// The chain's effective admission limit: the most tasks this chain
    /// may have admitted at once. The root's is the run's ceiling
    /// ([`RunLimits::max_concurrency`](crate::execute::RunLimits::max_concurrency));
    /// a spawned task and a call chain start with their parent's, and
    /// `tasks.concurrency` sets it, clamped to the parent's.
    pub(super) concurrency: NonZeroUsize,
    /// The provider's token counts for the chain's `chat` rounds, which the
    /// next round's context precheck counts from.
    pub(super) anchor: ChatAnchor,
}

impl fmt::Debug for Chain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Chain")
            .field("lineage", &self.lineage)
            .field("kind", &self.kind)
            .field("task", &self.task)
            .field("walk", &self.walk)
            .field("entered_name", &self.entered_name)
            .field("block", &self.entered.as_ref().map(|entered| entered.block))
            .field("parked", &self.parked)
            .field("call_depth", &self.call_depth)
            .field("concurrency", &self.concurrency)
            .finish_non_exhaustive()
    }
}

impl Chain {
    /// The chain's access capability for section-VM installation. A live
    /// chain always holds one; `finish` and `abort_subtree` take it at
    /// chain end.
    ///
    /// # Errors
    /// Returns an `Internal` failure when the chain's capability is gone,
    /// which only the chain-end paths do - a live chain always holds it.
    /// The two teardown probes skip rather than fail on a gone capability:
    /// `join_task` returns without a join, and `end_tool_call` ends the
    /// call's identity with no owner to merge it into.
    pub(super) fn access(&self) -> Result<&Arc<Access>> {
        self.access.as_ref().ok_or(error::make_internal_error(
            "a live chain holds its access capability",
        ))
    }

    /// Hands out the chain's next child id, under [`Counters::next_child`].
    ///
    /// # Errors
    /// Returns an `Internal` failure when one chain has started `u32::MAX`
    /// children, which no reachable run does.
    pub(super) fn next_child_id(&mut self) -> Result<ChainId> {
        let index = self.counters.next_child;
        self.counters.next_child = index.checked_add(1).ok_or(error::make_internal_error(
            "a chain's child count cannot exceed u32",
        ))?;
        Ok(self.lineage.child(index))
    }

    /// Hands out the chain's next section-entry id, under
    /// [`Counters::next_entry`].
    ///
    /// # Errors
    /// Returns an `Internal` failure when one chain has entered `u32::MAX`
    /// sections, which no reachable run does.
    pub(super) fn next_entry_id(&mut self) -> Result<String> {
        let index = self.counters.next_entry;
        self.counters.next_entry = index.checked_add(1).ok_or(error::make_internal_error(
            "a chain's entry count cannot exceed u32",
        ))?;
        Ok(self.lineage.entry(index))
    }

    /// What `tasks.status` reports the chain blocked on, or `None` while
    /// it runs or between blocks.
    pub(super) fn blocked(&self) -> Option<Blocked> {
        self.parked.as_ref().map(Parked::blocked)
    }

    /// Ends the chain's `join_any` wait when its set holds `task`, and
    /// reports whether it did. The set empties, so no other member's end
    /// wakes the chain again.
    pub(super) fn end_wait_for(&mut self, task: &TaskId) -> bool {
        match &mut self.parked {
            Some(Parked::Tasks(set)) if set.contains(task) => {
                set.clear();
                true
            }
            _ => false,
        }
    }

    /// Hands `answer` to the chain's suspended block, to be taken when
    /// the block resumes.
    ///
    /// # Errors
    /// Returns an `Internal` failure when the chain has no suspended block, or
    /// when its block already holds an answer: every answer goes to a block
    /// that yielded for it, once, and the first answer stays.
    #[track_caller]
    pub(super) fn deliver(&mut self, answer: Result<Answer>) -> Result<()> {
        let Some(running) = self
            .entered
            .as_mut()
            .and_then(|entered| entered.running.as_mut())
        else {
            return Err(error::make_internal_error(
                "a delivered answer implies a suspended block",
            ));
        };
        if running.answer.is_some() {
            return Err(error::make_internal_error(
                "a suspended block holds at most one delivered answer",
            ));
        }
        running.answer = Some(answer);
        Ok(())
    }

    /// The frame of the section the chain is running, for a path that
    /// reaches the chain by its arena index.
    ///
    /// # Errors
    /// Returns an `Internal` failure when the chain is between sections,
    /// which no caller that reached the chain through a suspended or
    /// running block can see.
    #[track_caller]
    pub(super) fn frame(&self) -> Result<&SectionContext> {
        self.entered
            .as_ref()
            .map(|entered| &entered.frame)
            .ok_or(error::make_internal_error("a live chain holds its frame"))
    }

    /// The frame of the section the chain is running, mutably.
    ///
    /// # Errors
    /// As [`frame`](Self::frame).
    #[track_caller]
    pub(super) fn frame_mut(&mut self) -> Result<&mut SectionContext> {
        self.entered
            .as_mut()
            .map(|entered| &mut entered.frame)
            .ok_or(error::make_internal_error("a live chain holds its frame"))
    }

    /// The chain's current block sequence: the H1 pass's blocks, or the
    /// current section's blocks on the walk, empty while the walk is past
    /// its slice. Resolved against the caller's handle on the tree so the
    /// result outlives a mutable borrow of the chain.
    pub(super) fn blocks<'p>(&self, prompt: &'p Prompt) -> &'p [Block] {
        if self.kind.is_h1() {
            crate::prompt::detail::h1_blocks(prompt)
        } else {
            self.walk
                .current()
                .section(prompt)
                .map_or(&[], Section::blocks)
        }
    }

    /// The chain's current section name for observations and errors: the
    /// prompt's title for the H1 pass, else the name of the section
    /// the chain most recently entered - so a chain between sections, or
    /// past its slice's last section, reports the section it just left.
    pub(super) fn section_name(&self) -> &str {
        if self.kind.is_h1() {
            self.ctx.prompt().title()
        } else {
            &self.entered_name
        }
    }
}

#[cfg(test)]
impl Chain {
    /// A chain of `kind` under `lineage` that admits at most `concurrency`
    /// tasks at once, holds no access, and has entered no section, for a
    /// test that fills an arena without a run.
    pub(in crate::execute::scheduler) fn for_test(
        lineage: runvil_types::ids::ChainId,
        kind: ChainKind,
        concurrency: std::num::NonZeroUsize,
    ) -> Chain {
        use crate::execute::RunParams;
        use crate::heading_address::SectionIndex;
        use crate::test_support::scenario::{TEST_SEED, TEST_STARTED_AT};

        let source = concat!(
            "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n",
            "# Title\n\n## Only\n\ndone\n",
        );
        let prompt = Prompt::parse(source, "prompt", "chain-test")
            .0
            .expect("the test prompt parses");
        let ctx = ChainState::root(
            Arc::new(prompt),
            "",
            RunParams::new("chain-test", TEST_SEED, TEST_STARTED_AT),
        );
        Chain {
            task: TaskId::from(lineage.clone()),
            lineage,
            counters: Counters::default(),
            kind,
            seed: TaskSeed::default(),
            parked: None,
            ctx,
            selection: Arc::new(Mutex::new(None)),
            access: None,
            entered: None,
            walk: Walk::new(SectionPos::top(SectionIndex::FIRST)),
            entered_name: "Only".to_owned(),
            var: serde_json::json!({}),
            call_depth: CallDepth::ROOT,
            concurrency,
            anchor: ChatAnchor::default(),
        }
    }
}

/// The most chains one run may start: the arena indexes chains by `u32`,
/// so the count is bounded by the index space.
const MAX_CHAINS: usize = u32::MAX as usize;

/// Arena index of a chain: indices, not references, so no chain ever holds
/// a pointer to another. Only [`ChainArena::push`] creates one outside test
/// builds, so every index names a chain; a `#[cfg(test)]` constructor mints
/// one for tables that only compare indices. The index is the scheduler's
/// private handle; the chain's identity for authors and callers is its
/// hierarchical [`ChainId`], which never depends
/// on arena order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in crate::execute::scheduler) struct ChainIndex(u32);

impl ChainIndex {
    /// The arena index as a `usize`.
    pub(in crate::execute::scheduler) fn index(self) -> usize {
        self.0 as usize
    }

    /// An index no arena handed out, for a test of a table that only
    /// compares indices.
    #[cfg(test)]
    pub(in crate::execute::scheduler) fn for_test(raw: u32) -> ChainIndex {
        ChainIndex(raw)
    }
}

/// The chain arena: every chain the run has started, indexed by
/// [`ChainIndex`]. Every index the arena hands out names a chain: it holds
/// at most [`MAX_CHAINS`] and never shrinks. A test build lowers that bound
/// through a `#[cfg(test)]` setter, so the overflow path runs without
/// allocating the real bound.
pub(in crate::execute::scheduler) struct ChainArena {
    chains: Vec<Chain>,
    /// The chain-count bound in test builds: [`MAX_CHAINS`] until a test
    /// shrinks it to drive the overflow path without allocating the real
    /// one.
    #[cfg(test)]
    max_chains: usize,
}

impl ChainArena {
    /// An empty arena.
    pub(in crate::execute::scheduler) fn new() -> ChainArena {
        ChainArena {
            chains: Vec::new(),
            #[cfg(test)]
            max_chains: MAX_CHAINS,
        }
    }

    /// Adds `chain` and returns the index that names it.
    ///
    /// # Errors
    /// Returns an `Internal` failure when the arena already holds its
    /// bound, leaving the arena unchanged.
    pub(in crate::execute::scheduler) fn push(&mut self, chain: Chain) -> Result<ChainIndex> {
        #[cfg(test)]
        let max_chains = self.max_chains;
        #[cfg(not(test))]
        let max_chains = MAX_CHAINS;
        if self.chains.len() >= max_chains {
            return Err(error::make_internal_error(
                "a run's chain count cannot exceed u32",
            ));
        }
        let id = ChainIndex(u32::try_from(self.chains.len()).map_err(|cause| {
            error::make_internal_with_cause_error("a run's chain count cannot exceed u32", &cause)
        })?);
        self.chains.push(chain);
        Ok(id)
    }

    /// Every chain with its index, in the order the chains were pushed.
    pub(in crate::execute::scheduler) fn iter(&self) -> impl Iterator<Item = (ChainIndex, &Chain)> {
        // The bound holds the count to `u32`, so the index range covers
        // every chain.
        (0..).map(ChainIndex).zip(&self.chains)
    }

    /// Every chain, mutably, in the order the chains were pushed.
    pub(in crate::execute::scheduler) fn iter_mut(&mut self) -> impl Iterator<Item = &mut Chain> {
        self.chains.iter_mut()
    }

    /// How many chains the run has started.
    pub(in crate::execute::scheduler) fn len(&self) -> usize {
        self.chains.len()
    }

    /// The `call` children of `parent`, in the order they started.
    pub(in crate::execute::scheduler) fn call_children(
        &self,
        parent: ChainIndex,
    ) -> impl Iterator<Item = ChainIndex> {
        self.iter()
            .filter(move |(_, chain)| chain.kind.parent() == Some(parent))
            .map(|(child, _)| child)
    }

    /// Shrinks the chain-count bound so a test can drive the overflow path.
    #[cfg(test)]
    pub(in crate::execute::scheduler) fn set_max_for_test(&mut self, limit: usize) {
        self.max_chains = limit;
    }
}

impl Index<ChainIndex> for ChainArena {
    type Output = Chain;

    fn index(&self, id: ChainIndex) -> &Chain {
        &self.chains[id.index()]
    }
}

impl IndexMut<ChainIndex> for ChainArena {
    fn index_mut(&mut self, id: ChainIndex) -> &mut Chain {
        &mut self.chains[id.index()]
    }
}

#[cfg(test)]
mod tests {
    //! Tests for the chain's usage-anchor bookkeeping (what a settled round
    //! keeps for the next precheck, what a failed or usage-less round leaves
    //! alone, and which model a measurement is offered to), for its walk
    //! stack and its id counters, and for the arena that holds the chains,
    //! one rule each.

    use std::num::NonZeroUsize;

    use runvil_types::ids::ChainId;
    use runvil_types::metrics::Usage;

    use super::{Chain, ChainArena, ChainIndex, ChainKind, ChatAnchor, Walk};
    use crate::execute::walk_target::JumpTarget;
    use crate::heading_address::{SectionIndex, SectionPos};
    use crate::lua::UsageAnchor;
    use crate::model::{Message, ModelId};

    fn usage(prompt: u32, completion: u32) -> Usage {
        Usage::new(prompt, completion, prompt + completion)
    }

    fn request(text: &str) -> Vec<Message> {
        vec![Message::user(text)]
    }

    fn model(name: &str) -> ModelId {
        ModelId::gateway(name).expect("the test model name is valid")
    }

    #[test]
    fn a_chain_starts_with_no_measurement() {
        assert!(ChatAnchor::default().measured(&model("a")).is_none());
    }

    #[test]
    fn a_round_that_reported_usage_becomes_the_measurement() {
        let mut anchor = ChatAnchor::default();
        anchor.sending(model("a"), request("first"));
        anchor.settle(Some(&usage(100, 20)));
        let measured = anchor
            .measured(&model("a"))
            .expect("the round reported usage");
        assert_eq!(measured.tokens(), 120);
    }

    #[test]
    fn the_newest_round_that_reported_usage_replaces_the_measurement() {
        let mut anchor = ChatAnchor::default();
        anchor.sending(model("a"), request("first"));
        anchor.settle(Some(&usage(100, 20)));
        anchor.sending(model("a"), request("second"));
        anchor.settle(Some(&usage(300, 40)));
        assert_eq!(
            anchor.measured(&model("a")).map(UsageAnchor::tokens),
            Some(340)
        );
    }

    #[test]
    fn a_round_with_no_usage_keeps_the_older_measurement() {
        let mut anchor = ChatAnchor::default();
        anchor.sending(model("a"), request("first"));
        anchor.settle(Some(&usage(100, 20)));
        anchor.sending(model("a"), request("second"));
        anchor.settle(None);
        assert_eq!(
            anchor.measured(&model("a")).map(UsageAnchor::tokens),
            Some(120),
            "a round that measured nothing leaves the older measurement"
        );
    }

    #[test]
    fn usage_with_no_round_in_flight_measures_nothing() {
        let mut anchor = ChatAnchor::default();
        anchor.sending(model("a"), request("first"));
        anchor.settle(None);
        // The failed round released its messages, so a late usage has nothing
        // to attach to.
        anchor.settle(Some(&usage(100, 20)));
        assert!(anchor.measured(&model("a")).is_none());
    }

    #[test]
    fn the_measurement_is_offered_only_to_the_model_that_made_it() {
        let mut anchor = ChatAnchor::default();
        anchor.sending(model("a"), request("first"));
        anchor.settle(Some(&usage(100, 20)));
        assert_eq!(
            anchor.measured(&model("a")).map(UsageAnchor::tokens),
            Some(120),
            "an id built again from the same parts is the same model"
        );
        assert!(
            anchor.measured(&model("b")).is_none(),
            "another model's tokenizer counted these tokens"
        );
        let other_server = ModelId::new("elsewhere", "a").expect("the test id is valid");
        assert!(
            anchor.measured(&other_server).is_none(),
            "the same name on another server is another model"
        );
    }

    #[test]
    fn a_round_on_another_model_replaces_the_measurement() {
        let mut anchor = ChatAnchor::default();
        anchor.sending(model("a"), request("first"));
        anchor.settle(Some(&usage(100, 20)));
        anchor.sending(model("b"), request("second"));
        anchor.settle(Some(&usage(300, 40)));
        assert_eq!(
            anchor.measured(&model("b")).map(UsageAnchor::tokens),
            Some(340)
        );
        assert!(
            anchor.measured(&model("a")).is_none(),
            "the old model's measurement is gone once the new one settles"
        );
    }

    #[test]
    fn a_round_on_another_model_with_no_usage_keeps_the_older_measurement() {
        let mut anchor = ChatAnchor::default();
        anchor.sending(model("a"), request("first"));
        anchor.settle(Some(&usage(100, 20)));
        anchor.sending(model("b"), request("second"));
        anchor.settle(None);
        assert_eq!(
            anchor.measured(&model("a")).map(UsageAnchor::tokens),
            Some(120),
            "a round that measured nothing leaves the older measurement"
        );
        assert!(anchor.measured(&model("b")).is_none());
    }

    fn top(index: usize) -> SectionPos {
        SectionPos::top(SectionIndex::new(index))
    }

    fn descend(jumper: &SectionPos, index: usize) -> JumpTarget {
        JumpTarget {
            pos: jumper.child(SectionIndex::new(index)),
            descends: true,
        }
    }

    #[test]
    fn a_new_walk_is_at_its_start() {
        assert_eq!(Walk::new(top(2)).current(), &top(2));
    }

    #[test]
    fn advancing_a_walk_moves_to_the_next_sibling_at_the_same_level() {
        let start = top(1).child(SectionIndex::FIRST);
        let mut walk = Walk::new(start.clone());
        walk.advance();
        assert_eq!(walk.current(), &start.next());
        assert_eq!(walk.current(), &top(1).child(SectionIndex::new(1)));
    }

    #[test]
    fn a_walk_descends_on_a_child_jump_and_a_pop_resumes_after_the_jumper() {
        let mut walk = Walk::new(top(1));
        walk.jump(descend(&top(1), 0));
        assert_eq!(walk.current(), &top(1).child(SectionIndex::FIRST));
        assert!(walk.pop(), "a child level pops back to its parent");
        assert_eq!(walk.current(), &top(2));
    }

    #[test]
    fn a_sibling_jump_replaces_the_walks_top_without_adding_a_level() {
        let mut walk = Walk::new(top(0));
        walk.jump(descend(&top(0), 0));
        let sibling = top(0).child(SectionIndex::new(2));
        walk.jump(JumpTarget {
            pos: sibling.clone(),
            descends: false,
        });
        assert_eq!(walk.current(), &sibling);
        assert!(walk.pop(), "the child level is still one level down");
        assert_eq!(walk.current(), &top(1));
        assert!(!walk.pop(), "the sibling jump added no level");
    }

    #[test]
    fn popping_a_walk_at_the_root_level_returns_false_and_keeps_the_position() {
        let mut walk = Walk::new(top(3));
        assert!(!walk.pop());
        assert_eq!(walk.current(), &top(3));
    }

    #[test]
    fn a_walk_unwinds_nested_descents_one_level_per_pop() {
        let mut walk = Walk::new(top(1));
        walk.jump(descend(&top(1), 0));
        walk.advance();
        let jumper = top(1).child(SectionIndex::new(1));
        walk.jump(descend(&jumper, 2));
        assert_eq!(walk.current(), &jumper.child(SectionIndex::new(2)));
        assert!(walk.pop());
        assert_eq!(walk.current(), &jumper.next());
        assert!(walk.pop());
        assert_eq!(walk.current(), &top(2));
        assert!(!walk.pop());
        assert_eq!(walk.current(), &top(2));
    }

    /// A chain of `kind` under `lineage` that admits one task at a time.
    fn chain(lineage: ChainId, kind: ChainKind) -> Chain {
        Chain::for_test(lineage, kind, NonZeroUsize::MIN)
    }

    fn call_of(parent: ChainIndex) -> ChainKind {
        ChainKind::Call { parent }
    }

    #[test]
    fn a_chain_hands_out_child_and_entry_ids_from_separate_counters() {
        let lineage = ChainId::root().child(4);
        let mut chain = chain(lineage.clone(), ChainKind::Root);
        assert_eq!(
            chain.next_child_id().expect("a child id is left"),
            lineage.child(0)
        );
        assert_eq!(
            chain.next_entry_id().expect("an entry id is left"),
            lineage.entry(0)
        );
        assert_eq!(
            chain.next_child_id().expect("a child id is left"),
            lineage.child(1)
        );
        assert_eq!(
            chain.next_entry_id().expect("an entry id is left"),
            lineage.entry(1)
        );
    }

    #[test]
    fn each_index_a_push_hands_out_names_the_chain_it_pushed() {
        let mut arena = ChainArena::new();
        let ids: Vec<ChainIndex> = (0..3)
            .map(|n| {
                arena
                    .push(chain(ChainId::root().child(n), ChainKind::Root))
                    .expect("the arena has room")
            })
            .collect();
        assert_eq!(arena.len(), 3);
        for (n, id) in (0..).zip(&ids) {
            assert_eq!(arena[*id].lineage, ChainId::root().child(n));
        }
        let listed: Vec<ChainIndex> = arena.iter().map(|(id, _)| id).collect();
        assert_eq!(listed, ids, "the arena lists its chains in push order");
    }

    #[test]
    fn a_push_at_the_bound_fails_without_adding_a_chain() {
        let mut arena = ChainArena::new();
        arena.set_max_for_test(1);
        let first = arena
            .push(chain(ChainId::root(), ChainKind::Root))
            .expect("the first chain fits");
        assert!(
            arena
                .push(chain(ChainId::root().child(0), ChainKind::Root))
                .is_err()
        );
        assert_eq!(arena.len(), 1);
        assert_eq!(arena[first].lineage, ChainId::root());
    }

    #[test]
    fn call_children_lists_only_the_parents_own_call_children() {
        let mut arena = ChainArena::new();
        let mut push = |lineage: ChainId, kind: ChainKind| {
            arena
                .push(chain(lineage, kind))
                .expect("the arena has room")
        };
        let root = ChainId::root();
        let walk = push(root.clone(), ChainKind::Root);
        let first = push(root.child(0), call_of(walk));
        push(root.child(1), ChainKind::Task);
        let grandchild = push(root.child(0).child(0), call_of(first));
        let second = push(root.child(2), call_of(walk));
        let children: Vec<ChainIndex> = arena.call_children(walk).collect();
        assert_eq!(
            children,
            [first, second],
            "a task and a grandchild are not listed"
        );
        let children: Vec<ChainIndex> = arena.call_children(first).collect();
        assert_eq!(children, [grandchild]);
    }
}
