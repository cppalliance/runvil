//! Per-VM tool runtime state: call counts, the offers a section made, and
//! the tool list its first loop round rendered from them.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use runvil_model_client::client::ToolSchema;
use runvil_types::packs::PackId;
use runvil_types::tools::ToolId;

use crate::error::{Error, Result};

/// Shared per-VM tool-call counts, keyed by tool id and seeded at 0 for
/// every id the installer was given. A dispatch seeds a missing key on
/// demand through [`ensure`](Self::ensure).
///
/// The executor increments a count when dispatch is attempted (even if the tool
/// later errors). Lua reads the snapshot through the `tools.calls` table.
#[derive(Debug, Clone, Default)]
pub struct ToolCallCounts {
    inner: Arc<Mutex<BTreeMap<String, u64>>>,
}

impl ToolCallCounts {
    /// Creates a counts map pre-seeded with 0 for every name.
    #[must_use]
    pub(crate) fn new(names: impl IntoIterator<Item = String>) -> Self {
        let map: BTreeMap<String, u64> = names.into_iter().map(|name| (name, 0)).collect();
        Self {
            inner: Arc::new(Mutex::new(map)),
        }
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, BTreeMap<String, u64>>> {
        self.inner
            .lock()
            .map_err(|_| Error::Internal("tool call counts mutex was poisoned".into()))
    }

    /// Ensures `name` is present in the map, seeding it at 0 when new.
    ///
    /// # Errors
    /// Returns [`Error::Internal`] if the mutex is poisoned.
    pub fn ensure(&self, name: &str) -> Result<()> {
        let mut map = self.lock()?;
        map.entry(name.to_owned()).or_insert(0);
        Ok(())
    }

    /// Increments the count for `name`.
    ///
    /// # Errors
    /// Returns [`Error::Internal`] if the mutex is poisoned or `name` was
    /// never seeded.
    pub fn increment(&self, name: &str) -> Result<()> {
        let mut map = self.lock()?;
        let count = map.get_mut(name).ok_or_else(|| {
            Error::Internal(Cow::Owned(format!(
                "tool call counts: name {name:?} was not pre-seeded"
            )))
        })?;
        *count += 1;
        Ok(())
    }

    /// Returns the current count for `name`, or `None` when `name` was
    /// never seeded.
    ///
    /// # Errors
    /// Returns [`Error::Internal`] if the mutex is poisoned.
    pub(crate) fn get(&self, name: &str) -> Result<Option<u64>> {
        Ok(self.lock()?.get(name).copied())
    }

    /// Returns a snapshot of every seeded name.
    ///
    /// # Errors
    /// Returns [`Error::Internal`] if the mutex is poisoned.
    pub(crate) fn names(&self) -> Result<Vec<String>> {
        Ok(self.lock()?.keys().cloned().collect())
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

/// Which list an offer feeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfferMode {
    /// The rendered list, which every round of the section sends.
    Static,
    /// The dynamic list, stored beside the rendered list.
    Dynamic,
}

/// One remembered offer: what it names and which list it feeds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    /// What the offer names.
    pub target: OfferTarget,
    /// Which list the offer feeds.
    pub mode: OfferMode,
}

/// The offers one section VM made, in call order.
#[derive(Debug, Default)]
pub struct ToolRuntime {
    /// Every offer, in call order, each held once.
    pub offers: Vec<Offer>,
}

impl ToolRuntime {
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

/// One advertised tool: its id and its Pack's vendor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Advertised {
    /// The tool's id.
    pub id: ToolId,
    /// The vendor of the tool's Pack, as the caller reported it; `None`
    /// when it reported none, and for an adhoc tool.
    pub vendor: Option<String>,
}

/// The tool list a section's first loop round rendered.
#[derive(Debug, Clone)]
pub struct RenderedTools {
    /// The schemas every round of the section sends.
    pub schemas: Vec<ToolSchema>,
    /// Each advertised wire name's tool.
    ///
    /// Every round copies this map onto its chain: the round's answer is
    /// gated against its keys, and the reported batch names each tool's
    /// id and vendor. A model's call is routed by the entry under its wire
    /// name.
    pub dispatch: BTreeMap<String, Advertised>,
    /// The tools the section's dynamic offers named that the rendered
    /// list leaves out, in offer order.
    pub dynamic: Vec<ToolId>,
}
