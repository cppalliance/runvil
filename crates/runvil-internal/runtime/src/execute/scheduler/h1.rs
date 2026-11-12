//! The H1 pass: the prompt's H1 blocks run first as section 0, the
//! driver loop's first chain, under the walk's rules with these deltas:
//! the frame takes the root chain's entry 0 and fires no section
//! observations, a scalar return short-circuits the run, a block's
//! `LuaRuntime` failure is the prompt's failed hard gate, mapped to a
//! `RequirementsUnmet` failure where it failed, and the pass's end hands
//! off to the root walk. The frame's own deltas, writable `argv` among
//! them, are listed on `SectionContext::new_h1`.
//!
//! The pass and the walk that follows it share the root lineage `0` and
//! are two arena records: the hand-off starts the walk from the pass's
//! `var` and frozen `argv`, continues the pass's child and entry counters,
//! so a child the pass started keeps its index and the first walked section
//! takes the next entry after the pass's entry 0 (see [`Chain::counters`]
//! for the normative statement), and hands the pass's tasks to the walk as
//! their owner.
//!
//! [`Chain::counters`]: super::Chain::counters

use std::num::NonZeroU32;
use std::sync::Arc;

use runvil_types::error::place;

use crate::section_pos::{SectionIndex, SectionPos};
use crate::{Result, error};

use super::chain::ChainStep;
use super::chain_record::{ChainKind, Entered};
use super::{ChainIndex, Counters, GENERIC_COMPLETION, Scheduler, prompt_origin};

impl Scheduler {
    /// Starts the H1 pass as the driver loop's first chain: the prompt's
    /// H1 blocks under its title - section 0 - driven through the same
    /// coroutine machinery as any section. The root start enqueues the
    /// chain itself, so this returns no index and no caller pushes it.
    ///
    /// # Errors
    /// Returns an `Internal` failure when the run's chain count exceeds `u32`,
    /// or a store failure when the backend refuses acquisition.
    pub(super) fn start_h1(&mut self) -> Result<()> {
        // The H1 pass runs under the prompt's title, from its first
        // compiled H1 chunk.
        let origin = prompt_origin(
            self.ctx.prompt(),
            self.ctx.prompt().title(),
            crate::prompt::detail::h1_blocks(self.ctx.prompt()),
        );
        let (access, scope) = self
            .ctx
            .vfs()
            .acquire_scope(origin)
            .map_err(|cause| error::make_store_error(&cause))?;
        self.scope = Some(scope);
        // The pass's one frame takes the root lineage's entry 0, and the
        // walk that follows continues its counters (see `Chain::lineage`).
        // Its admission limit is the run's ceiling.
        self.start_root_chain(
            ChainKind::H1,
            Counters::default(),
            self.ctx.clone(),
            self.ctx.default_binding(),
            Arc::new(access),
            SectionPos::top(SectionIndex::FIRST),
            serde_json::json!({}),
            self.ctx.limits().max_concurrency,
        )?;
        Ok(())
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
    /// Returns a `LuaRuntime` failure when the final `var` read-back fails or H1
    /// left `argv` as non-JSON data, placed on the pass's last Lua block at
    /// a fall-through and on the jumping block for a jump out,
    /// or an `Internal` failure when the chain holds no frame, when the walk
    /// it hands off to cannot start because the run's chain count exceeds
    /// `u32`, or when the pass's capability is gone - the walk reuses it, so
    /// it acquires nothing and has nothing to fall back on.
    pub(super) fn end_h1(&mut self, id: ChainIndex, start: SectionPos) -> Result<ChainStep> {
        let prompt = self.prompt();
        let chain = &mut self.chains[id];
        let Some(Entered { frame, block, .. }) = chain.entered.take() else {
            return Err(error::make_internal_error(
                "the H1 pass ends with a live frame",
            ));
        };
        let line = block.lua_line(chain.blocks(&prompt));
        let var = frame
            .read_var()
            .map_err(|error| place(error, prompt.file(), line))?;
        // The freeze: whatever `argv` H1 leaves behind - the derived parse
        // or its repair - is what every walked section inherits, frozen.
        let argv = frame
            .read_argv()
            .map_err(|error| place(error, prompt.file(), line))?;
        drop(frame);
        // The root identity spans the pass and the walk: the walk reuses
        // the pass's capability, so nothing the pass wrote false-conflicts
        // with the walk, and a task the pass spawned joins the one root
        // identity.
        let walk_access = Arc::clone(chain.access()?);
        // The walk continues the pass's counters (see `Chain::counters` for
        // the normative statement): the pass took entry 0, the first walked
        // section takes entry 1, and a child the pass started keeps its
        // index. Its admission limit continues too: the pass may have
        // lowered it with `tasks.concurrency`.
        let counters = chain.counters;
        let concurrency = chain.concurrency;
        // The walk starts on the model the pass ended on.
        let model = chain.current_model()?;
        if crate::prompt::detail::sections(self.ctx.prompt()).is_empty() {
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
        let root = self.start_root_chain(
            ChainKind::Root,
            counters,
            walk_ctx,
            model,
            walk_access,
            start,
            var,
            concurrency,
        )?;
        // The walk is the pass's continuation, so the tasks the pass
        // spawned are the walk's from here: it waits on, cancels, or leaks
        // them exactly as if it had spawned them.
        self.reassign_tasks(id, root);
        Ok(ChainStep::Continue)
    }

    /// Ends the H1 pass on a jump out: the path resolves from the H1,
    /// whose children are the top-level sections, through
    /// [`resolve_chain_target`](super::ChainOps::resolve_chain_target),
    /// then the pass ends and the root walk starts at the target.
    ///
    /// # Errors
    /// Returns a `LuaRuntime` failure when the path is refused or names no
    /// top-level section, placed on `line`, the `jump` call's; the pass's
    /// own ending can fail as [`end_h1`](Self::end_h1) documents.
    pub(super) fn end_h1_at_jump(
        &mut self,
        id: ChainIndex,
        path: &str,
        line: Option<NonZeroU32>,
    ) -> Result<ChainStep> {
        let prompt = self.prompt();
        let target = self
            .with_chain(id, |ops| ops.resolve_chain_target(path, None))
            .map_err(|error| place(error, prompt.file(), line))?;
        self.end_h1(id, target.pos)
    }
}
