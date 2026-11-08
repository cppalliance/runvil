//! The Host's installed Packs: [`Packs`], which keeps the factory of
//! every Pack a [`PackRegistry`] holds, the pick of each run's Packs, the
//! instance each run makes of each of them, and the snapshot each run
//! takes of those instances.
//!
//! The Host's Plugins insert Packs into the registry, each with its
//! factory or unavailable for a reason, and every run that declares a
//! Pack inserted unavailable is refused naming that reason.
//!
//! A run's Packs are the Packs its prompt declares under `uses:`, plus
//! every `runvil` Pack whose Pack id no declaration takes, so a declared
//! `acme/web` stands in for `runvil/web` and an undeclared third-party
//! Pack is in no run and is never made. Before each run, the Harness's
//! preparation picks those Packs and makes the run's instance of each
//! with the run's own services. A factory's refusal or panic is not an
//! error: the Pack is unavailable to that run alone, with the refusal or
//! "the Pack's factory panicked" as its reason, and the next run calls
//! the factory again. The built-in `runvil/user` is made only for a run
//! whose setup has a user; any other run holds it unavailable, with the
//! reason "this run has no user". Then it waits for each instance to be
//! ready, and a Pack whose `ready` fails is unavailable to that run the
//! same way. Then it takes a snapshot of the instances: the catalog the
//! Engine offers
//! the run, the roster that says how each Pack stands, a declared Pack
//! the pick could not resolve included, the preludes of the Packs the
//! prompt declares, and the performer that sends each tool call to the
//! instance its id names. The instances drop with the snapshot, when the
//! run's future ends.
//!
//! The snapshot reads each usable Pack's tool list once, so the run's
//! catalog stays fixed for the run. A render may add tools only the model
//! sees, which no catalog lists; every render and tool call goes to the
//! Pack its id's first segment names, so a call reaches such a tool as it
//! reaches a catalog tool.

use std::any::Any;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
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
    HostServices, Pack, PackFactory, PackId, PackInfo, PackRegistry, RenderContext, ToolContext,
};
use serde_json::Value;

use crate::performers::{BoxFuture, ToolPerformer};

/// The vendor whose Packs every run holds unless a declared Pack takes
/// their id, and the vendor a bare `uses:` entry means.
const RUNVIL: &str = "runvil";

/// The reason a Pack is unavailable to a run whose call of its factory
/// panicked.
const FACTORY_PANICKED: &str = "the Pack's factory panicked";

/// The built-in a run holds only when a user answers its main chain.
const USER: &str = "runvil/user";

/// The reason [`USER`] is unavailable to a run with no user.
const NO_USER: &str = "this run has no user";

/// Every Pack a Host installed in its registry, each with its factory or
/// the reason it was inserted unavailable. Built once and shared by every
/// run of that Host; each run makes its own instance of each Pack it
/// holds.
pub struct Packs {
    installed: BTreeMap<&'static str, Installed>,
}

/// One installed Pack: its Pack id, its `PackInfo`, and its factory or
/// the reason it was inserted unavailable.
struct Installed {
    id: PackId,
    info: PackInfo,
    factory: Result<Arc<dyn PackFactory>, String>,
}

/// The Packs one run holds, picked by `Packs::pick` from its
/// prompt's `uses:` entries, and what the pick cannot resolve.
#[derive(Debug)]
pub(crate) struct Picked {
    /// The FQN of each of the run's Packs: the declared ones the Host
    /// installed, then every `runvil` Pack whose Pack id no entry takes.
    fqns: Vec<&'static str>,
    /// Each declared Pack id the Host installed no Pack for, with the
    /// reason it is unavailable, or `None` when it is simply missing.
    unresolved: BTreeMap<PackId, Option<String>>,
}

/// One of a run's Packs as the run made it: its FQN, Pack id, and
/// `PackInfo`, with the run's instance or the reason the run has none.
struct Made<'a> {
    fqn: &'static str,
    id: &'a PackId,
    info: PackInfo,
    pack: Result<Arc<dyn Pack>, String>,
}

impl Packs {
    /// Takes every Pack `packs` holds, keeping each factory for the runs
    /// to call, and warns once per Pack inserted unavailable.
    #[must_use]
    pub fn new(packs: PackRegistry) -> Packs {
        let installed = packs
            .into_entries()
            .map(|(info, id, factory)| {
                let factory = factory.map_err(|error| {
                    tracing::warn!(
                        pack = %id,
                        fqn = info.fqn,
                        %error,
                        "Pack was inserted unavailable"
                    );
                    error.to_string()
                });
                (info.fqn, Installed { id, info, factory })
            })
            .collect();
        Packs { installed }
    }

    /// Picks the Packs a run of `prompt` holds, makes
    /// the run's instance of each with `services`, the
    /// run's own, waits until each is ready or
    /// `cancel` fires, and takes the run's snapshot.
    ///
    /// A Pack whose factory refuses or panics, or whose `ready` fails, is
    /// unavailable to this run, with the reason, and so is `runvil/user`
    /// when `has_user` is false. A cancel ends the wait, and each Pack is
    /// snapshotted as it stands.
    pub(crate) async fn snapshot(
        &self,
        prompt: &Prompt,
        services: HostServices,
        has_user: bool,
        cancel: &CancelHandle,
    ) -> RunPacks {
        let picked = self.pick(prompt);
        let made = self.make(&picked, &services, has_user);
        let failures = wait_ready(&made, cancel).await;
        RunPacks::snapshot(made, failures, &picked.unresolved)
    }

    /// Picks the Packs a run of `prompt` holds: each Pack its `uses:`
    /// entries name, plus every `runvil` Pack whose Pack id no entry
    /// takes.
    ///
    /// A bare entry means the `runvil` Pack of that id. One the Host did
    /// not install is missing, or unavailable naming each other vendor's
    /// Pack of that id; a `vendor/name` the Host did not install is
    /// unavailable.
    fn pick(&self, prompt: &Prompt) -> Picked {
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
        for (fqn, installed) in &self.installed {
            let runvil = fqn
                .split_once('/')
                .is_some_and(|(vendor, _)| vendor == RUNVIL);
            if runvil && !uses.iter().any(|entry| *entry.id() == installed.id) {
                fqns.push(*fqn);
            }
        }
        Picked { fqns, unresolved }
    }

    /// Makes the run's instance of each of its `packs` with `run`, the
    /// run's own services, in pick order.
    ///
    /// A Pack inserted unavailable keeps its reason and is never made, and
    /// neither is [`USER`] when `has_user` is false: it is unavailable to
    /// the run with [`NO_USER`] as the reason. A factory that refuses
    /// leaves its Pack unavailable to this run with the refusal as the
    /// reason, logged at debug, since a Host with no operator refuses
    /// `runvil/user-input` on every run. A factory that panics leaves its
    /// Pack unavailable with [`FACTORY_PANICKED`], logged at error.
    fn make(&self, packs: &Picked, run: &HostServices, has_user: bool) -> Vec<Made<'_>> {
        packs
            .fqns
            .iter()
            .filter_map(|fqn| {
                let (&fqn, installed) = self.installed.get_key_value(fqn)?;
                let pack = match &installed.factory {
                    Err(reason) => Err(reason.clone()),
                    Ok(_) if fqn == USER && !has_user => Err(NO_USER.to_owned()),
                    Ok(factory) => new_pack(factory.as_ref(), installed, run),
                };
                Some(Made {
                    fqn,
                    id: &installed.id,
                    info: installed.info,
                    pack,
                })
            })
            .collect()
    }

    /// Why each Pack `prompt` declares can serve no run of this Host: not
    /// installed, or inserted unavailable. A bare `uses:` id means
    /// `runvil/<id>`. Empty when every one is installed with a factory.
    ///
    /// Only a run makes a Pack, so a factory that would refuse a run is no
    /// refusal here.
    pub(crate) fn refusals(&self, prompt: &Prompt) -> Vec<(PackId, String)> {
        prompt
            .frontmatter()
            .uses()
            .iter()
            .filter_map(|entry| {
                let reason = match self.find(entry) {
                    Ok((_, installed)) => installed.factory.as_ref().err()?.clone(),
                    Err(reason) => reason.unwrap_or_else(|| {
                        format!("{}/{} is not installed", entry.vendor(), entry.id())
                    }),
                };
                Some((entry.id().clone(), reason))
            })
            .collect()
    }

    /// The installed Pack the `uses:` entry `entry` names, by FQN, or why
    /// the Host installed none.
    ///
    /// A bare entry means the `runvil` Pack of that id. One the Host did
    /// not install is unavailable naming each other vendor's Pack of that
    /// id, or simply missing, with no reason, when no vendor has one; a
    /// `vendor/name` the Host did not install is unavailable.
    fn find(&self, entry: &PackRef) -> Result<(&'static str, &Installed), Option<String>> {
        let wanted = format!("{}/{}", entry.vendor(), entry.id());
        if let Some((fqn, installed)) = self.installed.get_key_value(wanted.as_str()) {
            return Ok((*fqn, installed));
        }
        if entry.vendor() != RUNVIL {
            return Err(Some(format!("{wanted} is not installed")));
        }
        let others: Vec<&str> = self
            .installed
            .iter()
            .filter(|(_, installed)| installed.id == *entry.id())
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
}

impl fmt::Debug for Packs {
    /// Shows each installed Pack's FQN and Pack id, and whether it was
    /// inserted unavailable.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let installed: Vec<(&str, &PackId, &str)> = self
            .installed
            .iter()
            .map(|(fqn, installed)| {
                let state = if installed.factory.is_ok() {
                    "installed"
                } else {
                    "unavailable"
                };
                (*fqn, &installed.id, state)
            })
            .collect();
        f.debug_struct("Packs")
            .field("installed", &installed)
            .finish()
    }
}

/// Calls `factory` for the run's instance of the Pack `installed` names,
/// with `run`, the run's own services, and returns the instance or the
/// reason the run has none. A refusal is logged at debug, and a panic is
/// caught and logged at error.
fn new_pack(
    factory: &dyn PackFactory,
    installed: &Installed,
    run: &HostServices,
) -> Result<Arc<dyn Pack>, String> {
    let id = &installed.id;
    match catch_unwind(AssertUnwindSafe(|| factory.new_pack(id, run))) {
        Ok(Ok(pack)) => Ok(pack),
        Ok(Err(error)) => {
            tracing::debug!(
                pack = %id,
                fqn = installed.info.fqn,
                %error,
                "Pack factory refused this run; the Pack is unavailable to it"
            );
            Err(error.to_string())
        }
        Err(panic) => {
            tracing::error!(
                pack = %id,
                fqn = installed.info.fqn,
                panic = panic_message(&*panic),
                "Pack factory panicked; the Pack is unavailable to this run"
            );
            Err(FACTORY_PANICKED.to_owned())
        }
    }
}

/// A panic payload's message, when it carries one.
fn panic_message(payload: &(dyn Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("a panic with no message")
}

/// Waits until the `ready` of each instance in `made` resolves, polling
/// them together, and returns the reason of each one that will not be
/// ready, by FQN. A Pack the run could not make is never waited on.
///
/// A cancel on `cancel` ends the wait with no failures, so each Pack is
/// snapshotted as it stands, and a run canceled here ends canceled
/// whatever the snapshot holds.
async fn wait_ready(made: &[Made<'_>], cancel: &CancelHandle) -> BTreeMap<&'static str, String> {
    let waits = made.iter().filter_map(|made| {
        let pack = made.pack.as_ref().ok()?;
        Some(async move { (made.fqn, pack.ready().await) })
    });
    let joined = pin!(join_all(waits));
    // The cancel arm is polled first, so a run canceled already does not
    // wait.
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

/// One run's frozen snapshot of its Pack instances, by Pack id, and its
/// one tool catalog. It performs the run's renders and tool calls, and
/// the instances drop with it.
pub(crate) struct RunPacks {
    packs: BTreeMap<PackId, PackState>,
    catalog: ToolCatalog,
}

/// What one of the run's Packs looks like to it.
enum PackState {
    /// It can serve; its validated tools are in the run's catalog.
    Usable { info: PackInfo, pack: Arc<dyn Pack> },
    /// The pick could not resolve it, or it was inserted unavailable, or
    /// its factory refused or panicked, or its `ready` failed, for this
    /// reason.
    Unavailable(String),
}

impl RunPacks {
    /// Takes the snapshot of the run's `made` Packs, which share no Pack
    /// id.
    ///
    /// `failures` holds the reason of each Pack whose `ready` failed, by
    /// FQN, which the run treats as unavailable, as it does a Pack it has
    /// no instance of. Every other Pack is usable, and only its tool list
    /// is read and validated. The run's one catalog is built from every
    /// usable Pack's validated tools, in Pack id order. Each declared Pack
    /// id the pick could not resolve that `unresolved` gives a reason for
    /// is unavailable with that reason; one with no reason stays out, so
    /// the run finds it missing. The roster names each of the run's Packs
    /// with the reason of each one that can't serve, and the declared
    /// preludes are the declared, usable Packs', in declaration order.
    fn snapshot(
        made: Vec<Made<'_>>,
        mut failures: BTreeMap<&'static str, String>,
        unresolved: &BTreeMap<PackId, Option<String>>,
    ) -> RunPacks {
        let mut seen = BTreeSet::new();
        // The input arrives in pick order, declared Packs first, so the
        // catalog is built after `made` is collected, from these lists in
        // Pack id order.
        let mut usable_tools: BTreeMap<&PackId, Vec<ToolInfo>> = BTreeMap::new();
        let mut packs: BTreeMap<PackId, PackState> = made
            .into_iter()
            .map(|made| {
                let state = match (made.pack, failures.remove(made.fqn)) {
                    (Err(reason), _) | (Ok(_), Some(reason)) => PackState::Unavailable(reason),
                    (Ok(pack), None) => {
                        usable_tools.insert(made.id, validated(made.id, pack.tools(), &mut seen));
                        PackState::Usable {
                            info: made.info,
                            pack,
                        }
                    }
                };
                (made.id.clone(), state)
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
        RunPacks { packs, catalog }
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
            PackState::Unavailable(_) => None,
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
            let cx = ToolContext::new(&tool, &access, &origin);
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
    /// Shows each Pack's name and state, and the catalog's tool ids.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let packs: Vec<(&PackId, &str)> = self
            .packs
            .iter()
            .map(|(name, pack)| {
                let state = match pack {
                    PackState::Usable { .. } => "usable",
                    PackState::Unavailable(_) => "unavailable",
                };
                (name, state)
            })
            .collect();
        f.debug_struct("RunPacks")
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
