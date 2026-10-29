//! The chain record and the arena that holds it. A record is one chain's
//! identity, kind, walk stack, entered section, task bookkeeping, and
//! walk-scoped slots. The [`ChainArena`] holds one per chain the run has
//! started, indexed by [`ChainIndex`]; the H1 pass and the walk after it
//! share the root lineage `0` and are two records.

use std::fmt;
use std::num::NonZeroUsize;
use std::sync::Arc;

use mlua::Thread;
use runvil_types::ids::{ChainId, TaskId};
use runvil_vfs::Access;

use crate::error::DepthVia;
use crate::execute::context::RunState;
use crate::execute::section_context::{SectionContext, TaskSeed};
use crate::execute::walk_target::JumpTarget;
use crate::heading_address::SectionPos;
use crate::lua::{Answer, Blocked};
use crate::parser::{Block, BodyText, Prompt, Section};
use crate::{Error, Result};

use super::MAX_CALL_DEPTH;

mod anchor;
mod arena;
mod kind;

pub(super) use anchor::ChatAnchor;
pub(super) use arena::{ChainArena, ChainIndex};
pub(super) use kind::ChainKind;

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
}

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
    /// Returns [`Error::CallDepthExceeded`], named after `via`, when that
    /// depth passes the cap.
    pub(super) fn deeper(self, via: DepthVia) -> Result<CallDepth> {
        let depth = self.0 + 1;
        if depth > MAX_CALL_DEPTH {
            return Err(Error::CallDepthExceeded {
                cap: MAX_CALL_DEPTH,
                via,
            });
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

/// A Lua block that is running or suspended: its coroutine and the answer
/// delivered for it.
#[derive(Debug)]
pub(super) struct Running {
    /// The block's coroutine handle; a block that returns disposes of it.
    pub(super) coroutine: Thread,
    /// The answer delivered for the suspended coroutine, taken at resume.
    pub(super) answer: Option<Result<Answer<Error>>>,
}

/// The section a chain has entered: built at each section entry, the H1
/// pass's included, and taken at the section's end, at chain end, and at
/// an abort. The frame's drop is the teardown boundary on every path that
/// takes the `Entered`, and it reports `SECTION_FINISHED` only where the
/// section completed: `end_section`, `apply_jump`, and `finish` arm the
/// frame first, while the H1 pass's fall-through and scalar return, an
/// abort, and the run's teardown take it unarmed and are deliberately
/// silent.
pub(super) struct Entered {
    /// The section's running or suspended Lua block, `None` between
    /// blocks. Declared first: a suspended coroutine drops before the
    /// frame it runs in.
    pub(super) running: Option<Running>,
    /// The per-section frame (VM, `sys`, conversation, counts).
    pub(super) frame: SectionContext,
    /// The section's in-flight or next Lua/prose block: the running
    /// block's index while `running` is `Some`, otherwise the next block
    /// to start.
    pub(super) block: BlockIndex,
    /// The pending Markdown buffer: the prose block the next Lua fence
    /// consumes, installed as that block's lazy `prose` template when the
    /// coroutine starts. An unconsumed buffer drops with the section,
    /// never evaluated.
    pub(super) pending_prose: Option<BodyText>,
}

impl Entered {
    /// A fresh entry into `frame`'s section: no block running, the first
    /// block next, and no pending prose.
    pub(super) fn new(frame: SectionContext) -> Entered {
        Entered {
            running: None,
            frame,
            block: BlockIndex::FIRST,
            pending_prose: None,
        }
    }

    /// The block at the section's block position in `blocks`, or `None`
    /// once they are exhausted.
    pub(super) fn block_in<'p>(&self, blocks: &'p [Block]) -> Option<&'p Block> {
        blocks.get(self.block.0)
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

/// One chain: a contained line of section execution.
///
/// The chain owns its entered section (the frame, the block position, the
/// pending Markdown buffer, and the in-flight Lua block) and adds the
/// chain's [`Walk`] and the walk's `var` table. One section entry is one
/// [`Entered`]; the fall-through advance tears the old one down and the
/// next entry constructs the next.
pub(super) struct Chain {
    /// The chain's hierarchical id: the parent chain's id extended by the
    /// parent's local child counter (the root lineage is `0`, which the H1
    /// pass and the walk that follows it share as two arena records).
    /// Every id the chain hands out - its children's, its section
    /// entries' - extends this path, so two runs of one prompt allocate
    /// identical ids however their chains interleave.
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
    pub(super) ctx: RunState,
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
    /// Returns [`Error::Internal`] when the chain's capability is gone,
    /// which only the chain-end paths do - a live chain always holds it.
    /// The two teardown probes skip rather than fail on a gone capability:
    /// `join_task` returns without a join, and `end_tool_call` ends the
    /// call's identity with no owner to merge it into.
    pub(super) fn access(&self) -> Result<&Arc<Access>> {
        self.access
            .as_ref()
            .ok_or(Error::internal("a live chain holds its access capability"))
    }

    /// Hands out the chain's next child id: its id extended by its local
    /// child counter, which `call` children, spawned tasks, and the wait
    /// shims' timers share, so the ids a chain hands out depend only on
    /// the order of its own dispatches.
    ///
    /// # Errors
    /// Returns [`Error::Internal`] when one chain has started `u32::MAX`
    /// children, which no reachable run does.
    pub(super) fn next_child_id(&mut self) -> Result<ChainId> {
        let index = self.counters.next_child;
        self.counters.next_child = index
            .checked_add(1)
            .ok_or(Error::internal("a chain's child count cannot exceed u32"))?;
        Ok(self.lineage.child(index))
    }

    /// Hands out the chain's next section-entry id: its id extended by its
    /// local entry counter, the value the entered section reads as
    /// `sys.id`.
    ///
    /// # Errors
    /// Returns [`Error::Internal`] when one chain has entered `u32::MAX`
    /// sections, which no reachable run does.
    pub(super) fn next_entry_id(&mut self) -> Result<String> {
        let index = self.counters.next_entry;
        self.counters.next_entry = index
            .checked_add(1)
            .ok_or(Error::internal("a chain's entry count cannot exceed u32"))?;
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
    /// Returns [`Error::Internal`] when the chain has no suspended block:
    /// every answer goes to a block that yielded for it.
    #[track_caller]
    pub(super) fn deliver(&mut self, answer: Result<Answer<Error>>) -> Result<()> {
        let Some(running) = self
            .entered
            .as_mut()
            .and_then(|entered| entered.running.as_mut())
        else {
            return Err(Error::internal(
                "a delivered answer implies a suspended block",
            ));
        };
        running.answer = Some(answer);
        Ok(())
    }

    /// The frame of the section the chain is running, for a path that
    /// reaches the chain by its arena index.
    ///
    /// # Errors
    /// Returns [`Error::Internal`] when the chain is between sections,
    /// which no caller that reached the chain through a suspended or
    /// running block can see.
    #[track_caller]
    pub(super) fn frame(&self) -> Result<&SectionContext> {
        self.entered
            .as_ref()
            .map(|entered| &entered.frame)
            .ok_or(Error::internal("a live chain holds its frame"))
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
            .ok_or(Error::internal("a live chain holds its frame"))
    }

    /// The chain's current block sequence: the H1 pass's blocks, or the
    /// current section's blocks on the walk, empty while the walk is past
    /// its slice. Resolved against the caller's handle on the tree so the
    /// result outlives a mutable borrow of the chain.
    pub(super) fn blocks<'p>(&self, prompt: &'p Prompt) -> &'p [Block] {
        if self.kind.is_h1() {
            crate::parser::detail::h1_blocks(prompt)
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
mod tests;
