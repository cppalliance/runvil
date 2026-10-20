//! Tests for the `web` Pack's `PackInfo`, `install`, and `construct`: the
//! Pack `install` adds with its config, the tools `construct` names under
//! the Pack id it receives, the configuration it accepts, the Host-wide
//! services it reads, the custom fetch policy, and how a call dispatches:
//! to the tool its last segment names, or failing on a tool it lacks.

use std::sync::Arc;

use runvil_plugin::testing::TestCall;
use runvil_plugin::{
    HostServices, InstallError, Pack, PackId, PackRegistry, ServiceKey, TOKIO_RUNTIME, ToolError,
    ToolErrorKind, ToolId,
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

/// The Pack built under the Pack id `id` with `config` and both services.
fn construct(id: &str, config: Value) -> Result<Arc<dyn Pack>, ToolError> {
    (WEB.construct)(&name(id), config, &services_with(true, true))
}

#[test]
fn the_pack_is_runvil_web_with_no_prelude_and_no_per_run_needs() {
    assert_eq!(WEB.fqn, "runvil/web");
    assert!(WEB.prelude.is_none());
    assert!(WEB.needs.is_empty());
}

#[test]
fn install_adds_the_web_pack_with_the_config_under_its_pack_id() {
    for (config, expected) in [
        (json!({ "web": {}, "mcp": { "docs": {} } }), json!({})),
        (json!({ "mcp": {} }), Value::Null),
        (Value::Null, Value::Null),
    ] {
        let mut packs = PackRegistry::new();
        install(&mut packs, config.clone()).expect("a registry without web takes it");
        let added: Vec<(&str, PackId, Value)> = packs
            .into_entries()
            .filter(|(info, _, _)| info.fqn != "runvil/subagents")
            .map(|(info, id, config)| (info.fqn, id, config))
            .collect();
        assert_eq!(added, [("runvil/web", name("web"), expected)], "{config}");
    }
}

#[test]
fn install_returns_the_registrys_refusal() {
    let mut packs = PackRegistry::new();
    install(&mut packs, Value::Null).expect("the first install adds web");
    assert_eq!(
        install(&mut packs, Value::Null),
        Err(InstallError::DuplicatePack { fqn: "runvil/web" })
    );
}

#[tokio::test]
async fn construct_names_both_tools_under_the_pack_id_it_receives() {
    let tools = construct("browse", Value::Null)
        .expect("construct succeeds with both services")
        .tools();
    let ids: Vec<String> = tools.iter().map(|tool| tool.id.to_string()).collect();
    assert_eq!(ids, ["browse/fetch", "browse/search"]);
    assert!(tools.iter().all(|tool| !tool.survives_stop));
}

#[tokio::test]
async fn construct_accepts_null_or_an_empty_object_and_refuses_any_other_configuration() {
    assert!(construct("web", json!({})).is_ok());
    for config in [json!({ "max_chars": 10 }), json!([]), json!("web")] {
        let Err(error) = construct("web", config.clone()) else {
            panic!("{config} is refused");
        };
        assert_eq!(error.to_string(), "web takes no configuration");
    }
}

#[tokio::test]
async fn construct_fails_naming_whichever_host_wide_service_is_missing() {
    let missing = |provider, runtime| {
        let services = services_with(provider, runtime);
        match (WEB.construct)(&name("web"), Value::Null, &services) {
            Ok(_) => panic!("construct fails without both services"),
            Err(error) => error.to_string(),
        }
    };
    assert_eq!(
        missing(false, true),
        "web needs runvil/search-provider, and this host provides none"
    );
    assert_eq!(
        missing(true, false),
        "web needs runvil/tokio-runtime, and this host provides none"
    );
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
    let pack = construct("web", Value::Null).expect("construct succeeds");
    let call = TestCall::new(ToolId::parse("web/browse").expect("the id parses"));
    let error = pack
        .call(call.context(), json!({}))
        .await
        .expect_err("web has no browse tool");
    assert_eq!(error.to_string(), "web has no tool named browse");
}

#[tokio::test]
async fn a_call_reaches_the_tool_its_last_segment_names() {
    let pack = construct("browse", Value::Null).expect("construct succeeds");

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
    assert_eq!(error.kind(), ToolErrorKind::InvalidArguments);
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
