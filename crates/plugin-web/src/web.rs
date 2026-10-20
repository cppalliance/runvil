//! The crate's Plugin, [`install`], the `web` Pack's `PackInfo`, the
//! `construct` that builds its one shared object, and the keys of the two
//! Host-wide services that `construct` reads.
//!
//! The fetch client is built once, when the Host builds its Packs, with
//! the default fetch policy and bound to the Host's runtime handle; the
//! search tool is built over the Host's search provider. The prompt never
//! sees either.

use std::sync::Arc;

use runvil_plugin::{
    HostServices, InstallError, Pack, PackFuture, PackId, PackInfo, PackRegistry, ServiceKey,
    ToolContext, ToolError, ToolErrorKind, ToolId, ToolInfo, ToolOutput,
};
use serde_json::{Value, json};
use tokio::runtime::Handle;

use crate::fetch::{FetchClient, WebFetch};
use crate::provider::SearchProvider;
use crate::search::WebSearch;

/// The Pack's `PackInfo`.
///
/// Its FQN is `runvil/web`, so its Pack id is `web`. It has no prelude
/// and needs no per-run service; its `construct` reads
/// [`SEARCH_PROVIDER`] and the Plugin contract's
/// [`runvil_plugin::TOKIO_RUNTIME`] service from the Host-wide services
/// and takes no configuration but `null` or `{}`.
const WEB: PackInfo =
    PackInfo::new("runvil/web", construct).description("Search the web and fetch pages");

/// The Plugin: adds the `web` Pack to `packs` with `config["web"]`, which
/// is `null` when `config` has no `web` key.
///
/// # Errors
/// Returns the registry's [`InstallError`], such as
/// [`InstallError::DuplicatePack`] when `packs` holds `runvil/web`
/// already.
#[expect(
    clippy::needless_pass_by_value,
    reason = "the Plugin signature fixes the argument types"
)]
pub fn install(packs: &mut PackRegistry, config: Value) -> Result<(), InstallError> {
    packs.add(WEB, config["web"].clone())
}

/// The service key for the Host's search provider.
///
/// The key's id is `runvil/search-provider`. A Host provides it among
/// its Host-wide services, and every search runs through it.
pub const SEARCH_PROVIDER: ServiceKey<dyn SearchProvider> =
    ServiceKey::new("runvil/search-provider");

/// The key `construct` reads the Host's runtime through. Every fetch is
/// spawned onto it.
const TOKIO_RUNTIME: ServiceKey<Handle> = ServiceKey::new(runvil_plugin::TOKIO_RUNTIME);

/// The one object every run shares: its two-tool list and the two tools.
#[derive(Debug)]
struct Web {
    tools: Vec<ToolInfo>,
    fetch: WebFetch,
    search: WebSearch,
}

/// Builds the Pack under the Pack id `name`: refuses any configuration
/// but `null` or `{}`, reads the two Host-wide services or fails naming
/// the missing one, and names the tools `<name>/fetch` and
/// `<name>/search`.
#[expect(
    clippy::needless_pass_by_value,
    reason = "the PackInfo construct signature fixes the argument types"
)]
fn construct(
    name: &PackId,
    config: Value,
    services: &HostServices,
) -> Result<Arc<dyn Pack>, ToolError> {
    if !(config.is_null() || config == json!({})) {
        return Err(ToolError::message("web takes no configuration"));
    }
    let provider = services.get(&SEARCH_PROVIDER).ok_or_else(|| {
        ToolError::message("web needs runvil/search-provider, and this host provides none")
    })?;
    let runtime = services.get(&TOKIO_RUNTIME).ok_or_else(|| {
        ToolError::message("web needs runvil/tokio-runtime, and this host provides none")
    })?;
    let web = Web::new(
        name,
        FetchClient::new().tool(Handle::clone(&runtime)),
        WebSearch::new(provider),
    )?;
    Ok(Arc::new(web))
}

impl Web {
    /// The Pack over `fetch` and `search`, its tools named under `name`.
    fn new(name: &PackId, fetch: WebFetch, search: WebSearch) -> Result<Web, ToolError> {
        let id = |tool: &str| {
            ToolId::parse(&format!("{name}/{tool}")).map_err(|e| {
                ToolError::new(ToolErrorKind::Backend, "web could not name its tools")
                    .with_source(e)
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
    ) -> PackFuture<'a, Result<ToolOutput, ToolError>> {
        Box::pin(async move {
            match cx.tool().name() {
                "fetch" => self.fetch.call(args).await,
                "search" => self.search.call(args).await,
                other => Err(ToolError::message(format!("web has no tool named {other}"))),
            }
        })
    }
}

#[cfg(test)]
#[path = "web-tests.rs"]
mod tests;
