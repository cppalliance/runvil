//! The caller-supplied [`ToolCatalog`] of `ToolInfo` values.

use std::fmt;
use std::sync::Arc;

use super::ids::ToolId;
use super::info::ToolInfo;
use crate::names::IdError;

/// A catalog of the tools a run may offer, given as `ToolInfo` values.
///
/// The caller builds the catalog from every Pack it can serve, declared
/// by the prompt or not, and keeps the tool implementations itself. The
/// catalog lists every tool the run can use: the prompt's Lua calls one by
/// its [`ToolId`], and a section offers one to its model with
/// `tools.offer`.
///
/// Every tool in a catalog has a unique [`ToolId`]. Construction checks it,
/// and the [`get`](Self::get) lookup relies on that check.
///
/// Cloning is cheap, because all clones share one reference-counted list of
/// `ToolInfo` values.
#[derive(Clone, Default)]
#[non_exhaustive]
pub struct ToolCatalog {
    tools: Arc<[ToolInfo]>,
}

impl fmt::Debug for ToolCatalog {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ToolCatalog")
            .field(
                "ids",
                &self.tools.iter().map(|tool| &tool.id).collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl ToolCatalog {
    /// Builds a catalog from `ToolInfo` values, keeping the order they are
    /// given in.
    ///
    /// This is the only place the catalog checks identities.
    ///
    /// # Errors
    /// Returns [`IdError`] if two `ToolInfo` values share a [`ToolId`].
    pub fn new(tools: impl IntoIterator<Item = ToolInfo>) -> Result<Self, IdError> {
        let tools: Vec<ToolInfo> = tools.into_iter().collect();
        let mut seen = std::collections::BTreeSet::new();
        for tool in &tools {
            if !seen.insert(&tool.id) {
                return Err(IdError::new(format!(
                    "duplicate tool identity `{}` in the tool catalog",
                    tool.id
                )));
            }
        }
        Ok(Self {
            tools: Arc::from(tools),
        })
    }

    /// Returns the tool info for `id`, if one is in the catalog.
    ///
    /// The lookup scans the `ToolInfo` values one by one.
    #[must_use]
    pub fn get(&self, id: &ToolId) -> Option<&ToolInfo> {
        self.tools.iter().find(|tool| tool.id == *id)
    }

    /// Returns the catalog's `ToolInfo` values in supplied order.
    #[must_use]
    pub fn tools(&self) -> &[ToolInfo] {
        &self.tools
    }
}
