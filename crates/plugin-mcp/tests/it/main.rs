//! Integration tests for `plugin-mcp`, through `install` and `TestCall`
//! only, against a hand-written Streamable HTTP server: the Pack
//! `install` adds, its construct over a map of servers, and the
//! connection each server keeps here, the listing `get_dynamic_tools`
//! answers in `listing`, and the calls `call_dynamic_tool` makes in
//! `calls`.
#![expect(
    clippy::expect_used,
    reason = "test helpers panic on setup failure, which is the desired behavior"
)]

mod calls;
mod fixture;
mod listing;
mod support;

use std::time::Duration;

use fixture::Fixture;
use runvil_plugin::HostServices;
use serde_json::{Value, json};
use support::{
    SECRET, added, call, construct, list, pack_over, ready, remote, runtime_services, settled,
};

#[test]
fn install_adds_runvil_mcp_with_no_prelude_and_no_needs() {
    let added = added(Value::Null);
    let [(info, id, config)] = added.as_slice() else {
        panic!("install adds one Pack: {added:?}");
    };
    assert_eq!(info.fqn, "runvil/mcp");
    assert_eq!(id.to_string(), "mcp");
    assert_eq!(*config, Value::Null);
    assert!(info.prelude.is_none());
    assert!(info.needs.is_empty());
}

#[test]
fn install_adds_the_pack_with_the_server_map_under_its_pack_id() {
    let servers = json!({ "docs": { "url": "http://127.0.0.1:1/mcp" } });
    for (config, expected) in [
        (json!({ "mcp": servers, "web": {} }), servers.clone()),
        (json!({ "web": {} }), Value::Null),
    ] {
        let added = added(config.clone());
        let read: Vec<(&str, String, &Value)> = added
            .iter()
            .map(|(info, id, config)| (info.fqn, id.to_string(), config))
            .collect();
        assert_eq!(
            read,
            [("runvil/mcp", "mcp".to_owned(), &expected)],
            "{config}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_config_that_is_not_an_object_refuses_the_construct() {
    for config in [json!([]), json!("servers"), json!(1)] {
        let error = construct(config.clone(), &runtime_services())
            .err()
            .expect("only an object or null is a server map");
        assert!(
            error.to_string().contains("JSON object"),
            "{config}: {error}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_missing_runtime_service_fails_construct_naming_it_even_with_no_servers() {
    for config in [
        Value::Null,
        json!({ "docs": { "url": "http://127.0.0.1:1/mcp" } }),
    ] {
        let error = construct(config.clone(), &HostServices::new())
            .err()
            .expect("no runtime is a failure");
        assert!(
            error.to_string().contains("runvil/tokio-runtime"),
            "{config}: {error}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_initialize_request_names_runvil_and_declares_no_capabilities() {
    let fixture = Fixture::start().await;
    let pack = pack_over(json!({ "remote": remote(&fixture, &json!({})) }));
    ready(&pack, "remote").await;

    let initializes = fixture
        .shared
        .recorded
        .lock()
        .expect("lock")
        .initializes
        .clone();
    assert_eq!(initializes.len(), 1);
    assert_eq!(initializes[0]["clientInfo"]["name"], "Runvil");
    assert_eq!(initializes[0]["capabilities"], json!({}));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_configured_headers_arrive_on_every_request() {
    let fixture = Fixture::start().await;
    let headers = json!({ "Authorization": format!("Bearer {SECRET}"), "X-MCP-Tools": "echo" });
    let pack = pack_over(json!({ "remote": remote(&fixture, &headers) }));
    ready(&pack, "remote").await;
    call(&pack, "remote", "Echo", json!({}))
        .await
        .expect("the call succeeds");

    let seen = fixture.shared.recorded.lock().expect("lock").seen.clone();
    let methods: Vec<&str> = seen.iter().map(|s| s.method.as_str()).collect();
    assert!(methods.contains(&"initialize"));
    assert!(methods.contains(&"notifications/initialized"));
    assert!(methods.contains(&"tools/list"));
    assert!(methods.contains(&"tools/call"));
    for request in &seen {
        assert_eq!(
            request.headers["authorization"],
            format!("Bearer {SECRET}"),
            "{}",
            request.method
        );
        assert_eq!(request.headers["x-mcp-tools"], "echo", "{}", request.method);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_rejecting_server_is_an_error_namespace_whose_reason_holds_no_header_value() {
    let fixture = Fixture::rejecting().await;
    let headers = json!({ "Authorization": format!("Bearer {SECRET}") });
    let pack = pack_over(json!({ "remote": remote(&fixture, &headers) }));

    let entry = settled(&pack, "remote").await;
    assert_eq!(entry["status"], "error", "a 401 fails the start: {entry}");
    assert!(entry.get("tools").is_none(), "{entry}");
    let reason = entry["reason"].as_str().expect("an error has a reason");
    assert!(reason.contains("handshake"), "{reason}");
    assert!(!list(&pack, json!({})).await.to_string().contains(SECRET));
    let refused = call(&pack, "remote", "Echo", json!({}))
        .await
        .expect_err("no call goes out");
    assert!(!refused.to_string().contains(SECRET), "{refused}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_server_that_rejects_its_first_connection_lists_its_tools_once_it_accepts() {
    let fixture = Fixture::rejecting().await;
    let pack = pack_over(json!({ "remote": remote(&fixture, &json!({})) }));
    let entry = settled(&pack, "remote").await;
    assert_eq!(entry["status"], "error", "a 401 fails the start: {entry}");

    fixture.accept();
    let entry = ready(&pack, "remote").await;
    assert_eq!(entry["tools"][0]["tool"], "Echo", "{entry}");
    let text = call(&pack, "remote", "Echo", json!({ "q": "hello" }))
        .await
        .expect("succeeds");
    assert_eq!(text, r#"echo: {"q":"hello"}"#);
}

#[tokio::test(flavor = "multi_thread")]
async fn dropping_the_pack_ends_every_session_and_stops_the_connection_tasks() {
    let first = Fixture::start().await;
    let second = Fixture::start().await;
    let pack = pack_over(json!({
        "first": remote(&first, &json!({})),
        "second": remote(&second, &json!({})),
    }));
    ready(&pack, "first").await;
    ready(&pack, "second").await;
    drop(pack);

    tokio::time::timeout(Duration::from_secs(30), async {
        while [&first, &second]
            .iter()
            .any(|fixture| fixture.shared.recorded.lock().expect("lock").deletes == 0)
        {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("rmcp's worker ends each session when its service drops");
}
