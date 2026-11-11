//! The agent-sessions subsystem of the server: the `/agents/ws`
//! agent-session socket ([`socket`]) and its frames ([`wire`]), the
//! `/v1/models` catalog relay ([`relay`]), their shared route state
//! ([`state`]), and [`AgentSessions`], the server's launcher of agent
//! conversations. The `/ws` workshop socket sits outside this subsystem;
//! the two share only [`crate::websocket`].
//!
//! The server builds one [`Host`] at startup: its `Packs`, installed from
//! the linked web and mcp Plugins, mcp over the servers of the configured
//! `mcp.json`, the library Plugins in the configured Plugin directory, the
//! linked std Plugin when no library Plugin inserted the file-tool Pack,
//! and the built-ins subagents and user; a finder that reads
//! each agent from the agents folder through `load_agent`; and the tokio
//! timer. Each launch opens a conversation in `workshop-agents` and builds
//! that conversation's one run its own [`RunSetup`]: the conversation's
//! recorder tee on a handle of the run log's [`TursoRecorder`] that names
//! the launched agent, a per-run broker over the server's inference broker
//! ([`broker`]), and a user, the conversation's operator, so the run holds
//! `runvil/user`; the operator's messages reach the run's main chain
//! through the conversation. The run makes its own instance of each Pack
//! it holds and drops them when it ends. The run reaches the gateway only
//! through its per-run broker, which follows the live gateway binding and
//! sends each round to the dropdown's current pick. Each run's filesystem
//! is an in-memory root, with the language guide demo's read-only mount
//! beside it while that demo's switch is on (`language_guide`). The run's
//! setup carries the Host state ([`bindings`]): the menu's pick and the
//! workspace's granted roots, read once the menu's catalog holds a
//! chat-capable model. Status-bar reporting stays in the server
//! (`status`): a per-conversation reporter derives it from the
//! conversation's events, deltas, and failure reports.
//!
//! **Registry carve-out.** Conversations survive socket disconnect and
//! sockets attach and detach (`socket`), so the launcher keeps the
//! conversation table the server's socket rule otherwise forbids. The rule
//! governed per-request relay work, where every held resource belonged to
//! one socket; a conversation is longer-lived than any socket on purpose.

mod bindings;
mod broker;
mod gateway;
mod language_guide;
mod relay;
mod search;
mod socket;
mod socket_frames;
mod state;
mod status;
mod wire;

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use plugin_web::SEARCH_PROVIDER;
use runvil::harness::{Packs, PluginReport, RunId, insert_plugin_dir};
use runvil::plugin::{HostServices, PackRegistry, ServiceKey};
use runvil::{Error, ErrorCode, ErrorFact, Host, RunSetup};
use serde_json::{Map, Value};
use workshop_agents::{
    Conversation, ConversationId, Conversations, LaunchError, TokioTimer, discover_agents,
    load_agent,
};
use workshop_registry::Registry;
use workshop_run_log::TursoRecorder;
use workshop_support::{Config, ReconnectBackoff};

use bindings::with_host_state;
use broker::{RunBroker, WorkshopBroker};
use gateway::usable_gateway;
use search::GatewaySearchProvider;
use socket::Receivers;
pub(crate) use state::{SessionsState, register};

/// The directory under the server's state directory the run log sits in:
/// the `runs.db` every conversation's run is recorded in.
const HARNESS_STATE_DIR: &str = "harness";

/// The key the server provides its runtime under: the Plugin contract's
/// tokio runtime service, as the handle it supplies.
const RUNTIME: ServiceKey<tokio::runtime::Handle> = ServiceKey::new(runvil::plugin::TOKIO_RUNTIME);

/// The server's installed Packs over its Host-wide [`services`], and what
/// its Plugin directory loaded. `mcp` is the `[agents] mcp` setting: the
/// mcp Pack is installed over the servers of the file it names. `plugins`
/// is the `[agents] plugins` setting: the directory whose library Plugins
/// are loaded.
fn host_packs(
    registry: &Registry,
    mcp: Option<&Path>,
    plugins: Option<&Path>,
) -> (Packs, Option<PluginReport>) {
    with_plugins(&services(registry), read_mcp_servers(mcp), plugins)
}

/// The `mcpServers` object of the `mcp.json`-format file at `setting`,
/// with every other top-level key ignored. No setting or a missing file
/// gives an empty object, and so does a file that cannot be read or is
/// not an `mcpServers` object, with a warning that names the file and the
/// rule and holds no entry value. The object is handed on as it stands:
/// the mcp Pack refuses a malformed entry on its own, and the other
/// servers still connect.
fn read_mcp_servers(setting: Option<&Path>) -> Value {
    let none = || Value::Object(serde_json::Map::new());
    let Some(path) = setting else {
        return none();
    };
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return none(),
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                %error,
                "the MCP file could not be read; no MCP servers are configured"
            );
            return none();
        }
    };
    let document = match serde_json::from_str::<Value>(&text) {
        Ok(document) => document,
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                %error,
                "the MCP file is not valid JSON; no MCP servers are configured"
            );
            return none();
        }
    };
    let Value::Object(mut top) = document else {
        tracing::warn!(
            path = %path.display(),
            "the MCP file must be a JSON object; no MCP servers are configured"
        );
        return none();
    };
    match top.remove("mcpServers") {
        None => none(),
        Some(servers @ Value::Object(_)) => servers,
        Some(_) => {
            tracing::warn!(
                path = %path.display(),
                "the MCP file's `mcpServers` must be a JSON object; no MCP servers are configured"
            );
            none()
        }
    }
}

/// The fully qualified name of the file-tool Pack, which a library Plugin
/// in the Plugin directory inserts, or else the linked std Plugin.
const STD_PACK: &str = "runvil/std";

/// A Host after the server calls, with the Host-wide `services`, the
/// linked Plugins of the Packs agents may declare, `web` and `mcp`, with
/// the `servers` object as mcp's configuration, then loads the library
/// Plugins in the directory `plugins`, when the server is given one, and
/// then calls the linked std Plugin when no library Plugin inserted the
/// file-tool Pack. The Host keeps those Packs and the built-ins
/// `subagents` and `user`, and each run makes its own instances of them.
/// The report lists each library Plugin that loaded and each failure.
fn with_plugins(
    services: &HostServices,
    servers: Value,
    plugins: Option<&Path>,
) -> (Packs, Option<PluginReport>) {
    let mut packs = PackRegistry::new();
    if let Err(error) = plugin_web::install(&mut packs, Value::Null, services) {
        tracing::warn!(%error, "the web Plugin inserted no Pack");
    }
    let mcp = Map::from_iter([("mcp".to_owned(), servers)]);
    if let Err(error) = plugin_mcp::install(&mut packs, Value::Object(mcp), services) {
        tracing::warn!(%error, "the mcp Plugin inserted no Pack");
    }
    let report = plugins.and_then(|directory| load_plugin_dir(&mut packs, directory));
    // The built-in chat agent declares the file tools, so a Host with no
    // Plugin directory, or one whose std library did not load, still
    // installs them.
    let has_std = packs.list().any(|(info, _)| info.fqn == STD_PACK);
    if !has_std && let Err(error) = plugin_std::install(&mut packs, Value::Null, services) {
        tracing::warn!(%error, "the std Plugin inserted no Pack");
    }
    (Packs::new(packs), report)
}

/// Loads every library Plugin in `directory` and returns the report of
/// what loaded, or `None` when the directory cannot be read. A directory
/// that does not exist is a state, not a failure: an installation may
/// ship no Plugin directory. Each Plugin that loaded is logged with its
/// name, version, and source, and each library that did not, with its
/// path and the reason; one failure never stops the others.
fn load_plugin_dir(packs: &mut PackRegistry, directory: &Path) -> Option<PluginReport> {
    let report = match insert_plugin_dir(packs, directory) {
        Ok(report) => report,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
        Err(error) => {
            tracing::warn!(
                directory = %directory.display(),
                %error,
                "the Plugin directory could not be read; no library Plugin is loaded"
            );
            return None;
        }
    };
    for plugin in report.plugins() {
        tracing::info!(
            name = plugin.name(),
            version = plugin.version(),
            source = ?plugin.source(),
            "loaded a library Plugin"
        );
    }
    for failure in report.failures() {
        let path = failure
            .facts
            .get(&ErrorFact::Path)
            .and_then(Value::as_str)
            .unwrap_or_default();
        tracing::warn!(path, error = %failure, "a library Plugin did not load");
    }
    Some(report)
}

/// The Host-wide services the Plugins' `install`s read: the search
/// provider over the gateway `registry` holds, and the runtime the server
/// runs on, under the Plugin contract's tokio runtime service. Built
/// outside a runtime, as a synchronous test does, the runtime is left
/// out, web and mcp are inserted unavailable, and a run that requires
/// either is refused.
fn services(registry: &Registry) -> HostServices {
    let mut services = HostServices::new();
    // Two valid, distinct literals into an empty map, so neither call
    // can be refused.
    let _ = services.provide(
        &SEARCH_PROVIDER,
        Arc::new(GatewaySearchProvider::new(registry.clone())),
    );
    if let Ok(runtime) = tokio::runtime::Handle::try_current() {
        let _ = services.provide(&RUNTIME, Arc::new(runtime));
    }
    services
}

/// The Host finder over the agents folder at `agents_path`: the source
/// `load_agent` reads for each name, or its refusal as [`agent_error`].
fn agent_finder(agents_path: PathBuf) -> impl Fn(&str) -> Result<String, Error> + Send + Sync {
    move |name: &str| {
        load_agent(&agents_path, name)
            .map(|(_file, source)| source)
            .map_err(|refusal| agent_error(&refusal))
    }
}

/// The Host finder's failure for an agent `load_agent` refused: the
/// refusal's display text, under `NotFound` for an agent discovery does
/// not list and `Io` for one whose source cannot be read.
fn agent_error(refusal: &LaunchError) -> Error {
    let code = match refusal {
        LaunchError::UnknownAgent { .. } => ErrorCode::NotFound,
        _ => ErrorCode::Io,
    };
    Error::new(code, refusal.to_string(), code.prose())
}

/// The server's launcher of agent conversations: discovery, launch,
/// lookup, and close, plus the server-side work a launch wires up - the
/// run's setup and the status reporter.
///
/// Typed and construction-phased: the registry, the server's backoff, the
/// run log, and the Host every run shares are captured when
/// the composition root builds it, and the other subsystems are read
/// through the registry at the point of use, so this handle never holds
/// another subsystem's handle.
#[derive(Clone)]
pub struct AgentSessions {
    inner: Arc<Inner>,
}

/// The shared state behind the cloneable handle.
struct Inner {
    /// The subsystem registry: the gateway and menu handles, the
    /// workspace roots, and the push facade are read through it.
    registry: Registry,
    /// Reset on completed replies: an agent reply is useful gateway work.
    backoff: ReconnectBackoff,
    /// The folder the launchable agents are discovered in.
    agents_path: PathBuf,
    /// The run log every run is recorded in, through a handle naming its
    /// agent behind each run's tee.
    recorder: Arc<TursoRecorder>,
    /// The inference broker behind each run's broker.
    broker: WorkshopBroker,
    /// What every run shares: the installed Packs each run makes its
    /// instances of, the finder that reads each agent from the agents
    /// folder, and the tokio timer.
    host: Arc<Host>,
    /// What the Plugin directory loaded, held so that a library Plugin
    /// which inserted no Pack still stays loaded for as long as the
    /// launcher lives.
    _plugins: Option<PluginReport>,
    /// The running conversations.
    conversations: Conversations,
}

impl fmt::Debug for AgentSessions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AgentSessions")
            .field("conversations", &self.inner.conversations)
            .finish_non_exhaustive()
    }
}

impl AgentSessions {
    /// Builds the launcher for `config` over the subsystem registry and
    /// the server's reconnect backoff. The only file this reads is the
    /// `[agents] mcp` file, when the setting names one, and the only
    /// connections it starts are the background ones to that file's MCP
    /// servers, on the server's runtime. It opens each dynamic library in
    /// the `[agents] plugins` directory, when the setting names one, and
    /// loads its Plugin, which runs that library's code. Nothing else is
    /// spawned and nothing else touches the filesystem here: the run log
    /// opens under the state directory when the first run starts.
    #[must_use]
    pub fn new(config: &Config, registry: Registry, backoff: ReconnectBackoff) -> Self {
        let (packs, plugins) = host_packs(
            &registry,
            config.agents.mcp.as_deref(),
            config.agents.plugins.as_deref(),
        );
        let agents_path = config.agents.path.clone();
        let host = Host {
            packs,
            prompts: Arc::new(agent_finder(agents_path.clone())),
            timer: Arc::new(TokioTimer),
        };
        Self {
            inner: Arc::new(Inner {
                agents_path,
                recorder: Arc::new(TursoRecorder::new(
                    config.server.state_dir.join(HARNESS_STATE_DIR),
                )),
                broker: WorkshopBroker::new(registry.clone()),
                host: Arc::new(host),
                _plugins: plugins,
                conversations: Conversations::new(),
                registry,
                backoff,
            }),
        }
    }

    /// The launchable agent names: the `.md` file stems under the
    /// configured agents directory plus the built-in `chat`, sorted.
    #[must_use]
    pub fn discover(&self) -> Vec<String> {
        discover_agents(&self.inner.agents_path)
    }

    /// Launches a conversation running the discovered agent `name` and
    /// returns it with its channels, subscribed before the run starts so
    /// the launching socket sees everything the run reports, a parse
    /// failure included. Its run goes until its program returns, fails,
    /// or [`close`](Self::close) ends it; a stop drops only the round in
    /// flight.
    ///
    /// A launch with no usable gateway is refused before the agent is
    /// read, since its every model round would fail.
    ///
    /// # Errors
    /// Returns [`LaunchRefusal::GatewayUnusable`] when no gateway is
    /// registered or its URL or key cannot build, and the conversation's
    /// own [`LaunchError`] otherwise: an unknown agent or an unreadable
    /// agent source.
    async fn launch(&self, name: &str) -> Result<(Conversation, Receivers), LaunchRefusal> {
        if usable_gateway(&self.inner.registry).is_none() {
            return Err(LaunchRefusal::GatewayUnusable);
        }
        // The folder walk and the source read are filesystem work, so
        // they run on the blocking pool; a worker that cannot report is
        // the same unreadable state as an unreadable file. The run reads
        // the agent again through the Host's finder when it parses it.
        let agents_path = self.inner.agents_path.clone();
        let agent = name.to_owned();
        tokio::task::spawn_blocking(move || load_agent(&agents_path, &agent))
            .await
            .unwrap_or_else(|join| {
                Err(LaunchError::Unreadable {
                    source: io::Error::other(join),
                })
            })?;
        let conversation = self.inner.conversations.open(name);
        let receivers = Receivers::subscribe(&conversation);
        status::spawn_reporter(
            &conversation,
            self.inner.registry.push(),
            self.inner.backoff.clone(),
        );
        tokio::spawn(self.clone().drive(conversation.clone()));
        Ok((conversation, receivers))
    }

    /// Runs `conversation`'s one run of its agent. The setup's selected
    /// model and workspace roots are read once the menu's catalog holds a
    /// chat-capable model, so a pick made while the catalog was empty is
    /// the one the run binds; a close during that wait ends the run before
    /// it begins.
    async fn drive(self, conversation: Conversation) {
        tokio::select! {
            () = conversation.closing() => {}
            () = self.inner.broker.chat_model_ready() => {}
        }
        let setup = with_host_state(&self.inner.registry, self.setup_for(&conversation));
        conversation
            .run(Arc::clone(&self.inner.host), conversation.agent(), setup)
            .await;
    }

    /// The setup of `conversation`'s run before the Host state is read:
    /// its recorder tee over a run-log handle naming its agent, its broker,
    /// its filesystem, and a user, since every conversation has an
    /// operator.
    fn setup_for(&self, conversation: &Conversation) -> RunSetup {
        let recorder = conversation.recorder(Arc::new(
            self.inner.recorder.for_agent(conversation.agent()),
        ));
        let broker = Arc::new(RunBroker::new(
            self.inner.broker.clone(),
            conversation.clone(),
        ));
        RunSetup {
            vfs: language_guide::run_vfs(),
            has_user: true,
            ..RunSetup::new(recorder, broker)
        }
    }

    /// The running conversation with this id, when one exists: how a
    /// socket reattaches after a disconnect.
    fn get(&self, id: &str) -> Option<Conversation> {
        self.inner.conversations.get(&ConversationId::new(id))
    }

    /// Ends the conversation with this id: its run is canceled, and the
    /// conversation leaves the table at once. Returns whether a
    /// conversation was ended.
    #[must_use]
    pub fn close(&self, id: &str) -> bool {
        self.inner.conversations.close(&ConversationId::new(id))
    }

    /// The one run of the running conversation with this id, as the run
    /// log issued it; `None` when no such conversation is running or its
    /// run has not begun.
    #[must_use]
    pub fn run_id(&self, id: &str) -> Option<RunId> {
        self.get(id)?.run_id()
    }
}

/// A refused agent launch, relayed to the client as an error frame.
#[derive(Debug, thiserror::Error)]
enum LaunchRefusal {
    /// No gateway is registered, or the registered gateway's URL or key
    /// cannot build, so no agent could complete a model round.
    #[error("agent sessions need a usable gateway binding; check the gateway base URL and key")]
    GatewayUnusable,
    /// The agent was refused.
    #[error(transparent)]
    Refused(#[from] LaunchError),
}

#[cfg(test)]
mod tests;
