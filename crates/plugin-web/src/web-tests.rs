//! Tests for the `web` Pack's `PackInfo`, `install`, and factory: the
//! Pack `install` inserts, the configuration it accepts and the Host-wide
//! services it reads, the tools the factory names under the Pack id it
//! receives, the custom fetch policy, and how a call dispatches: to the
//! tool its last segment names, or failing on a tool it lacks.

use std::sync::Arc;

use runvil_plugin::testing::TestCall;
use runvil_plugin::{
    Error, ErrorCode, ErrorFact, HostServices, Pack, PackFactory, PackId, PackRegistry, ServiceKey,
    TOKIO_RUNTIME, ToolId,
};
use serde_json::{Value, json};
use tokio::runtime::Handle;

use super::{SEARCH_PROVIDER, WEB, Web, install};
use crate::config::FetchConfig;
use crate::fetch::FetchClient;
use crate::provider::{SearchError, SearchProvider, SearchQuery, SearchResults};
use crate::search::WebSearch;

/// The key a Host provides its runtime under, built from the Plugin
/// contract's name.
const RUNTIME: ServiceKey<Handle> = ServiceKey::new(TOKIO_RUNTIME);

/// A provider that finds nothing.
struct NoResults;

#[async_trait::async_trait]
impl SearchProvider for NoResults {
    async fn search(&self, query: SearchQuery) -> Result<SearchResults, SearchError> {
        Ok(SearchResults {
            query: query.query,
            results: Vec::new(),
        })
    }
}

/// Host-wide services holding the search provider and this runtime's
/// handle as each flag says.
fn services_with(provider: bool, runtime: bool) -> HostServices {
    let mut host = HostServices::new();
    if provider {
        host.provide(&SEARCH_PROVIDER, Arc::new(NoResults))
            .expect("an empty map takes the provider");
    }
    if runtime {
        host.provide(&RUNTIME, Arc::new(Handle::current()))
            .expect("an empty map takes the runtime");
    }
    host
}

fn name(text: &str) -> PackId {
    PackId::parse(text).expect("a one-segment Pack id parses")
}

/// A factory or the reason its Pack was inserted unavailable.
type Inserted = Result<Arc<dyn PackFactory>, Error>;

/// Each Pack `install` inserts into a fresh registry with `config` and the
/// Host-wide `services`, beside its Pack id and its factory or the reason
/// it was inserted unavailable.
fn inserted(config: Value, services: &HostServices) -> Vec<(&'static str, PackId, Inserted)> {
    let mut packs = PackRegistry::new();
    install(&mut packs, config, services).expect("a registry without web takes it");
    packs
        .into_entries()
        .filter(|(info, _, _)| info.fqn != "runvil/subagents")
        .map(|(info, id, factory)| (info.fqn, id, factory))
        .collect()
}

/// The reason `install` with `config` and `services` inserts the Pack
/// unavailable, or `None` when it inserts it usable.
fn reason(config: Value, services: &HostServices) -> Option<String> {
    let mut inserted = inserted(config, services);
    let (_fqn, _id, factory) = inserted.pop().expect("install inserts one Pack");
    factory.err().map(|error| error.to_string())
}

/// The Pack made under the Pack id `id` by the factory `install` inserts
/// with both services.
fn made(id: &str) -> Arc<dyn Pack> {
    let mut inserted = inserted(Value::Null, &services_with(true, true));
    let (_fqn, _id, factory) = inserted.pop().expect("install inserts one Pack");
    factory
        .expect("install inserts web usable with both services")
        .new_pack(&name(id), &HostServices::new())
        .expect("the factory makes the Pack")
}

#[test]
fn the_pack_is_runvil_web_with_no_prelude() {
    assert_eq!(WEB.fqn, "runvil/web");
    assert!(WEB.prelude.is_none());
}

#[tokio::test]
async fn install_adds_the_web_pack_usable_when_its_config_is_null_or_an_empty_object() {
    for config in [
        json!({ "web": {}, "mcp": { "docs": {} } }),
        json!({ "mcp": {} }),
        Value::Null,
    ] {
        let read: Vec<(&str, PackId, bool)> = inserted(config.clone(), &services_with(true, true))
            .into_iter()
            .map(|(fqn, id, factory)| (fqn, id, factory.is_ok()))
            .collect();
        assert_eq!(read, [("runvil/web", name("web"), true)], "{config}");
    }
}

#[tokio::test]
async fn install_adds_the_web_pack_unavailable_for_any_other_configuration() {
    for config in [json!({ "max_chars": 10 }), json!([]), json!("web")] {
        assert_eq!(
            reason(json!({ "web": config }), &services_with(true, true)),
            Some("web takes no configuration".to_owned()),
            "{config}"
        );
    }
}

#[tokio::test]
async fn install_without_a_host_wide_service_adds_the_web_pack_unavailable_naming_it() {
    assert_eq!(
        reason(Value::Null, &services_with(false, true)),
        Some("web needs runvil/search-provider, and this host provides none".to_owned())
    );
    assert_eq!(
        reason(Value::Null, &services_with(true, false)),
        Some("web needs runvil/tokio-runtime, and this host provides none".to_owned())
    );
}

#[tokio::test]
async fn install_returns_the_registrys_refusal() {
    let mut packs = PackRegistry::new();
    let services = services_with(true, true);
    install(&mut packs, Value::Null, &services).expect("the first install inserts web");
    let error =
        install(&mut packs, Value::Null, &services).expect_err("a second install is refused");
    assert_eq!(error.code, ErrorCode::InvalidValue);
    assert_eq!(
        error.facts.get(&ErrorFact::Reason),
        Some(&Value::from("duplicate_pack"))
    );
    assert_eq!(
        error.facts.get(&ErrorFact::Name),
        Some(&Value::from("runvil/web"))
    );
}

#[tokio::test]
async fn the_factory_names_both_tools_under_the_pack_id_it_receives() {
    let tools = made("browse").tools();
    let ids: Vec<String> = tools.iter().map(|tool| tool.id.to_string()).collect();
    assert_eq!(ids, ["browse/fetch", "browse/search"]);
    assert!(tools.iter().all(|tool| !tool.survives_stop));
}

#[tokio::test]
async fn a_custom_fetch_policy_reaches_the_fetch_tools_schema() {
    let id = name("web");
    let web = Web::new(
        &id,
        FetchClient::new().tool(Handle::current()),
        WebSearch::new(Arc::new(NoResults)),
    )
    .expect("the tool ids parse");
    let policy = FetchConfig::builder()
        .max_chars(10_000)
        .build()
        .expect("valid policy");
    let web = web
        .with_fetch_config(&id, Handle::current(), policy)
        .expect("the custom policy builds a fetch client");
    let fetch = web
        .tools()
        .into_iter()
        .find(|tool| tool.id.to_string() == "web/fetch")
        .expect("the fetch tool is offered");
    assert_eq!(
        fetch.parameters_schema["properties"]["max_chars"]["maximum"], 10_000,
        "the fetch tool enforces the custom policy"
    );
}

#[tokio::test]
async fn a_call_to_a_tool_web_does_not_offer_fails_naming_it() {
    let pack = made("web");
    let call = TestCall::new(ToolId::parse("web/browse").expect("the id parses"));
    let error = pack
        .call(call.context(), json!({}))
        .await
        .expect_err("web has no browse tool");
    assert_eq!(error.to_string(), "web has no tool named browse");
}

#[tokio::test]
async fn a_call_reaches_the_tool_its_last_segment_names() {
    let pack = made("browse");

    let search = TestCall::new(ToolId::parse("browse/search").expect("the id parses"));
    let output = pack
        .call(search.context(), json!({ "query": "rust" }))
        .await
        .expect("the search tool answers through the provider");
    assert_eq!(output.text(), r#"{"query":"rust","results":[]}"#);

    let fetch = TestCall::new(ToolId::parse("browse/fetch").expect("the id parses"));
    let error = pack
        .call(fetch.context(), json!({ "url": "https://1.2.3.4/" }))
        .await
        .expect_err("the fetch policy refuses an IP literal host");
    assert_eq!(error.code, ErrorCode::ToolInvalidArguments);
    assert!(
        error.to_string().contains("ip literal host not allowed"),
        "the fetch tool's policy refused the call, got: {error}"
    );
}

#[tokio::test]
async fn each_service_key_literal_is_a_namespace_name_id() {
    let mut host = HostServices::new();
    host.provide(&SEARCH_PROVIDER, Arc::new(NoResults))
        .expect("provide accepts the search provider's literal");
    host.provide(&RUNTIME, Arc::new(Handle::current()))
        .expect("provide accepts the runtime's literal");
}
