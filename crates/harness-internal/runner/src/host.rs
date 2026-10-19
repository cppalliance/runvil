//! The Host's installed Plugins: [`HostContext`], built once at startup
//! over a [`PluginRegistry`] and shared by every run, and the snapshot
//! each run takes of it.
//!
//! A Host installs a Plugin by the FQN the registry holds, under a Plugin
//! id, and install calls the Plugin's `construct` once to build the one
//! Plugin object every run shares. A construct failure is not an install
//! error: the Plugin is stored as unavailable with the failure as its
//! reason, and every run that needs it is refused naming that reason.
//! Before each run, the Harness's preparation waits for every built Plugin
//! to be ready, and a Plugin whose `ready` fails is unavailable to that run
//! the same way.
//! Then it takes a snapshot of every installed Plugin with the run's own
//! services: the catalog the Engine offers the run, the preludes of
//! the Plugins the prompt declares, the requirements the snapshot cannot
//! meet, and the performer that sends each tool call to the Plugin its id
//! names.

use std::collections::BTreeMap;
use std::fmt;
use std::pin::pin;
use std::sync::Arc;

use futures_util::future::{Either, join_all, select};
use runvil::cancel::CancelHandle;
use runvil::{Prompt, Requirements};
use runvil_plugin::{HostServices, Plugin, PluginId, PluginInfo};

use crate::registry::{PluginKind, PluginRegistry};

#[path = "host-run.rs"]
mod run;

pub(crate) use run::HostRunContext;

#[cfg(test)]
#[path = "host-tests.rs"]
mod tests;

/// Every Plugin a Host installed, the registry it installed them from,
/// and the Host-wide services their `construct` functions read.
///
/// A Host builds one at startup, installs its Plugins, and shares it with
/// every Harness behind an `Arc`. Dropping it drops the Plugin objects,
/// which is where a Plugin cleans up.
pub struct HostContext {
    plugins: PluginRegistry,
    services: HostServices,
    installed: Vec<Installed>,
}

/// One installed Plugin: its Plugin id, its `PluginInfo`, and the built
/// object or the reason building it failed.
struct Installed {
    name: PluginId,
    info: PluginInfo,
    plugin: Result<Arc<dyn Plugin>, String>,
}

impl HostContext {
    /// A context with no Plugins, which installs from the Plugins
    /// `plugins` holds and hands `services` to each Plugin's `construct`.
    ///
    /// `services` are Host-wide, such as a search provider. They reach
    /// only `construct`; a tool call reads the run's own services instead.
    #[must_use]
    pub fn new(plugins: PluginRegistry, services: HostServices) -> HostContext {
        HostContext {
            plugins,
            services,
            installed: Vec::new(),
        }
    }

    /// Builds the Plugin the registry holds as `fqn` and adds it under
    /// `name`, or under the FQN's second segment when `name` is `None`,
    /// and returns the Plugin id it used.
    ///
    /// `construct` runs once, here, with the Plugin id, `config`, and the
    /// Host-wide services. A `construct` failure is not an error here: the
    /// Plugin is stored as unavailable, with the failure as the reason a
    /// run that needs it is refused.
    ///
    /// # Errors
    /// Returns [`InstallError::UnknownPlugin`] when the registry does not
    /// hold `fqn`, [`InstallError::ReservedName`] when a built-in is asked
    /// for under a Plugin id other than its own or another Plugin under a
    /// built-in's Plugin id, and [`InstallError::NameTaken`] when the
    /// Plugin id is installed already or differs from an installed one
    /// only by `-`, `_`, or `.`.
    pub fn install(
        &mut self,
        fqn: &str,
        name: Option<PluginId>,
        config: serde_json::Value,
    ) -> Result<PluginId, InstallError> {
        let Some((info, kind, default)) = self.plugins.entry(fqn) else {
            return Err(InstallError::UnknownPlugin {
                fqn: fqn.to_owned(),
            });
        };
        let name = name.unwrap_or_else(|| default.clone());
        let reserved = match kind {
            PluginKind::Builtin => name != *default,
            PluginKind::Linked => self.plugins.reserves(&name),
        };
        if reserved {
            return Err(InstallError::ReservedName {
                fqn: info.fqn,
                name,
            });
        }
        let twin = normalize(&name);
        if let Some(existing) = self
            .installed
            .iter()
            .find(|installed| normalize(&installed.name) == twin)
        {
            return Err(InstallError::NameTaken {
                name,
                existing: existing.name.clone(),
            });
        }
        let plugin = (info.construct)(&name, config, &self.services).map_err(|error| {
            tracing::warn!(
                plugin = %name,
                fqn = info.fqn,
                %error,
                "Plugin construct failed; it is installed as unavailable"
            );
            error.to_string()
        });
        self.installed.push(Installed {
            name: name.clone(),
            info,
            plugin,
        });
        Ok(name)
    }

    /// Waits until every built Plugin's `ready` resolves, polling them
    /// together, and returns the reason of each one that will not be
    /// ready, by name.
    ///
    /// A cancel on `cancel` ends the wait with no failures, so each Plugin
    /// is snapshotted as it stands, and a run cancelled here ends
    /// cancelled whatever the snapshot holds.
    pub(crate) async fn wait_ready(&self, cancel: &CancelHandle) -> BTreeMap<PluginId, String> {
        let waits = self.installed.iter().filter_map(|installed| {
            let plugin = installed.plugin.as_ref().ok()?;
            Some(async move { (&installed.name, plugin.ready().await) })
        });
        let joined = pin!(join_all(waits));
        // The cancel arm is polled first, so a run cancelled already does
        // not wait.
        match select(cancel.cancelled(), joined).await {
            Either::Left(((), _)) => BTreeMap::new(),
            Either::Right((results, _)) => results
                .into_iter()
                .filter_map(|(name, result)| {
                    let error = result.err()?;
                    tracing::warn!(
                        plugin = %name,
                        %error,
                        "Plugin will not be ready; this run treats it as unavailable"
                    );
                    Some((name.clone(), error.to_string()))
                })
                .collect(),
        }
    }

    /// Takes one run's snapshot of every installed Plugin with `services`,
    /// the run's own services, and reports the requirements it cannot meet
    /// for `prompt`.
    ///
    /// `failures` holds the reason of each Plugin whose `ready` failed
    /// before this run, which the run treats as unavailable. The snapshot
    /// gives the run's catalog, which holds every usable Plugin's tools,
    /// declared or not, its roster, which names every installed Plugin with
    /// the reason of each one that can't serve, and its declared preludes,
    /// the declared, usable Plugins', in declaration order. It also performs
    /// the run's tool calls. The requirements cover every Plugin the prompt
    /// declares.
    pub(crate) fn begin_run(
        &self,
        services: HostServices,
        failures: BTreeMap<PluginId, String>,
        prompt: &Prompt,
    ) -> (HostRunContext, Requirements) {
        let run = HostRunContext::snapshot(
            self.installed
                .iter()
                .map(|installed| (&installed.name, installed.info, &installed.plugin)),
            services,
            failures,
        );
        let requirements = run.requirements(prompt);
        (run, requirements)
    }
}

impl fmt::Debug for HostContext {
    /// Shows the services, each installed name with whether it built, and
    /// the registry.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let installed: Vec<(&PluginId, bool)> = self
            .installed
            .iter()
            .map(|installed| (&installed.name, installed.plugin.is_ok()))
            .collect();
        f.debug_struct("HostContext")
            .field("services", &self.services)
            .field("installed", &installed)
            .field("plugins", &self.plugins)
            .finish()
    }
}

/// A mistake [`PluginRegistry::add`] or [`HostContext::install`] refuses
/// outright, so the Host's author sees it at startup.
///
/// A Plugin whose `construct` fails is not one of these; it is installed
/// as unavailable instead.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum InstallError {
    /// The added Plugin's FQN is not `vendor/name`, or a segment of it is
    /// not a valid one-segment name.
    #[error("Plugin fully qualified name `{fqn}` is not a vendor/name pair")]
    InvalidFqn {
        /// The FQN as its `PluginInfo` spells it.
        fqn: &'static str,
    },
    /// The registry holds the added Plugin's FQN already, as a built-in or
    /// as a Plugin added before.
    #[error("Plugin `{fqn}` is already in the registry")]
    DuplicatePlugin {
        /// The FQN as its `PluginInfo` spells it.
        fqn: &'static str,
    },
    /// The registry holds no Plugin with the FQN the install asked for.
    #[error(
        "Plugin `{fqn}` is not in the registry; add it with PluginRegistry::add before building the HostContext"
    )]
    UnknownPlugin {
        /// The FQN the install asked for.
        fqn: String,
    },
    /// A built-in was asked for under a Plugin id other than its own, or
    /// another Plugin under a built-in's Plugin id.
    #[error(
        "Plugin `{fqn}` cannot be installed as {name}: a built-in Plugin keeps its own Plugin id, and no other Plugin may take it"
    )]
    ReservedName {
        /// The FQN as its `PluginInfo` spells it.
        fqn: &'static str,
        /// The Plugin id the install asked for.
        name: PluginId,
    },
    /// The Plugin id is installed already (`existing` equals `name`), or
    /// it differs from the installed `existing` only by `-`, `_`, or `.`,
    /// as `user-input` and `user_input` do, which a model reading the tool
    /// list cannot tell apart.
    #[error(
        "Plugin id {name} is taken by the installed {existing}; install it under another Plugin id"
    )]
    NameTaken {
        /// The Plugin id the install asked for.
        name: PluginId,
        /// The installed Plugin id it collides with.
        existing: PluginId,
    },
}

/// A name with each `-`, `_`, and `.` mapped to `-`, so two names that
/// differ only by that punctuation compare equal. Names are lowercase
/// only, so case needs no handling.
fn normalize(name: &PluginId) -> String {
    name.to_string()
        .chars()
        .map(|c| if matches!(c, '-' | '_' | '.') { '-' } else { c })
        .collect()
}
