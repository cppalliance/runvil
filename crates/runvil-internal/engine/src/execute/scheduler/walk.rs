//! The section walk: sections run in fall-through order, `var` rolls
//! forward across sections and jumps, every section entry takes the
//! chain's next entry id (the chain's hierarchical id extended by its
//! local entry counter), and a jump transfers control - a sibling move
//! within the chain's slice, or a descent into the jumper's child slice
//! with the parent position suspended on the chain's own walk stack
//! until the child level exhausts. A prompt with H1 blocks runs them first
//! as section 0 (the `h1` module), and the root walk starts from that
//! pass's `var` hand-off, sharing the root lineage `0` as a second arena
//! record; a prompt without H1 blocks starts the root walk directly.

use std::sync::Arc;

use runvil_types::ids::ChainId;

use crate::execute::section_context::SectionContext;
use crate::execute::walk_target::{
    JumpTarget, h1_visible_sections, resolve_jump_target, section_position,
};
use crate::heading_address::{self, SectionIndex, SectionPos, Visible};
use crate::parser::Block;
use crate::{Error, Result};

use super::chain::{ChainStart, ChainStep};
use super::chain_record::{CallDepth, ChainKind, Entered};
use super::{ChainIndex, ChainOps, Counters, Scheduler, prompt_origin};

impl Scheduler {
    /// Starts the root walk chain over the top-level sections when the
    /// prompt has no H1 blocks, seeded with an empty `var`, and enqueues
    /// it. The walk is the root chain `0`; its entry 0 stays reserved for
    /// the H1 pass the prompt does not have, so the first walked section
    /// is `0.1` exactly as on a prompt whose pass ran.
    ///
    /// # Errors
    /// Returns [`Error::Internal`] when the run's chain count exceeds `u32`,
    /// or [`Error::Store`] when the backend refuses acquisition.
    pub(super) fn start_root_walk(&mut self) -> Result<()> {
        // The pass the prompt does not have would have taken root entry 0
        // and started no child, so the walk continues from the counters
        // it would have left.
        let after_h1 = Counters {
            next_child: 0,
            next_entry: 1,
        };
        // The root walk is its own serial thread of execution, so a prompt
        // without H1 blocks acquires its capability fresh. It serves every
        // section in turn, so its label is the prompt's own; the line is
        // where the walk starts.
        let prompt = self.ctx.prompt();
        let blocks: &[Block] =
            runvil_parser::detail::entry(prompt).map_or(&[], |section| section.blocks());
        let origin = prompt_origin(prompt, prompt.title(), blocks);
        let access = self.ctx.vfs().acquire(origin).map_err(Error::store)?;
        self.scope = Some(runvil_vfs::detail::scope_handle(&access));
        let root = self.start_chain(ChainStart {
            lineage: ChainId::root(),
            counters: after_h1,
            ctx: self.ctx.clone(),
            access: Arc::new(access),
            start: SectionPos::top(SectionIndex::FIRST),
            kind: ChainKind::Root,
            var: serde_json::json!({}),
            call_depth: CallDepth::ROOT,
            concurrency: self.ctx.limits().max_concurrency,
        })?;
        self.ready.push_back(root);
        Ok(())
    }

    /// Enters the chain's next section and requeues it, or finishes the
    /// chain when its slice is exhausted.
    ///
    /// # Errors
    /// Returns the [`Error`] of frame construction, as documented on
    /// [`SectionContext::new`].
    pub(super) fn advance_entry(&mut self, id: ChainIndex) -> Result<ChainStep> {
        if self.with_chain(id, ChainOps::enter_section)?
            || self.with_chain(id, ChainOps::pop_position)
        {
            // A section was entered, or a jump-started child level
            // exhausted and the parent walk resumes after the jumper.
            self.ready.push_back(id);
            return Ok(ChainStep::Continue);
        }
        // The walk ran off the slice's last section: the chain ends.
        Ok(self.finish(id, Ok(None)))
    }
}

impl ChainOps<'_> {
    /// Enters the chain's next section and reports whether one was entered:
    /// constructs the frame with the chain's next entry id and its task id,
    /// seeded from the chain's `var` slot (and, on a spawned
    /// chain's first entry, its `item` and `sys.index` seeds). The pending
    /// Markdown buffer
    /// resets: a previous section's unconsumed prose never crosses the
    /// boundary. `Ok(false)` means the
    /// slice is exhausted and the chain ends.
    ///
    /// # Errors
    /// Returns the [`Error`] of frame construction, as documented on
    /// [`SectionContext::new`].
    fn enter_section(&mut self) -> Result<bool> {
        let prompt = self.prompt();
        let chain = &mut *self.chain;
        if chain.kind.is_h1() {
            // The H1 pass enters its frame exactly once: section 0 under
            // the prompt's title, through the same install path as any
            // section - and no SECTION_STARTED, the pass is not a walked
            // section. Its id is the root chain's entry 0.
            let section_id = chain.next_entry_id()?;
            let frame =
                SectionContext::new_h1(&chain.ctx, chain.access()?, &section_id, &chain.task)?;
            chain.entered = Some(Entered::new(frame));
            return Ok(true);
        }
        // `section` borrows the shared prompt tree, not the arena, so the
        // frame construction can borrow the chain's own context and slots.
        let Some(section) = chain.walk.current().section(&prompt) else {
            return Ok(false);
        };
        let section_id = chain.next_entry_id()?;
        let task = chain.task.clone();
        // A spawned chain's first entry consumes its `item` and `sys.index`
        // seeds; every later entry, and every other chain's, has none.
        let seed = std::mem::take(&mut chain.seed);
        let frame = SectionContext::new(
            &chain.ctx,
            chain.access()?,
            chain.walk.current(),
            &section_id,
            &task,
            &chain.var,
            seed,
        )?;
        chain.entered = Some(Entered::new(frame));
        section.name().clone_into(&mut chain.entered_name);
        Ok(true)
    }

    /// Resumes a jump-suspended parent position when a child level
    /// exhausts, returning `false` when the chain holds no suspended
    /// position - meaning its own root slice exhausted and the chain ends.
    /// The child walk shared the `var` slot, so it already holds the
    /// child level's last value.
    fn pop_position(&mut self) -> bool {
        self.chain.walk.pop()
    }

    /// Falls the chain through at its section's end: the section's final
    /// `var` replaces the chain's `var` table, read back while the VM is
    /// live; the frame's drop is
    /// the teardown boundary, firing `SECTION_FINISHED` for this completed
    /// section; then the walk advances to the next section.
    ///
    /// # Errors
    /// Returns [`Error::Lua`] when the final `var` read-back fails (the
    /// frame drops unarmed), or [`Error::Internal`] when the chain holds no
    /// frame.
    pub(super) fn end_section(&mut self) -> Result<()> {
        let chain = &mut *self.chain;
        let Some(Entered { mut frame, .. }) = chain.entered.take() else {
            return Err(Error::internal("a section end implies a live frame"));
        };
        chain.var = frame.read_var()?;
        frame.mark_completed();
        drop(frame);
        chain.walk.advance();
        Ok(())
    }

    /// Applies a jump's control transfer: closes the jumper's frame as
    /// completed (the final `var`
    /// rolled forward; the armed drop firing `SECTION_FINISHED`, a jump
    /// being a completion), resolves the heading against the jumper's
    /// visible set, and moves the walk. A sibling target replaces the
    /// walk's current position; a child target suspends the jumper on the
    /// chain's walk stack and descends into the jumper's child slice from
    /// the target.
    ///
    /// # Errors
    /// Returns [`Error::Lua`] when the `var` read-back fails (the
    /// frame drops unarmed) or when the heading matches no visible section
    /// or more than one - the jumper's frame has already closed as
    /// completed.
    pub(super) fn apply_jump(&mut self, heading: &str) -> Result<()> {
        {
            let chain = &mut *self.chain;
            let Some(Entered { mut frame, .. }) = chain.entered.take() else {
                return Err(Error::internal("a jump implies a live frame"));
            };
            chain.var = frame.read_var()?;
            frame.mark_completed();
            drop(frame);
        }
        let target = self.resolve_chain_target(heading)?;
        self.chain.walk.jump(target);
        Ok(())
    }

    /// Resolves `heading` against the chain's current section's visible
    /// set into the position the walk or a contained chain continues from:
    /// a direct child's place in the jumper's child slice, or a sibling's
    /// in the jumper's own. The H1 pass's set is the whole top-level
    /// slice, so its targets never descend.
    ///
    /// # Errors
    /// Returns [`Error::Lua`] when the heading is malformed, matches no
    /// visible section, or matches more than one (see
    /// [`heading_address::resolve_visible`]).
    pub(super) fn resolve_chain_target(&self, heading: &str) -> Result<JumpTarget> {
        let prompt = self.prompt();
        let chain = &*self.chain;
        if chain.kind.is_h1() {
            let sections = h1_visible_sections(&prompt);
            let target = heading_address::resolve_visible(heading, Visible::all(sections))?;
            let index = section_position(sections, target).ok_or(Error::internal(
                "a resolved H1 target is absent from the top-level slice",
            ))?;
            return Ok(JumpTarget {
                pos: SectionPos::top(index),
                descends: false,
            });
        }
        resolve_jump_target(heading, &prompt, chain.walk.current())
    }
}
