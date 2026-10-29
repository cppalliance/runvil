//! Per-VM tool state: call counts, the offers a section made, the frozen
//! sets its first loop round built from them, and the Engine's two
//! discovery tools, which join the Static Set beside a Dynamic Set.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use runvil_types::packs::PackId;
use runvil_types::tools::{ToolId, ToolInfo};

use crate::error::{Error, Result};
use crate::handles::ToolBinding;

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
            .map_err(|_| Error::Internal("tool call counts mutex was poisoned".into()))
    }

    /// Ensures `id` is present in the map, seeding it at 0 when new.
    ///
    /// # Errors
    /// Returns [`Error::Internal`] if the mutex is poisoned.
    pub fn ensure(&self, id: &str) -> Result<()> {
        let mut map = self.lock()?;
        map.entry(id.to_owned()).or_insert(0);
        Ok(())
    }

    /// Increments the count for `id`, seeding it at 0 first when new.
    ///
    /// # Errors
    /// Returns [`Error::Internal`] if the mutex is poisoned.
    pub fn increment(&self, id: &str) -> Result<()> {
        let mut map = self.lock()?;
        *map.entry(id.to_owned()).or_insert(0) += 1;
        Ok(())
    }

    /// Returns every seeded id with its count, read under one lock.
    ///
    /// # Errors
    /// Returns [`Error::Internal`] if the mutex is poisoned.
    pub(crate) fn snapshot(&self) -> Result<BTreeMap<String, u64>> {
        Ok(self.lock()?.clone())
    }
}

/// What one offer names: a tool by id, or a Pack, whose tools the render
/// expands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OfferTarget {
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
pub struct Offer {
    /// What the offer names.
    pub target: OfferTarget,
    /// Which set the offer feeds.
    pub mode: OfferMode,
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
/// (`models.loop` or a handle's `loop`): the one record of what its model
/// was shown. Each holds its tools' bindings under their wire names, in
/// merge order, and no wire name appears twice across them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenderedTools {
    /// The Static Set: the tools every round of the section advertises,
    /// in schema order.
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

/// One of the Engine's two discovery tools, which join the end of a
/// section entry's Static Set when its Dynamic Set holds a tool: the model
/// finds Dynamic Set tools with the first and calls them with the second.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryTool {
    /// `dynamic/search`, which the model calls as `GetDynamicTools`.
    Search,
    /// `dynamic/call`, which the model calls as `CallDynamicTool`.
    Call,
}

impl DiscoveryTool {
    /// Both discovery tools, in the order they join the Static Set.
    pub const ALL: [DiscoveryTool; 2] = [DiscoveryTool::Search, DiscoveryTool::Call];

    /// The vendor both discovery tools report.
    pub const VENDOR: &'static str = "runvil";

    /// The tool's id, under the reserved Pack id `dynamic`.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            DiscoveryTool::Search => "dynamic/search",
            DiscoveryTool::Call => "dynamic/call",
        }
    }

    /// The wire name the model calls the tool by.
    #[must_use]
    pub fn wire_name(self) -> &'static str {
        match self {
            DiscoveryTool::Search => "GetDynamicTools",
            DiscoveryTool::Call => "CallDynamicTool",
        }
    }

    /// The discovery tool whose id is `id`, if it is one.
    #[must_use]
    pub fn of(id: &ToolId) -> Option<DiscoveryTool> {
        let id = id.to_string();
        DiscoveryTool::ALL.into_iter().find(|tool| tool.id() == id)
    }
}

#[cfg(test)]
#[path = "scope-tests.rs"]
mod tests;
