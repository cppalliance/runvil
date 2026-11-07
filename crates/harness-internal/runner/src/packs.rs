//! The Host's built Packs: [`Packs`], built once at startup from every
//! Pack a [`PackRegistry`] holds and shared by every run, the pick of
//! each run's Packs, and the snapshot each run takes of them.
//!
//! The Host's Plugins insert Packs into the registry, and building the
//! Packs calls each Pack's `construct` once, with the config its Plugin
//! inserted it with, to build the one Pack object every run shares. A
//! construct failure is not an error: the Pack is stored as unavailable
//! with the failure as its reason, and every run that needs it is refused
//! naming that reason.
//!
//! A run's Packs are the Packs its prompt declares under `uses:`, plus
//! every `runvil` Pack whose Pack id no declaration takes, so a declared
//! `acme/web` stands in for `runvil/web` and an undeclared third-party
//! Pack is in no run. Before each run, the Harness's preparation picks
//! those Packs and waits for each of them to be ready, and a Pack whose
//! `ready` fails is unavailable to that run the same way. Then it takes a
//! snapshot of them with the run's own services: the catalog the Engine
//! offers the run, the roster that says how each Pack stands, a declared
//! Pack the pick could not resolve included, the preludes of the Packs
//! the prompt declares, and the performer that sends each tool call to
//! the Pack its id names.
//!
//! The snapshot reads each usable Pack's tool list once, so the run's
//! catalog stays fixed for the run. A render may add tools only the model
//! sees, which no catalog lists; every render and tool call goes to the
//! Pack its id's first segment names, so a call reaches such a tool as it
//! reaches a catalog tool.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::pin::pin;
use std::sync::Arc;

use futures_util::future::{Either, join_all, select};
use runvil::Prompt;
use runvil::cancel::CancelHandle;
use runvil::effect::{PackToolSets, ToolCallOrigin};
use runvil::model::ModelInfo;
use runvil::packs::{PackRef, Prelude, RosterEntry};
use runvil::vfs::Access;
use runvil::{Error, ErrorCode};
use runvil::{ToolCatalog, ToolId, ToolInfo, ToolOutput, make_tool_error};
use runvil_plugin::{
    HostServices, Pack, PackId, PackInfo, PackRegistry, RenderContext, ServiceId, ToolContext,
};
use serde_json::Value;

use crate::performers::{BoxFuture, ToolPerformer};

/// The vendor whose Packs every run holds unless a declared Pack takes
/// their id, and the vendor a bare `uses:` entry means.
const RUNVIL: &str = "runvil";

/// Every Pack a Host built from its registry. Built once and shared by
/// every run of that Host; the Pack objects drop with it, which is where
/// a Pack cleans up.
pub struct Packs {
    built: BTreeMap<&'static str, Built>,
}

/// One built Pack: its Pack id, its `PackInfo`, and the built object or
/// the reason building it failed.
struct Built {
    id: PackId,
    info: PackInfo,
    pack: Result<Arc<dyn Pack>, String>,
}

/// The Packs one run holds, picked by [`Packs::pick`] from its
/// prompt's `uses:` entries, and what the pick cannot resolve.
#[derive(Debug)]
pub(crate) struct Picked {
    /// The FQN of each of the run's Packs: the declared ones the Host
    /// built, then every `runvil` Pack whose Pack id no entry takes.
    fqns: Vec<&'static str>,
    /// Each declared Pack id the Host built no Pack for, with the reason
    /// it is unavailable, or `None` when it is simply missing.
    unresolved: BTreeMap<PackId, Option<String>>,
}

impl Packs {
    /// Builds every Pack `packs` holds, the built-ins included, handing
    /// each `construct` its Pack id, its stored config, and the Host-wide
    /// `services`. A failed construct leaves that Pack unavailable, with
    /// the failure as its reason.
    ///
    /// `services` are Host-wide, such as a search provider. They reach
    /// only `construct`; a tool call reads the run's own services instead.
    #[must_use]
    #[expect(
        clippy::needless_pass_by_value,
        reason = "the public signature takes the Host-wide services by value"
    )]
    pub fn new(packs: PackRegistry, services: HostServices) -> Packs {
        let built = packs
            .into_entries()
            .map(|(info, id, config)| {
                let pack = (info.construct)(&id, config, &services).map_err(|error| {
                    tracing::warn!(
                        pack = %id,
                        fqn = info.fqn,
                        %error,
                        "Pack construct failed; it is built as unavailable"
                    );
                    error.to_string()
                });
                (info.fqn, Built { id, info, pack })
            })
            .collect();
        Packs { built }
    }

    /// Picks the Packs a run of `prompt` holds: each Pack its `uses:`
    /// entries name, plus every `runvil` Pack whose Pack id no entry
    /// takes.
    ///
    /// A bare entry means the `runvil` Pack of that id. One the Host did
    /// not build is missing, or unavailable naming each other vendor's
    /// Pack of that id; a `vendor/name` the Host did not build is
    /// unavailable.
    pub(crate) fn pick(&self, prompt: &Prompt) -> Picked {
        let uses = prompt.frontmatter().uses();
        let mut fqns = Vec::new();
        let mut unresolved = BTreeMap::new();
        for entry in uses {
            match self.find(entry) {
                Ok((fqn, _)) => fqns.push(fqn),
                Err(reason) => {
                    unresolved.insert(entry.id().clone(), reason);
                }
            }
        }
        for (fqn, built) in &self.built {
            let runvil = fqn
                .split_once('/')
                .is_some_and(|(vendor, _)| vendor == RUNVIL);
            if runvil && !uses.iter().any(|entry| *entry.id() == built.id) {
                fqns.push(*fqn);
            }
        }
        Picked { fqns, unresolved }
    }

    /// Why each Pack `prompt` declares can serve no run of this Host: not
    /// installed, or its construct failed. A bare `uses:` id means
    /// `runvil/<id>`. Empty when every one built.
    pub(crate) fn refusals(&self, prompt: &Prompt) -> Vec<(PackId, String)> {
        prompt
            .frontmatter()
            .uses()
            .iter()
            .filter_map(|entry| {
                let reason = match self.find(entry) {
                    Ok((_, built)) => built.pack.as_ref().err()?.clone(),
                    Err(reason) => reason.unwrap_or_else(|| {
                        format!("{}/{} is not installed", entry.vendor(), entry.id())
                    }),
                };
                Some((entry.id().clone(), reason))
            })
            .collect()
    }

    /// The built Pack the `uses:` entry `entry` names, by FQN, or why the
    /// Host built none.
    ///
    /// A bare entry means the `runvil` Pack of that id. One the Host did
    /// not build is unavailable naming each other vendor's Pack of that
    /// id, or simply missing, with no reason, when no vendor has one; a
    /// `vendor/name` the Host did not build is unavailable.
    fn find(&self, entry: &PackRef) -> Result<(&'static str, &Built), Option<String>> {
        let wanted = format!("{}/{}", entry.vendor(), entry.id());
        if let Some((fqn, built)) = self.built.get_key_value(wanted.as_str()) {
            return Ok((*fqn, built));
        }
        if entry.vendor() != RUNVIL {
            return Err(Some(format!("{wanted} is not installed")));
        }
        let others: Vec<&str> = self
            .built
            .iter()
            .filter(|(_, built)| built.id == *entry.id())
            .map(|(fqn, _)| *fqn)
            .collect();
        Err((!others.is_empty()).then(|| {
            format!(
                "{id} means {wanted}, which is not installed; write {} to use that Pack",
                others.join(" or "),
                id = entry.id(),
            )
        }))
    }

    /// Waits until the `ready` of each of the run's `packs` that built
    /// resolves, polling them together, and returns the reason of each
    /// one that will not be ready, by FQN.
    ///
    /// A cancel on `cancel` ends the wait with no failures, so each Pack
    /// is snapshotted as it stands, and a run canceled here ends
    /// canceled whatever the snapshot holds.
    pub(crate) async fn wait_ready(
        &self,
        packs: &Picked,
        cancel: &CancelHandle,
    ) -> BTreeMap<&'static str, String> {
        let waits = packs.fqns.iter().filter_map(|fqn| {
            let (fqn, built) = self.built.get_key_value(fqn)?;
            let pack = built.pack.as_ref().ok()?;
            Some(async move { (*fqn, pack.ready().await) })
        });
        let joined = pin!(join_all(waits));
        // The cancel arm is polled first, so a run canceled already does
        // not wait.
        match select(cancel.canceled(), joined).await {
            Either::Left(((), _)) => BTreeMap::new(),
            Either::Right((results, _)) => results
                .into_iter()
                .filter_map(|(fqn, result)| {
                    let error = result.err()?;
                    tracing::warn!(
                        fqn,
                        %error,
                        "Pack will not be ready; this run treats it as unavailable"
                    );
                    Some((fqn, error.to_string()))
                })
                .collect(),
        }
    }

    /// Takes one run's snapshot of its `packs` with `services`, the run's
    /// own services.
    ///
    /// `failures` holds the reason of each Pack whose `ready` failed
    /// before this run, by FQN, which the run treats as unavailable. The
    /// snapshot gives the run's catalog, which holds the tools of each of
    /// its usable Packs, its roster, which names each of its Packs with
    /// the reason of each one that can't serve, a declared Pack the pick
    /// could not resolve with a reason included, and its declared
    /// preludes, the declared, usable Packs', in declaration order. It also
    /// performs the run's tool calls.
    pub(crate) fn begin_run(
        &self,
        packs: &Picked,
        services: HostServices,
        failures: BTreeMap<&'static str, String>,
    ) -> RunPacks {
        RunPacks::snapshot(
            packs
                .fqns
                .iter()
                .filter_map(|fqn| self.built.get(fqn))
                .map(|built| (&built.id, built.info, &built.pack)),
            services,
            failures,
            &packs.unresolved,
        )
    }
}

impl fmt::Debug for Packs {
    /// Shows each built Pack's FQN and Pack id with whether it built, and
    /// never a Pack's config.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let built: Vec<(&str, &PackId, bool)> = self
            .built
            .iter()
            .map(|(fqn, built)| (*fqn, &built.id, built.pack.is_ok()))
            .collect();
        f.debug_struct("Packs").field("built", &built).finish()
    }
}

/// One run's frozen snapshot of its Packs, by Pack id, its one tool
/// catalog, and the run's own services. It performs the run's renders and
/// tool calls.
pub(crate) struct RunPacks {
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

impl RunPacks {
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
    fn snapshot<'a>(
        packs: impl Iterator<Item = (&'a PackId, PackInfo, &'a Result<Arc<dyn Pack>, String>)>,
        services: HostServices,
        mut failures: BTreeMap<&'static str, String>,
        unresolved: &BTreeMap<PackId, Option<String>>,
    ) -> RunPacks {
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
        RunPacks {
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

impl ToolPerformer for RunPacks {
    fn call(
        &self,
        tool: ToolId,
        alias: String,
        access: Arc<Access>,
        origin: ToolCallOrigin,
        args: Value,
    ) -> BoxFuture<Result<ToolOutput, Error>> {
        let pack = self.usable(&tool.pack()).cloned();
        let services = self.services.clone();
        Box::pin(async move {
            let Some(pack) = pack else {
                tracing::error!(
                    tool = %tool,
                    alias = %alias,
                    "a ToolCall names a tool of no usable Pack of the run"
                );
                return Err(make_tool_error(
                    ErrorCode::ToolOther,
                    format!("tool `{alias}` ({tool}) belongs to no usable Pack of this run"),
                ));
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

impl fmt::Debug for RunPacks {
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
        f.debug_struct("RunPacks")
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

#[cfg(test)]
#[path = "packs-tests.rs"]
mod tests;
