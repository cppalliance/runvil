//! Per-VM tool state: call counts, the offers a section made, and the
//! frozen sets its first loop round built from them.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use runvil_types::packs::PackId;
use runvil_types::tools::{ToolId, ToolInfo};

use crate::bindings::ToolBinding;
use crate::error::{self, Result};

/// Shared per-VM tool-call counts, keyed by tool id and seeded at 0 for
/// every id the installer was given. `increment` seeds a missing id at 0,
/// and `ensure` pre-seeds one so `tools.stats` lists it before any call.
///
/// The executor increments a count when dispatch is attempted (even if the tool
/// later errors). Lua reads the counts through the `tools.stats` table, which
/// computes its total, its per-Pack counts, and its per-tool counts from a
/// snapshot of them at each read.
#[derive(Debug, Clone, Default)]
pub struct ToolCallCounts {
    inner: Arc<Mutex<BTreeMap<String, u64>>>,
}

impl ToolCallCounts {
    /// Creates a counts map pre-seeded with 0 for every id.
    #[must_use]
    pub(crate) fn new(ids: impl IntoIterator<Item = String>) -> Self {
        let map: BTreeMap<String, u64> = ids.into_iter().map(|id| (id, 0)).collect();
        Self {
            inner: Arc::new(Mutex::new(map)),
        }
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, BTreeMap<String, u64>>> {
        self.inner
            .lock()
            .map_err(|_| error::make_internal_error("tool call counts mutex was poisoned"))
    }

    /// Ensures `id` is present in the map, seeding it at 0 when new.
    ///
    /// # Errors
    /// Returns an `Internal` failure if the mutex is poisoned.
    pub fn ensure(&self, id: &str) -> Result<()> {
        let mut map = self.lock()?;
        map.entry(id.to_owned()).or_insert(0);
        Ok(())
    }

    /// Increments the count for `id`, seeding it at 0 first when new.
    ///
    /// # Errors
    /// Returns an `Internal` failure if the mutex is poisoned.
    pub fn increment(&self, id: &str) -> Result<()> {
        let mut map = self.lock()?;
        *map.entry(id.to_owned()).or_insert(0) += 1;
        Ok(())
    }

    /// Returns every seeded id with its count, read under one lock.
    ///
    /// # Errors
    /// Returns an `Internal` failure if the mutex is poisoned.
    pub(crate) fn snapshot(&self) -> Result<BTreeMap<String, u64>> {
        Ok(self.lock()?.clone())
    }
}

/// What one offer names: a tool by id, or a Pack, whose tools the render
/// expands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OfferTarget {
    /// One tool, by id.
    Tool(ToolId),
    /// Every tool of a Pack, read when the section renders.
    Pack(PackId),
}

/// Which of a section's two sets an offer feeds, or a frozen binding sits
/// in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfferMode {
    /// The Static Set, which every round of the section sends.
    Static,
    /// The Dynamic Set, frozen beside the Static Set.
    Dynamic,
}

impl std::fmt::Display for OfferMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            OfferMode::Static => "Static Set",
            OfferMode::Dynamic => "Dynamic Set",
        })
    }
}

/// One remembered offer: what it names and which set it feeds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Offer {
    /// What the offer names.
    pub(crate) target: OfferTarget,
    /// Which set the offer feeds.
    pub(crate) mode: OfferMode,
}

/// One tool a section's offers name, as the merge takes it: a catalog
/// tool as the run catalog describes it, or an adhoc tool under its
/// binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OfferedTool {
    /// A catalog tool, as the run catalog describes it.
    Catalog(ToolInfo),
    /// An adhoc tool, bound under its name.
    Adhoc(ToolBinding),
}

impl OfferedTool {
    /// Returns the tool's id.
    #[must_use]
    pub fn id(&self) -> &ToolId {
        match self {
            OfferedTool::Catalog(info) => &info.id,
            OfferedTool::Adhoc(binding) => binding.id(),
        }
    }
}

/// The failure of a poisoned section offers lock.
pub(crate) const OFFERS_POISONED: &str = "section offers lock was poisoned";

/// The offers one section VM made, in call order.
#[derive(Debug, Default)]
pub struct Offers {
    /// Every offer, in call order, each held once.
    pub(crate) offers: Vec<Offer>,
}

impl Offers {
    /// Remembers an offer of the tool `id` under `mode`, unless an equal
    /// offer is already held.
    pub fn add_tool(&mut self, id: ToolId, mode: OfferMode) {
        self.add(OfferTarget::Tool(id), mode);
    }

    /// Remembers an offer of the Pack `pack` under `mode`, unless an equal
    /// offer is already held.
    pub fn add_pack(&mut self, pack: PackId, mode: OfferMode) {
        self.add(OfferTarget::Pack(pack), mode);
    }

    fn add(&mut self, target: OfferTarget, mode: OfferMode) {
        let offer = Offer { target, mode };
        if !self.offers.contains(&offer) {
            self.offers.push(offer);
        }
    }
}

/// The two sets a section entry froze at its first loop round
/// (`models.loop`): the one record of what its model
/// was shown. Each holds its tools' bindings under their wire names, in
/// merge order, and no wire name appears twice across them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenderedTools {
    /// The Static Set: the tools every round of the section advertises.
    pub static_set: Vec<ToolBinding>,
    /// The Dynamic Set: the tools the section offered dynamically, which
    /// no round advertises.
    pub dynamic_set: Vec<ToolBinding>,
}

impl RenderedTools {
    /// Returns the binding either set holds under `wire_name`, with the set
    /// that holds it.
    #[must_use]
    pub fn binding(&self, wire_name: &str) -> Option<(&ToolBinding, OfferMode)> {
        [
            (&self.static_set, OfferMode::Static),
            (&self.dynamic_set, OfferMode::Dynamic),
        ]
        .into_iter()
        .find_map(|(set, mode)| {
            set.iter()
                .find(|binding| binding.alias() == wire_name)
                .map(|binding| (binding, mode))
        })
    }
}

#[cfg(test)]
mod tests {
    //! Tests for the scope types: a count increment seeds the id it was
    //! handed.

    use super::ToolCallCounts;

    #[test]
    fn increment_seeds_a_missing_id_and_counts_it() {
        let counts = ToolCallCounts::default();
        counts
            .increment("std/read")
            .expect("increment seeds an id it was never given");
        let snapshot = counts.snapshot().expect("the counts mutex is not poisoned");
        assert_eq!(snapshot.get("std/read"), Some(&1));
    }
}
