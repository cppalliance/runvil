//! Per-VM tool runtime state: call counts, the scoped tool set a section
//! sees, and the tool list its first loop round rendered from that set.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use runvil_model_client::client::ToolSchema;
use runvil_types::packs::PackId;
use runvil_types::tools::ToolId;

use crate::error::{Error, Result};

/// Shared per-VM tool-call counts, seeded at 0 for every key the installer
/// was given: a catalog tool's id, or an adhoc tool's alias. A dispatch
/// seeds a missing key on demand through [`ensure`](Self::ensure).
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

/// Tracks the tools and Packs one section VM offered.
#[derive(Debug, Default)]
pub struct ToolRuntime {
    /// The wire names `tools.offer` put in the section's scope, in
    /// first-offer order.
    pub added: Vec<String>,
    /// The Packs `packs.offer` put in the section's scope, in
    /// first-offer order.
    pub packs: Vec<PackId>,
}

/// The tool list a section's first loop round rendered.
#[derive(Debug, Clone)]
pub struct RenderedTools {
    /// The schemas every round of the section sends.
    pub schemas: Vec<ToolSchema>,
    /// Each advertised name's catalog tool, `None` for an adhoc tool.
    ///
    /// Every round copies this map onto its chain: the round's answer is
    /// gated against its keys, and the reported batch names a catalog
    /// tool's id. The `tool_call` the loop then yields resolves each name
    /// itself, a wire name against the run's offering and an adhoc alias
    /// against the section VM's handlers.
    pub dispatch: BTreeMap<String, Option<ToolId>>,
}
