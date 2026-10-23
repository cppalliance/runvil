//! One run's snapshot of its Packs: [`HostRunContext`], which holds what
//! each of the Packs picked for the run looks like to it and performs the
//! run's tool calls.
//!
//! The snapshot reads each usable Pack's tool list once, after the wait
//! for the run's Packs to be ready, and builds the run's one catalog from
//! the validated lists. The Engine receives a clone of that catalog, and
//! the Harness routes the run's calls through it, so the tools the run
//! offers and the tools its calls may reach are the same list and stay
//! fixed for the run. A run holds one Pack per Pack id, and a tool call
//! goes to the Pack its id's first segment names.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use runvil::effect::ToolCallOrigin;
use runvil::packs::Prelude;
use runvil::tools::{ToolCatalog, ToolError, ToolErrorKind, ToolId, ToolInfo, ToolOutput};
use runvil::vfs::Access;
use runvil::{MissingService, Prompt, Requirements, UnavailablePack, UsablePack};
use runvil_plugin::{HostServices, Pack, PackId, PackInfo, ServiceId, ToolContext};
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
    /// Its `construct` failed, or its `ready` did before this run, for
    /// this reason.
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
    /// Pack's validated tools, in Pack id order.
    pub(super) fn snapshot<'a>(
        packs: impl Iterator<Item = (&'a PackId, PackInfo, &'a Result<Arc<dyn Pack>, String>)>,
        services: HostServices,
        mut failures: BTreeMap<&'static str, String>,
    ) -> HostRunContext {
        let mut seen = BTreeSet::new();
        // The input arrives in pick order, declared Packs first, so the
        // catalog is built after `packs` is collected, from these lists
        // in Pack id order.
        let mut usable_tools: BTreeMap<&PackId, Vec<ToolInfo>> = BTreeMap::new();
        let packs = packs
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

    /// The run's roster: each of its usable Packs, with or without tools,
    /// with the vendor of its FQN, and an entry with the reason for each
    /// one that can't serve, both in id order.
    pub(crate) fn roster(&self) -> (Vec<UsablePack>, Vec<UnavailablePack>) {
        let mut usable = Vec::new();
        let mut unavailable = Vec::new();
        for (name, pack) in &self.packs {
            match pack {
                PackState::Usable { info, .. } => {
                    let vendor = info
                        .fqn
                        .split_once('/')
                        .map_or(info.fqn, |(vendor, _)| vendor);
                    usable.push(UsablePack::new(name.clone(), vendor));
                }
                PackState::Unavailable(reason) => {
                    unavailable.push(UnavailablePack::new(name.clone(), reason.clone()));
                }
                PackState::NeedsUnmet(unmet) => {
                    let services: Vec<String> = unmet.iter().map(ToString::to_string).collect();
                    let reason = format!(
                        "the run lacks the services it needs: {}",
                        services.join(", ")
                    );
                    unavailable.push(UnavailablePack::new(name.clone(), reason));
                }
            }
        }
        (usable, unavailable)
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

    /// What the run cannot meet for `prompt`, for each Pack it declares:
    /// one the pick could not resolve, as `unresolved` holds it, missing
    /// or unavailable with its reason, one that failed to build or to be
    /// ready, and one whose needs the run lacks.
    pub(super) fn requirements(
        &self,
        prompt: &Prompt,
        unresolved: &BTreeMap<PackId, Option<String>>,
    ) -> Requirements {
        let mut requirements = Requirements::default();
        for name in prompt
            .frontmatter()
            .uses()
            .iter()
            .map(|entry| entry.id().clone())
        {
            if let Some(reason) = unresolved.get(&name) {
                match reason {
                    None => requirements.missing_required.push(name),
                    Some(reason) => requirements
                        .unavailable
                        .push(UnavailablePack::new(name, reason.clone())),
                }
                continue;
            }
            match self.packs.get(&name) {
                None => requirements.missing_required.push(name),
                Some(PackState::Unavailable(reason)) => requirements
                    .unavailable
                    .push(UnavailablePack::new(name, reason.clone())),
                Some(PackState::NeedsUnmet(unmet)) => {
                    for service in unmet {
                        tracing::warn!(
                            pack = %name,
                            %service,
                            "the prompt needs a Pack whose service this run lacks"
                        );
                        requirements
                            .missing_services
                            .push(MissingService::new(name.clone(), service.to_string()));
                    }
                }
                Some(PackState::Usable { .. }) => {}
            }
        }
        requirements
    }

    /// The usable Pack the tool id's first segment names, when the
    /// catalog holds the tool, and the tool's info. Validation keeps each
    /// catalog tool under its Pack's id, so the two lookups agree.
    fn offering(&self, tool: &ToolId) -> Option<(&Arc<dyn Pack>, &ToolInfo)> {
        let info = self.catalog.get(tool)?;
        match self.packs.get(&tool.pack())? {
            PackState::Usable { pack, .. } => Some((pack, info)),
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
        let pack = self.offering(&tool).map(|(pack, _)| Arc::clone(pack));
        let services = self.services.clone();
        Box::pin(async move {
            let Some(pack) = pack else {
                tracing::error!(
                    tool = %tool,
                    alias = %alias,
                    "a ToolCall names a tool no usable Pack of the run offers"
                );
                return Err(ToolError::message(format!(
                    "tool `{alias}` ({tool}) is not offered by any usable Pack of this run"
                ))
                .with_kind(ToolErrorKind::Other));
            };
            let cx = ToolContext::new(&tool, &access, &origin, &services);
            pack.call(cx, args).await
        })
    }

    fn survives_stop(&self, tool: &ToolId) -> bool {
        self.offering(tool)
            .is_some_and(|(_, info)| info.survives_stop)
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
