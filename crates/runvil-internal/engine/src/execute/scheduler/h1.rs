//! The H1 pass: the prompt's H1 blocks run first as section 0, the
//! driver loop's first chain, under the walk's rules with these deltas:
//! the frame takes the root chain's entry 0 and fires no section
//! observations, a scalar return short-circuits the run, a Lua failure is
//! the prompt's failed hard gate, mapped to [`Error::RequirementsUnmet`],
//! and the pass's end hands off to the root walk. The frame's own deltas,
//! writable `argv` among them, are listed on `SectionContext::new_h1`.
//!
//! The pass and the walk that follows it share the root lineage `0` and
//! are two arena records: the hand-off starts the walk from the pass's
//! `var` and frozen `argv`, continues the pass's child and entry counters,
//! so the first walked section is `0.1` and a child the pass started keeps
//! its index, and hands the pass's tasks to the walk as their owner.

use std::sync::Arc;

use runvil_types::ids::ChainId;

use crate::heading_address::{SectionIndex, SectionPos};
use crate::{Error, Result};

use super::chain::{ChainStart, ChainStep};
use super::chain_record::{CallDepth, ChainKind, Entered};
use super::{ChainIndex, Counters, GENERIC_COMPLETION, Scheduler, prompt_origin};

impl Scheduler {
    /// Starts the H1 pass as the driver loop's first chain: the prompt's
    /// H1 blocks under its title - section 0 - driven through the same
    /// coroutine machinery as any section.
    ///
    /// # Errors
    /// Returns [`Error::Internal`] when the run's chain count exceeds `u32`,
    /// or [`Error::Store`] when the backend refuses acquisition.
    pub(super) fn start_h1(&mut self) -> Result<ChainIndex> {
        // The H1 pass runs under the prompt's title, from its first
        // compiled H1 chunk.
        let origin = prompt_origin(
            self.ctx.prompt(),
            self.ctx.prompt().title(),
            runvil_parser::detail::h1_blocks(self.ctx.prompt()),
        );
        let access = self.ctx.vfs().acquire(origin).map_err(Error::store)?;
        self.scope = Some(runvil_vfs::detail::scope_handle(&access));
        // The pass is the root lineage's first record: its one frame takes
        // entry 0, and the walk that follows, a second record under the
        // same lineage, continues its counters. Its admission limit is the
        // run's ceiling.
        self.start_chain(ChainStart {
            lineage: ChainId::root(),
            counters: Counters::default(),
            ctx: self.ctx.clone(),
            access: Arc::new(access),
            start: SectionPos::top(SectionIndex::FIRST),
            kind: ChainKind::H1,
            var: serde_json::json!({}),
            call_depth: CallDepth::ROOT,
            concurrency: self.ctx.limits().max_concurrency,
        })
    }

    /// Ends the H1 pass at its fall-through: the final `var` read back
    /// while the VM is live, then the frame drops unarmed - the
    /// pass never arms completion, so `SECTION_FINISHED` never fires for
    /// it. The root walk then starts from the `var` hand-off at `start`
    /// (the first section on a fall-through, the resolved target on a jump
    /// out) under the walk's own context fork; with no sections the run's
    /// result is the shared generic completion.
    ///
    /// # Errors
    /// Returns [`Error::Lua`] when the final `var` read-back fails or H1
    /// left `argv` as non-JSON data,
    /// or [`Error::Internal`] when the chain holds no frame. The walk
    /// reuses the pass's capability, so the hand-off acquires nothing.
    pub(super) fn end_h1(&mut self, id: ChainIndex, start: SectionPos) -> Result<ChainStep> {
        let chain = &mut self.chains[id];
        let Some(Entered { frame, .. }) = chain.entered.take() else {
            return Err(Error::internal("the H1 pass ends with a live frame"));
        };
        let var = frame.read_var()?;
        // The freeze: whatever `argv` H1 leaves behind - the derived parse
        // or its repair - is what every walked section inherits, frozen.
        let argv = frame.read_argv()?;
        drop(frame);
        // The root identity spans the pass and the walk: the walk reuses
        // the pass's capability, so nothing the pass wrote false-conflicts
        // with the walk, and a task the pass spawned joins the one root
        // identity.
        let walk_access = Arc::clone(chain.access()?);
        // The walk shares the root lineage with the pass, so it continues
        // the pass's counters: the pass took entry 0, the first walked section
        // takes entry 1, and a child the pass started keeps its index. Its
        // admission limit continues too: the pass may have lowered it with
        // `tasks.concurrency`.
        let counters = chain.counters;
        let concurrency = chain.concurrency;
        if runvil_parser::detail::sections(self.ctx.prompt()).is_empty() {
            // No walk follows, so the pass's end is the run's end: a task
            // the pass spawned and left live ends here under the same
            // rules as a finishing chain.
            return Ok(ChainStep::RunEnded(
                self.settle_owned_tasks(id, Ok(GENERIC_COMPLETION.to_owned())),
            ));
        }
        // The H1-to-walk handoff: the walk's context takes the frozen
        // `argv`; H1's prompt-wide records already landed in the shared
        // sets the views read, and `when` is the run's own.
        let walk_ctx = self.ctx.with_walk_state(argv);
        let root = self.start_chain(ChainStart {
            lineage: ChainId::root(),
            counters,
            ctx: walk_ctx,
            access: walk_access,
            start,
            kind: ChainKind::Root,
            var,
            call_depth: CallDepth::ROOT,
            concurrency,
        })?;
        // The walk is the pass's continuation, so the tasks the pass
        // spawned are the walk's from here: it waits on, cancels, or leaks
        // them exactly as if it had spawned them.
        self.reassign_tasks(id, root);
        self.ready.push_back(root);
        Ok(ChainStep::Continue)
    }

    /// Ends the H1 pass on a jump out: the heading resolves against the
    /// pass's top-level set through
    /// [`resolve_chain_target`](Self::resolve_chain_target), then the pass
    /// ends and the root walk starts at the target.
    ///
    /// # Errors
    /// Returns [`Error::Lua`] when the heading is malformed, matches no
    /// top-level section, or matches more than one; the pass's own ending
    /// can fail as [`end_h1`](Self::end_h1) documents.
    pub(super) fn end_h1_at_jump(&mut self, id: ChainIndex, heading: &str) -> Result<ChainStep> {
        let target = self.resolve_chain_target(id, heading)?;
        self.end_h1(id, target.pos)
    }
}
