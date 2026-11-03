//! The agent sessions' search provider: every `web/search`
//! call runs through the current Gateway generation's web search relay.
//!
//! The provider holds the server's [`Registry`] and reads the gateway
//! handles through it on every search, as the inference broker does, so a
//! replaced gateway serves the next search. It builds a [`GatewaySearch`]
//! per search; the Gateway client's shared connection pool makes that a
//! few clones and keeps connections alive across searches.

use harness_gateway_client::GatewaySearch;
use plugin_web::{SearchError, SearchErrorKind, SearchProvider, SearchQuery, SearchResults};
use workshop_gateway::GatewayHandles;
use workshop_registry::Registry;

use super::gateway::UsableGateway;

/// The message a search fails with when no usable Gateway client exists,
/// so no endpoint or key detail reaches the model.
const NO_GATEWAY: &str = "request failed";

/// Searches through the Gateway the server's registry currently holds.
pub(super) struct GatewaySearchProvider {
    /// The subsystem registry the gateway handles are read through.
    registry: Registry,
}

impl GatewaySearchProvider {
    /// Builds the provider over the server's subsystem registry.
    pub(super) fn new(registry: Registry) -> Self {
        Self { registry }
    }

    /// A client for the current gateway generation. A missing gateway
    /// registration, a gateway the heartbeat reports down, or an endpoint
    /// or key that cannot be built fails as transport with [`NO_GATEWAY`].
    fn client(&self) -> Result<GatewaySearch, SearchError> {
        let no_gateway = || SearchError::new(SearchErrorKind::Transport, NO_GATEWAY);
        let handles = self
            .registry
            .state::<GatewayHandles>()
            .ok_or_else(no_gateway)?;
        if !handles.health().is_reachable() {
            return Err(no_gateway());
        }
        let gateway = UsableGateway::read(&handles).ok_or_else(no_gateway)?;
        Ok(GatewaySearch::new(gateway.endpoint, gateway.key))
    }
}

#[async_trait::async_trait]
impl SearchProvider for GatewaySearchProvider {
    async fn search(&self, query: SearchQuery) -> Result<SearchResults, SearchError> {
        let client = self.client()?;
        SearchProvider::search(&client, query).await
    }
}

#[cfg(test)]
#[path = "search-tests.rs"]
mod tests;
