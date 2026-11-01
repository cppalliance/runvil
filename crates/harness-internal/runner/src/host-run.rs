//! One run's snapshot of its Packs: [`HostRunContext`], which holds what
//! each of the Packs picked for the run looks like to it and performs the
//! run's renders and tool calls.
//!
//! The snapshot reads each usable Pack's tool list once, after the wait
//! for the run's Packs to be ready, and builds the run's one catalog from
//! the validated lists, which stays fixed for the run. The Engine receives
//! a clone of that catalog. Each section entry's render asks the Packs it
//! offered tools from what the model sees, and a render may add tools
//! only the model sees, which no catalog lists. A run holds one Pack per
//! Pack id, and every render and tool call goes to the Pack its id's
//! first segment names, so a call reaches a model-only tool as it reaches
//! a catalog tool.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use runvil::Prompt;
use runvil::effect::{PackToolSets, ToolCallOrigin};
use runvil::model::ModelInfo;
use runvil::packs::{Prelude, RosterEntry};
use runvil::tools::{ToolCatalog, ToolError, ToolErrorKind, ToolId, ToolInfo, ToolOutput};
use runvil::vfs::Access;
use runvil_plugin::{HostServices, Pack, PackId, PackInfo, RenderContext, ServiceId, ToolContext};
use serde_json::Value;

use crate::performers::{BoxFuture, ToolPerformer};

/// One run's frozen snapshot of its Packs, by Pack id, its one tool
/// catalog, and the run's own services.
pub(crate) struct HostRunContext {
    services: HostServices,
    packs: BTreeMap<PackId, PackState>,
    catalog: ToolCatalog,
}

/// What one of the run's Packs looks like to it.
enum PackState {
    /// It can serve; its validated tools are in the run's catalog.
    Usable { info: PackInfo, pack: Arc<dyn Pack> },
    /// The pick could not resolve it, or its `construct` failed, or its
    /// `ready` did before this run, for this reason.
    Unavailable(String),
    /// The run lacks these services from the Pack's needs.
    NeedsUnmet(Vec<ServiceId>),
}

impl HostRunContext {
    /// Takes the snapshot of the run's `packs`, which share no Pack id,
    /// with `services` as the run's own.
    ///
    /// A built Pack whose FQN `failures` names is unavailable with the
    /// reason it maps to. Any other built Pack is usable when `services`
    /// provide every id the Pack needs; only then is its tool list read
    /// and validated. The run's one catalog is built from every usable
    /// Pack's validated tools, in Pack id order. Each declared Pack id the
    /// pick could not resolve that `unresolved` gives a reason for is
    /// unavailable with that reason; one with no reason stays out, so the
    /// run finds it missing.
    pub(super) fn snapshot<'a>(
        packs: impl Iterator<Item = (&'a PackId, PackInfo, &'a Result<Arc<dyn Pack>, String>)>,
        services: HostServices,
        mut failures: BTreeMap<&'static str, String>,
        unresolved: &BTreeMap<PackId, Option<String>>,
    ) -> HostRunContext {
        let mut seen = BTreeSet::new();
        // The input arrives in pick order, declared Packs first, so the
        // catalog is built after `packs` is collected, from these lists
        // in Pack id order.
        let mut usable_tools: BTreeMap<&PackId, Vec<ToolInfo>> = BTreeMap::new();
        let mut packs: BTreeMap<PackId, PackState> = packs
            .map(|(name, info, built)| {
                let state = match (built, failures.remove(info.fqn)) {
                    (Err(reason), _) => PackState::Unavailable(reason.clone()),
                    (Ok(_), Some(reason)) => PackState::Unavailable(reason),
                    (Ok(pack), None) => {
                        let unmet: Vec<ServiceId> = info
                            .needs
                            .iter()
                            .copied()
                            .filter(|service| !services.provides(service))
                            .collect();
                        if unmet.is_empty() {
                            usable_tools.insert(name, validated(name, pack.tools(), &mut seen));
                            PackState::Usable {
                                info,
                                pack: Arc::clone(pack),
                            }
                        } else {
                            PackState::NeedsUnmet(unmet)
                        }
                    }
                };
                (name.clone(), state)
            })
            .collect();
        packs.extend(unresolved.iter().filter_map(|(name, reason)| {
            let reason = reason.clone()?;
            Some((name.clone(), PackState::Unavailable(reason)))
        }));
        let catalog =
            ToolCatalog::new(usable_tools.into_values().flatten()).unwrap_or_else(|error| {
                // Every tool passed containment and uniqueness validation one
                // at a time, so this build cannot fail.
                tracing::warn!(%error, "the run's catalog failed after per-tool validation");
                ToolCatalog::default()
            });
        HostRunContext {
            services,
            packs,
            catalog,
        }
    }

    /// The run's catalog: a clone sharing the snapshot's one list.
    pub(crate) fn catalog(&self) -> ToolCatalog {
        self.catalog.clone()
    }

    /// The run's roster, the one account of how each of its Packs stands:
    /// each usable Pack, with or without tools, with the vendor of its
    /// FQN, and each one that can't serve with the reason, a declared Pack
    /// the pick could not resolve included, in id order.
    pub(crate) fn roster(&self) -> BTreeMap<PackId, RosterEntry> {
        self.packs
            .iter()
            .map(|(name, pack)| {
                let entry = match pack {
                    PackState::Usable { info, .. } => {
                        let vendor = info
                            .fqn
                            .split_once('/')
                            .map_or(info.fqn, |(vendor, _)| vendor);
                        RosterEntry::Usable {
                            vendor: vendor.to_owned(),
                        }
                    }
                    PackState::Unavailable(reason) => RosterEntry::Unavailable {
                        reason: reason.clone(),
                    },
                    PackState::NeedsUnmet(unmet) => {
                        let services: Vec<String> = unmet.iter().map(ToString::to_string).collect();
                        RosterEntry::Unavailable {
                            reason: format!(
                                "the run lacks the services it needs: {}",
                                services.join(", ")
                            ),
                        }
                    }
                };
                (name.clone(), entry)
            })
            .collect()
    }

    /// The preludes of the usable Packs `prompt` declares, in
    /// declaration order, each under the Pack's id.
    pub(crate) fn preludes(&self, prompt: &Prompt) -> Vec<Prelude> {
        prompt
            .frontmatter()
            .uses()
            .iter()
            .filter_map(|entry| match self.packs.get(entry.id()) {
                Some(PackState::Usable { info, .. }) => info
                    .prelude
                    .map(|source| Prelude::new(entry.id().clone(), source)),
                _ => None,
            })
            .collect()
    }

    /// The usable Pack whose id is `pack`.
    fn usable(&self, pack: &PackId) -> Option<&Arc<dyn Pack>> {
        match self.packs.get(pack)? {
            PackState::Usable { pack, .. } => Some(pack),
            PackState::Unavailable(_) | PackState::NeedsUnmet(_) => None,
        }
    }
}

impl ToolPerformer for HostRunContext {
    fn call(
        &self,
        tool: ToolId,
        alias: String,
        access: Arc<Access>,
        origin: ToolCallOrigin,
        args: Value,
    ) -> BoxFuture<Result<ToolOutput, ToolError>> {
        let pack = self.usable(&tool.pack()).cloned();
        let services = self.services.clone();
        Box::pin(async move {
            let Some(pack) = pack else {
                tracing::error!(
                    tool = %tool,
                    alias = %alias,
                    "a ToolCall names a tool of no usable Pack of the run"
                );
                return Err(ToolError::message(format!(
                    "tool `{alias}` ({tool}) belongs to no usable Pack of this run"
                ))
                .with_kind(ToolErrorKind::Other));
            };
            let cx = ToolContext::new(&tool, &access, &origin, &services);
            pack.call(cx, args).await
        })
    }

    /// Asks each usable Pack's `render` once per non-empty set, one at a
    /// time. A Pack the run cannot use has no catalog tool, so no render
    /// asks for it; should one, its tools pass through as offered.
    fn render(
        &self,
        entry: String,
        model: ModelInfo,
        offered: Vec<PackToolSets<ToolInfo>>,
    ) -> BoxFuture<Vec<PackToolSets<ToolInfo>>> {
        let packs: Vec<Option<Arc<dyn Pack>>> = offered
            .iter()
            .map(|sets| self.usable(&sets.pack).cloned())
            .collect();
        Box::pin(async move {
            let mut rendered = offered;
            for (sets, pack) in rendered.iter_mut().zip(packs) {
                let Some(pack) = pack else {
                    tracing::error!(pack = %sets.pack, "a Render names no usable Pack of the run");
                    continue;
                };
                for set in [&mut sets.static_set, &mut sets.dynamic_set] {
                    if !set.is_empty() {
                        let tools = std::mem::take(set);
                        *set = pack.render(RenderContext::new(&entry, &model), tools).await;
                    }
                }
            }
            rendered
        })
    }
}

impl fmt::Debug for HostRunContext {
    /// Shows the run's services, each Pack's name and state, and its
    /// catalog's tool ids.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let packs: Vec<(&PackId, &str)> = self
            .packs
            .iter()
            .map(|(name, pack)| {
                let state = match pack {
                    PackState::Usable { .. } => "usable",
                    PackState::Unavailable(_) => "unavailable",
                    PackState::NeedsUnmet(_) => "needs unmet",
                };
                (name, state)
            })
            .collect();
        f.debug_struct("HostRunContext")
            .field("services", &self.services)
            .field("packs", &packs)
            .field("catalog", &self.catalog)
            .finish()
    }
}

/// The tools of the Pack whose id is `pack` that pass validation: a
/// tool must sit under the Pack's id and must not repeat an id in
/// `seen`. A tool that fails is logged and dropped, so it costs only
/// itself.
fn validated(pack: &PackId, tools: Vec<ToolInfo>, seen: &mut BTreeSet<ToolId>) -> Vec<ToolInfo> {
    tools
        .into_iter()
        .filter(|tool| {
            if !pack.contains(&tool.id) {
                tracing::warn!(
                    pack = %pack,
                    tool = %tool.id,
                    "a Pack's tool id escapes its Pack id; dropped"
                );
                return false;
            }
            if seen.contains(&tool.id) {
                tracing::warn!(
                    pack = %pack,
                    tool = %tool.id,
                    "a Pack's tool id repeats an earlier tool; dropped"
                );
                return false;
            }
            seen.insert(tool.id.clone());
            true
        })
        .collect()
}
