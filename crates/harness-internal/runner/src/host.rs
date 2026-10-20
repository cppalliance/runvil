//! The Host's built Packs: [`HostContext`], built once at startup from
//! every Pack a [`PackRegistry`] holds and shared by every run, the pick
//! of each run's Packs, and the snapshot each run takes of them.
//!
//! The Host's Plugins add Packs to the registry, and building the
//! context calls each Pack's `construct` once, with the config its Plugin
//! added it with, to build the one Pack object every run shares. A
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
//! offers the run, the preludes of the Packs the prompt declares, the
//! requirements the pick and the snapshot cannot meet, and the performer
//! that sends each tool call to the Pack its id names.

use std::collections::BTreeMap;
use std::fmt;
use std::pin::pin;
use std::sync::Arc;

use futures_util::future::{Either, join_all, select};
use runvil::cancel::CancelHandle;
use runvil::{Prompt, Requirements};
use runvil_plugin::{HostServices, Pack, PackId, PackInfo, PackRegistry};

#[path = "host-run.rs"]
mod run;

pub(crate) use run::HostRunContext;

#[cfg(test)]
#[path = "host-tests.rs"]
mod tests;

/// The vendor whose Packs every run holds unless a declared Pack takes
/// their id, and the vendor a bare `uses:` entry means.
const RUNVIL: &str = "runvil";

/// Every Pack a Host built and the Host-wide services their `construct`
/// functions read.
///
/// A Host builds one at startup from the registry its Plugins filled and
/// shares it with every Harness behind an `Arc`. Dropping it drops the
/// Pack objects, which is where a Pack cleans up.
pub struct HostContext {
    services: HostServices,
    built: BTreeMap<&'static str, Built>,
}

/// One built Pack: its Pack id, its `PackInfo`, and the built object or
/// the reason building it failed.
struct Built {
    id: PackId,
    info: PackInfo,
    pack: Result<Arc<dyn Pack>, String>,
}

/// The Packs one run holds, picked by [`HostContext::pick`] from its
/// prompt's `uses:` entries, and what the pick cannot resolve.
#[derive(Debug)]
pub(crate) struct RunPacks {
    /// The FQN of each of the run's Packs: the declared ones the Host
    /// built, then every `runvil` Pack whose Pack id no entry takes.
    fqns: Vec<&'static str>,
    /// Each declared Pack id the Host built no Pack for, with the reason
    /// it is unavailable, or `None` when it is simply missing.
    unresolved: BTreeMap<PackId, Option<String>>,
}

impl HostContext {
    /// Builds every Pack `packs` holds, the built-ins included, handing
    /// each `construct` its Pack id, its stored config, and `services`.
    ///
    /// `services` are Host-wide, such as a search provider. They reach
    /// only `construct`; a tool call reads the run's own services instead.
    /// A `construct` failure is not an error here: the Pack is stored as
    /// unavailable, with the failure as the reason a run that needs it is
    /// refused.
    #[must_use]
    pub fn new(packs: PackRegistry, services: HostServices) -> HostContext {
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
        HostContext { services, built }
    }

    /// Picks the Packs a run of `prompt` holds: each Pack its `uses:`
    /// entries name, plus every `runvil` Pack whose Pack id no entry
    /// takes.
    ///
    /// A bare entry means the `runvil` Pack of that id. One the Host did
    /// not build is missing, or unavailable naming each other vendor's
    /// Pack of that id; a `vendor/name` the Host did not build is
    /// unavailable.
    pub(crate) fn pick(&self, prompt: &Prompt) -> RunPacks {
        let uses = prompt.frontmatter().uses();
        let mut fqns = Vec::new();
        let mut unresolved = BTreeMap::new();
        for entry in uses {
            let wanted = format!("{}/{}", entry.vendor(), entry.id());
            if let Some((fqn, _)) = self.built.get_key_value(wanted.as_str()) {
                fqns.push(*fqn);
                continue;
            }
            let reason = if entry.vendor() == RUNVIL {
                let others: Vec<&str> = self
                    .built
                    .iter()
                    .filter(|(_, built)| built.id == *entry.id())
                    .map(|(fqn, _)| *fqn)
                    .collect();
                (!others.is_empty()).then(|| {
                    format!(
                        "{id} means {wanted}, which is not installed; write {} to use that Pack",
                        others.join(" or "),
                        id = entry.id(),
                    )
                })
            } else {
                Some(format!("{wanted} is not installed"))
            };
            unresolved.insert(entry.id().clone(), reason);
        }
        for (fqn, built) in &self.built {
            let runvil = fqn
                .split_once('/')
                .is_some_and(|(vendor, _)| vendor == RUNVIL);
            if runvil && !uses.iter().any(|entry| *entry.id() == built.id) {
                fqns.push(*fqn);
            }
        }
        RunPacks { fqns, unresolved }
    }

    /// Waits until the `ready` of each of the run's `packs` that built
    /// resolves, polling them together, and returns the reason of each
    /// one that will not be ready, by FQN.
    ///
    /// A cancel on `cancel` ends the wait with no failures, so each Pack
    /// is snapshotted as it stands, and a run cancelled here ends
    /// cancelled whatever the snapshot holds.
    pub(crate) async fn wait_ready(
        &self,
        packs: &RunPacks,
        cancel: &CancelHandle,
    ) -> BTreeMap<&'static str, String> {
        let waits = packs.fqns.iter().filter_map(|fqn| {
            let (fqn, built) = self.built.get_key_value(fqn)?;
            let pack = built.pack.as_ref().ok()?;
            Some(async move { (*fqn, pack.ready().await) })
        });
        let joined = pin!(join_all(waits));
        // The cancel arm is polled first, so a run cancelled already does
        // not wait.
        match select(cancel.cancelled(), joined).await {
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
    /// own services, and reports the requirements it cannot meet for
    /// `prompt`.
    ///
    /// `failures` holds the reason of each Pack whose `ready` failed
    /// before this run, by FQN, which the run treats as unavailable. The
    /// snapshot gives the run's catalog, which holds the tools of each of
    /// its usable Packs, its roster, which names each of its Packs with
    /// the reason of each one that can't serve, and its declared preludes,
    /// the declared, usable Packs', in declaration order. It also performs
    /// the run's tool calls. The requirements cover every Pack the prompt
    /// declares, the ones the pick could not resolve included.
    pub(crate) fn begin_run(
        &self,
        packs: &RunPacks,
        services: HostServices,
        failures: BTreeMap<&'static str, String>,
        prompt: &Prompt,
    ) -> (HostRunContext, Requirements) {
        let run = HostRunContext::snapshot(
            packs
                .fqns
                .iter()
                .filter_map(|fqn| self.built.get(fqn))
                .map(|built| (&built.id, built.info, &built.pack)),
            services,
            failures,
        );
        let requirements = run.requirements(prompt, &packs.unresolved);
        (run, requirements)
    }
}

impl fmt::Debug for HostContext {
    /// Shows the services and each built Pack's FQN and Pack id with
    /// whether it built, and never a Pack's config.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let built: Vec<(&str, &PackId, bool)> = self
            .built
            .iter()
            .map(|(fqn, built)| (*fqn, &built.id, built.pack.is_ok()))
            .collect();
        f.debug_struct("HostContext")
            .field("services", &self.services)
            .field("built", &built)
            .finish()
    }
}
