//! The chain record's [`Entered`] section and its [`Running`] block.

use std::num::NonZeroU32;

use mlua::Thread;

use crate::Result;
use crate::execute::section_context::SectionContext;
use crate::lua::Answer;
use crate::prompt::{Block, BodyText};

use super::BlockIndex;

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
