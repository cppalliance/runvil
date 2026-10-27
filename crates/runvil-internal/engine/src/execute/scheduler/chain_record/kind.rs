//! What kind of chain a record is: the main walk, the H1 pass, a
//! `call` child, or a task's backing chain, each with the links only that
//! kind has.

use super::ChainIndex;

/// What kind of chain a record is.
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
