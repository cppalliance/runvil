//! The chain record and the arena that holds it. A record is one chain's
//! identity, kind, walk stack, entered section, task bookkeeping, and
//! walk-scoped slots. The [`ChainArena`] holds one per chain the run has
//! started, indexed by [`ChainIndex`].

use std::fmt;
use std::num::{NonZeroU32, NonZeroUsize};
use std::sync::{Arc, Mutex};

use runvil_types::ids::{ChainId, TaskId};
use runvil_vfs::Access;

use crate::execute::context::ChainState;
use crate::execute::section_context::{SectionContext, TaskSeed};
use crate::execute::walk_target::JumpTarget;
use crate::heading_address::SectionPos;
use crate::lua::{Answer, Blocked};
use crate::prompt::{Block, Prompt, Section};
use crate::{Result, error};

mod anchor;
mod arena;
mod entered;
mod kind;

pub(super) use anchor::ChatAnchor;
pub(super) use arena::{ChainArena, ChainIndex};
pub(super) use entered::{Entered, Running};
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
            return Err(error::call_depth_exceeded(MAX_CALL_DEPTH));
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
        self.access
            .as_ref()
            .ok_or(error::internal("a live chain holds its access capability"))
    }

    /// Hands out the chain's next child id, under [`Counters::next_child`].
    ///
    /// # Errors
    /// Returns an `Internal` failure when one chain has started `u32::MAX`
    /// children, which no reachable run does.
    pub(super) fn next_child_id(&mut self) -> Result<ChainId> {
        let index = self.counters.next_child;
        self.counters.next_child = index
            .checked_add(1)
            .ok_or(error::internal("a chain's child count cannot exceed u32"))?;
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
        self.counters.next_entry = index
            .checked_add(1)
            .ok_or(error::internal("a chain's entry count cannot exceed u32"))?;
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
            return Err(error::internal(
                "a delivered answer implies a suspended block",
            ));
        };
        if running.answer.is_some() {
            return Err(error::internal(
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
            .ok_or(error::internal("a live chain holds its frame"))
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
            .ok_or(error::internal("a live chain holds its frame"))
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
