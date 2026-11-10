//! The crate's Plugin, [`install`], the `web` Pack's `PackInfo`, the
//! factory that holds the HTTP clients every instance shares, and the
//! keys of the two Host-wide services that `install` reads.
//!
//! The fetch client is built once, when the Plugin installs the Pack,
//! with the default fetch policy and bound to the Host's runtime handle;
//! the search tool is built over the Host's search provider. The prompt
//! never sees either.

use std::sync::Arc;

use runvil_plugin::{
    BoxFuture, Error, ErrorCode, HostServices, Pack, PackFactory, PackId, PackInfo, PackRegistry,
    ServiceKey, ToolContext, ToolId, ToolInfo, ToolOutput, make_tool_error,
};
use serde_json::{Value, json};
use tokio::runtime::Handle;

use crate::fetch::{FetchClient, WebFetch};
use crate::provider::SearchProvider;
use crate::search::WebSearch;

/// The Pack's `PackInfo`.
///
/// Its FQN is `runvil/web`, so its Pack id is `web`. It has no prelude,
/// and its `install` reads [`SEARCH_PROVIDER`] and the Plugin contract's
/// [`runvil_plugin::TOKIO_RUNTIME`] service from the Host-wide services
/// and takes no configuration but `null` or `{}`.
const WEB: PackInfo = PackInfo::new("runvil/web").description("Search the web and fetch pages");

/// The Plugin: inserts the `web` Pack into `packs` with a factory holding
/// the HTTP clients every instance shares, built from the search provider
/// and the Tokio runtime in `services`. A missing one inserts the Pack
/// unavailable, naming it. Its config, `config["web"]`, which is `null`
/// when `config` has no `web` key, must be `null` or `{}`; anything else
/// inserts the Pack unavailable with "web takes no configuration".
///
/// # Errors
/// Returns the registry's refusal, such as an
/// [`InvalidValue`](runvil_plugin::ErrorCode::InvalidValue) error with the
/// reason `duplicate_pack` when `packs` holds `runvil/web` already.
#[expect(
    clippy::needless_pass_by_value,
    reason = "the Plugin signature fixes the argument types"
)]
pub fn install(
    packs: &mut PackRegistry,
    config: Value,
    services: &HostServices,
) -> Result<(), Error> {
    match Clients::new(&config["web"], services) {
        Ok(clients) => packs.insert(WEB, clients),
        Err(reason) => packs.insert_unavailable(WEB, reason),
    }
}

/// The service key for the Host's search provider.
///
/// The key's id is `runvil/search-provider`. A Host provides it among
/// its Host-wide services, and every search runs through it.
pub const SEARCH_PROVIDER: ServiceKey<dyn SearchProvider> =
    ServiceKey::new("runvil/search-provider");

/// The key `install` reads the Host's runtime through. Every fetch is
/// spawned onto it.
const TOKIO_RUNTIME: ServiceKey<Handle> = ServiceKey::new(runvil_plugin::TOKIO_RUNTIME);

/// The Pack's factory: the two tools every instance shares, built once at
/// `install`.
struct Clients {
    fetch: WebFetch,
    search: WebSearch,
}

impl Clients {
    /// The clients over the two Host-wide services: refuses any
    /// configuration but `null` or `{}`, and fails naming a missing
    /// service.
    fn new(config: &Value, services: &HostServices) -> Result<Clients, Error> {
        if !(config.is_null() || *config == json!({})) {
            return Err(make_tool_error(
                ErrorCode::ToolOther,
                "web takes no configuration",
            ));
        }
        let provider = services.get(&SEARCH_PROVIDER).ok_or_else(|| {
            make_tool_error(
                ErrorCode::ToolOther,
                "web needs runvil/search-provider, and this host provides none",
            )
        })?;
        let runtime = services.get(&TOKIO_RUNTIME).ok_or_else(|| {
            make_tool_error(
                ErrorCode::ToolOther,
                "web needs runvil/tokio-runtime, and this host provides none",
            )
        })?;
        Ok(Clients {
            fetch: FetchClient::new().tool(Handle::clone(&runtime)),
            search: WebSearch::new(provider),
        })
    }
}

impl PackFactory for Clients {
    /// Makes the Pack under the Pack id `id` over clones of the shared
    /// clients, naming the tools `<id>/fetch` and `<id>/search`.
    fn new_pack(&self, id: &PackId) -> Result<Arc<dyn Pack>, Error> {
        let web = Web::new(id, self.fetch.clone(), self.search.clone())?;
        Ok(Arc::new(web))
    }
}

/// One run's instance: its two-tool list and the two tools.
#[derive(Debug)]
struct Web {
    tools: Vec<ToolInfo>,
    fetch: WebFetch,
    search: WebSearch,
}

impl Web {
    /// The Pack over `fetch` and `search`, its tools named under `name`.
    fn new(name: &PackId, fetch: WebFetch, search: WebSearch) -> Result<Web, Error> {
        let id = |tool: &str| {
            ToolId::parse(&format!("{name}/{tool}")).map_err(|e| {
                make_tool_error(ErrorCode::ToolBackend, "web could not name its tools")
                    .with_cause(&e)
            })
        };
        let tools = vec![
            ToolInfo::new(id("fetch")?, fetch.description(), fetch.parameters_schema()),
            ToolInfo::new(
                id("search")?,
                search.description(),
                search.parameters_schema(),
            ),
        ];
        Ok(Web {
            tools,
            fetch,
            search,
        })
    }

    /// Replaces the default fetch policy with a validated custom one,
    /// rebuilding the fetch tool's info to match.
    #[cfg(test)]
    fn with_fetch_config(
        self,
        name: &PackId,
        runtime: Handle,
        config: crate::config::FetchConfig,
    ) -> Result<Web, crate::config::ConfigError> {
        let fetch = FetchClient::try_with_config(config)?.tool(runtime);
        Ok(Web::new(name, fetch, self.search).expect("the tool ids parsed once already"))
    }
}

impl Pack for Web {
    fn tools(&self) -> Vec<ToolInfo> {
        self.tools.clone()
    }

    fn call<'a>(
        &'a self,
        cx: ToolContext<'a>,
        args: Value,
    ) -> BoxFuture<'a, Result<ToolOutput, Error>> {
        Box::pin(async move {
            match cx.tool().name() {
                "fetch" => self.fetch.call(args).await,
                "search" => self.search.call(args).await,
                other => Err(make_tool_error(
                    ErrorCode::ToolOther,
                    format!("web has no tool named {other}"),
                )),
            }
        })
    }
}

#[cfg(test)]
#[path = "web-tests.rs"]
mod tests;
