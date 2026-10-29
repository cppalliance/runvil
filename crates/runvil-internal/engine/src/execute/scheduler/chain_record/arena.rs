//! The chain arena: every chain the run has started, and the index that
//! names each one.

use std::ops::{Index, IndexMut};

use crate::{Error, Result};

use super::Chain;

/// The most chains one run may start: the arena indexes chains by `u32`,
/// so the count is bounded by the index space.
const MAX_CHAINS: usize = u32::MAX as usize;

/// Arena index of a chain: indices, not references, so no chain ever holds
/// a pointer to another. Only [`ChainArena::push`] creates one outside test
/// builds, so every index names a chain; a `#[cfg(test)]` constructor mints
/// one for tables that only compare indices. The index is the scheduler's
/// private handle; the chain's identity for authors and callers is its
/// hierarchical [`ChainId`](runvil_types::ids::ChainId), which never depends
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

#[cfg(test)]
impl Chain {
    /// A chain of `kind` under `lineage` that admits at most `concurrency`
    /// tasks at once, holds no access, and has entered no section, for a
    /// test that fills an arena without a run.
    pub(in crate::execute::scheduler) fn for_test(
        lineage: runvil_types::ids::ChainId,
        kind: super::ChainKind,
        concurrency: std::num::NonZeroUsize,
    ) -> Chain {
        use std::sync::Arc;

        use runvil_types::ids::TaskId;

        use super::{CallDepth, ChatAnchor, Counters, Walk};
        use crate::execute::RunContext;
        use crate::execute::context::RunState;
        use crate::execute::section_context::TaskSeed;
        use crate::heading_address::{SectionIndex, SectionPos};
        use crate::parser::Prompt;
        use crate::test_support::scenario::{TEST_SEED, TEST_STARTED_AT};

        let source = concat!(
            "---\nname: t\ndescription: d\nrunvil: 0\n---\n\n",
            "# Title\n\n## Only\n\ndone\n",
        );
        let prompt = Prompt::parse(source, "chain-test")
            .0
            .expect("the test prompt parses");
        let ctx = RunState::new(
            Arc::new(prompt),
            "",
            RunContext::new("chain-test", TEST_SEED, TEST_STARTED_AT),
        );
        Chain {
            task: TaskId::from(lineage.clone()),
            lineage,
            counters: Counters::default(),
            kind,
            seed: TaskSeed::default(),
            parked: None,
            ctx,
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
    /// Returns [`Error::Internal`] when the arena already holds its bound,
    /// leaving the arena unchanged.
    pub(in crate::execute::scheduler) fn push(&mut self, chain: Chain) -> Result<ChainIndex> {
        #[cfg(test)]
        let max_chains = self.max_chains;
        #[cfg(not(test))]
        let max_chains = MAX_CHAINS;
        if self.chains.len() >= max_chains {
            return Err(Error::internal("a run's chain count cannot exceed u32"));
        }
        let id = ChainIndex(u32::try_from(self.chains.len()).map_err(|error| {
            Error::internal_with_cause("a run's chain count cannot exceed u32", error)
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
