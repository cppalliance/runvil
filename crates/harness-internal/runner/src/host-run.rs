//! One run's snapshot of the Host's Plugins: [`HostRunContext`], which
//! holds what each installed Plugin looks like to the run and performs
//! the run's tool calls.
//!
//! The snapshot reads each usable Plugin's tool list once, after the wait
//! for every Plugin to be ready, so the run's catalog and the tools its
//! calls may reach stay fixed for the run. A tool call goes to the Plugin
//! its id's first segment names.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use runvil::effect::ToolCallOrigin;
use runvil::packs::Prelude;
use runvil::tools::{ToolCatalog, ToolError, ToolErrorKind, ToolId, ToolInfo, ToolOutput};
use runvil::vfs::Access;
use runvil::{MissingService, Prompt, Requirements, UnavailablePack};
use runvil_plugin::{HostServices, PackId, Plugin, PluginInfo, ServiceId, ToolContext};
use serde_json::Value;

use crate::performers::{BoxFuture, ToolPerformer};

/// One run's frozen snapshot of every installed Plugin, beside the run's
/// own services, which every call it performs is lent.
pub(crate) struct HostRunContext {
    services: HostServices,
    packs: BTreeMap<PackId, RunPlugin>,
}

/// What one installed Plugin looks like to a run.
enum RunPlugin {
    /// It can serve; `tools` is its tool list after validation.
    Usable {
        info: PluginInfo,
        plugin: Arc<dyn Plugin>,
        tools: Vec<ToolInfo>,
    },
    /// Its `construct` failed, or its `ready` did before this run, for
    /// this reason.
    Unavailable(String),
    /// The run lacks these services from the Plugin's needs.
    NeedsUnmet(Vec<ServiceId>),
}

impl HostRunContext {
    /// Takes the snapshot of the `installed` Plugins, in install order,
    /// with `services` as the run's own.
    ///
    /// A built Plugin named in `failures` is unavailable with the reason
    /// it maps to. Any other built Plugin is usable when `services`
    /// provide every id the Plugin needs; only then is its tool list read
    /// and validated.
    pub(super) fn snapshot<'a>(
        installed: impl Iterator<Item = (&'a PackId, PluginInfo, &'a Result<Arc<dyn Plugin>, String>)>,
        services: HostServices,
        mut failures: BTreeMap<PackId, String>,
    ) -> HostRunContext {
        let mut seen = BTreeSet::new();
        let packs = installed
            .map(|(name, info, built)| {
                let state = match (built, failures.remove(name)) {
                    (Err(reason), _) => RunPlugin::Unavailable(reason.clone()),
                    (Ok(_), Some(reason)) => RunPlugin::Unavailable(reason),
                    (Ok(plugin), None) => {
                        let unmet: Vec<ServiceId> = info
                            .needs
                            .iter()
                            .copied()
                            .filter(|service| !services.provides(service))
                            .collect();
                        if unmet.is_empty() {
                            RunPlugin::Usable {
                                info,
                                plugin: Arc::clone(plugin),
                                tools: validated(name, plugin.tools(), &mut seen),
                            }
                        } else {
                            RunPlugin::NeedsUnmet(unmet)
                        }
                    }
                };
                (name.clone(), state)
            })
            .collect();
        HostRunContext { services, packs }
    }

    /// Every usable Plugin's tools, declared by the prompt or not.
    pub(crate) fn catalog(&self) -> ToolCatalog {
        let tools = self
            .packs
            .values()
            .filter_map(|plugin| match plugin {
                RunPlugin::Usable { tools, .. } => Some(tools),
                RunPlugin::Unavailable(_) | RunPlugin::NeedsUnmet(_) => None,
            })
            .flatten()
            .cloned();
        ToolCatalog::new(tools).unwrap_or_else(|error| {
            // Every tool passed containment and uniqueness validation one
            // at a time, so this build cannot fail.
            tracing::warn!(%error, "the run's catalog failed after per-tool validation");
            ToolCatalog::default()
        })
    }

    /// The run's roster: the names of the usable Plugins, declared by the
    /// prompt or not, with or without tools, and an entry with the reason
    /// for each one that can't serve, both in name order.
    pub(crate) fn roster(&self) -> (Vec<PackId>, Vec<UnavailablePack>) {
        let mut usable = Vec::new();
        let mut unavailable = Vec::new();
        for (name, plugin) in &self.packs {
            match plugin {
                RunPlugin::Usable { .. } => usable.push(name.clone()),
                RunPlugin::Unavailable(reason) => {
                    unavailable.push(UnavailablePack::new(name.clone(), reason.clone()));
                }
                RunPlugin::NeedsUnmet(unmet) => {
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

    /// The preludes of the usable Plugins `prompt` declares, in
    /// declaration order, each under the Plugin's installed name.
    pub(crate) fn preludes(&self, prompt: &Prompt) -> Vec<Prelude> {
        prompt
            .frontmatter()
            .uses()
            .iter()
            .filter_map(|name| match self.packs.get(name) {
                Some(RunPlugin::Usable { info, .. }) => info
                    .prelude
                    .map(|source| Prelude::new(name.clone(), source)),
                _ => None,
            })
            .collect()
    }

    /// What the snapshot cannot meet for `prompt`, for each Plugin it
    /// declares: one not installed, one that failed to build, and one
    /// whose needs the run lacks.
    pub(super) fn requirements(&self, prompt: &Prompt) -> Requirements {
        let mut requirements = Requirements::default();
        for name in prompt.frontmatter().uses().iter().cloned() {
            match self.packs.get(&name) {
                None => requirements.missing_required.push(name),
                Some(RunPlugin::Unavailable(reason)) => requirements
                    .unavailable
                    .push(UnavailablePack::new(name, reason.clone())),
                Some(RunPlugin::NeedsUnmet(unmet)) => {
                    for service in unmet {
                        tracing::warn!(
                            plugin = %name,
                            %service,
                            "the prompt needs a Plugin whose service this run lacks"
                        );
                        requirements
                            .missing_services
                            .push(MissingService::new(name.clone(), service.to_string()));
                    }
                }
                Some(RunPlugin::Usable { .. }) => {}
            }
        }
        requirements
    }

    /// The usable Plugin whose snapshot offers `tool`, and that tool's
    /// info.
    fn offering(&self, tool: &ToolId) -> Option<(&Arc<dyn Plugin>, &ToolInfo)> {
        match self.packs.get(&tool.pack())? {
            RunPlugin::Usable { plugin, tools, .. } => tools
                .iter()
                .find(|info| info.id == *tool)
                .map(|info| (plugin, info)),
            RunPlugin::Unavailable(_) | RunPlugin::NeedsUnmet(_) => None,
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
        let plugin = self.offering(&tool).map(|(plugin, _)| Arc::clone(plugin));
        let services = self.services.clone();
        Box::pin(async move {
            let Some(plugin) = plugin else {
                tracing::error!(
                    tool = %tool,
                    alias = %alias,
                    "a ToolCall names a tool no usable Plugin of the run offers"
                );
                return Err(ToolError::message(format!(
                    "tool `{alias}` ({tool}) is not offered by any usable Plugin of this run"
                ))
                .with_kind(ToolErrorKind::Other));
            };
            let cx = ToolContext::new(&tool, &access, &origin, &services);
            plugin.call(cx, args).await
        })
    }

    fn survives_stop(&self, tool: &ToolId) -> bool {
        self.offering(tool)
            .is_some_and(|(_, info)| info.survives_stop)
    }
}

impl fmt::Debug for HostRunContext {
    /// Shows the run's services and each Plugin's name and state.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let packs: Vec<(&PackId, &str)> = self
            .packs
            .iter()
            .map(|(name, plugin)| {
                let state = match plugin {
                    RunPlugin::Usable { .. } => "usable",
                    RunPlugin::Unavailable(_) => "unavailable",
                    RunPlugin::NeedsUnmet(_) => "needs unmet",
                };
                (name, state)
            })
            .collect();
        f.debug_struct("HostRunContext")
            .field("services", &self.services)
            .field("plugins", &packs)
            .finish()
    }
}

/// The tools of the Plugin installed as `pack` that pass validation: a
/// tool must sit under the Plugin's name and must not repeat an id in
/// `seen`. A tool that fails is logged and dropped, so it costs only
/// itself.
fn validated(pack: &PackId, tools: Vec<ToolInfo>, seen: &mut BTreeSet<ToolId>) -> Vec<ToolInfo> {
    tools
        .into_iter()
        .filter(|tool| {
            if !pack.contains(&tool.id) {
                tracing::warn!(
                    plugin = %pack,
                    tool = %tool.id,
                    "a Plugin's tool id escapes its name; dropped"
                );
                return false;
            }
            if seen.contains(&tool.id) {
                tracing::warn!(
                    plugin = %pack,
                    tool = %tool.id,
                    "a Plugin's tool id repeats an earlier tool; dropped"
                );
                return false;
            }
            seen.insert(tool.id.clone());
            true
        })
        .collect()
}
