//! The calls `call_dynamic_tool` makes: the tool's own name on the wire,
//! the result's trust and errors, the arguments it refuses, the servers
//! it cannot reach yet, and the cancel a dropped call sends.

use std::time::Duration;

use runvil_plugin::testing::TestCall;
use runvil_plugin::{ToolErrorKind, ToolId};
use serde_json::json;

use crate::fixture::Fixture;
use crate::support::{Silent, call, invoke, pack_over, ready, remote, settled};

#[tokio::test(flavor = "multi_thread")]
async fn a_call_sends_the_tools_own_name_and_returns_untrusted_text() {
    let fixture = Fixture::start().await;
    let pack = pack_over(json!({ "GitHub": remote(&fixture, &json!({})) }));
    ready(&pack, "GitHub").await;

    let text = call(&pack, "GitHub", "Echo", json!({ "q": "hello" }))
        .await
        .expect("succeeds");
    assert_eq!(text, r#"echo: {"q":"hello"}"#);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_call_without_arguments_sends_an_empty_object() {
    let fixture = Fixture::start().await;
    let pack = pack_over(json!({ "GitHub": remote(&fixture, &json!({})) }));
    ready(&pack, "GitHub").await;

    for args in [
        json!({ "namespace": "GitHub", "toolName": "Echo" }),
        json!({ "namespace": "GitHub", "toolName": "Echo", "arguments": null }),
    ] {
        let text = invoke(&pack, "call_dynamic_tool", args)
            .await
            .expect("succeeds");
        assert_eq!(text, "echo: {}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_is_error_result_becomes_a_backend_error() {
    let fixture = Fixture::start().await;
    let pack = pack_over(json!({ "GitHub": remote(&fixture, &json!({})) }));
    ready(&pack, "GitHub").await;

    let error = call(&pack, "GitHub", "fail", json!({}))
        .await
        .expect_err("isError fails");
    assert_eq!(error.kind(), ToolErrorKind::Backend);
    assert_eq!(error.to_string(), "it failed");
}

#[tokio::test(flavor = "multi_thread")]
async fn arguments_that_are_not_an_object_are_refused_before_anything_is_sent() {
    let fixture = Fixture::start().await;
    let pack = pack_over(json!({ "GitHub": remote(&fixture, &json!({})) }));
    ready(&pack, "GitHub").await;

    let error = call(&pack, "GitHub", "Echo", json!([1]))
        .await
        .expect_err("refused");
    assert_eq!(error.kind(), ToolErrorKind::InvalidArguments);
    assert_eq!(
        error.to_string(),
        "MCP tool arguments must be a JSON object"
    );
    for args in [
        json!(["GitHub", "Echo"]),
        json!({ "namespace": "GitHub" }),
        json!({ "toolName": "Echo" }),
        json!({ "namespace": "GitHub", "toolName": 1 }),
    ] {
        let error = invoke(&pack, "call_dynamic_tool", args)
            .await
            .expect_err("refused");
        assert_eq!(error.kind(), ToolErrorKind::InvalidArguments, "{error}");
    }
    assert!(!fixture.methods().contains(&"tools/call".to_owned()));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_namespace_or_tool_is_refused_without_a_call() {
    let fixture = Fixture::start().await;
    let pack = pack_over(json!({ "GitHub": remote(&fixture, &json!({})) }));
    ready(&pack, "GitHub").await;

    let error = call(&pack, "github", "Echo", json!({}))
        .await
        .expect_err("namespaces match exactly");
    assert_eq!(error.kind(), ToolErrorKind::InvalidArguments);
    assert!(error.to_string().contains("namespace `github`"), "{error}");
    let error = call(&pack, "GitHub", "echo", json!({}))
        .await
        .expect_err("tool names match exactly");
    assert_eq!(error.kind(), ToolErrorKind::InvalidArguments);
    assert!(error.to_string().contains("no tool `echo`"), "{error}");
    assert!(!fixture.methods().contains(&"tools/call".to_owned()));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_call_to_a_server_that_is_not_ready_is_refused_with_its_status() {
    let silent = Silent::start().await;
    let rejecting = Fixture::rejecting().await;
    let pack = pack_over(json!({
        "local": { "command": "never-started" },
        "rejecting": remote(&rejecting, &json!({})),
        "slow": { "url": silent.url },
    }));
    let reason = settled(&pack, "rejecting").await["reason"].clone();

    let error = call(&pack, "slow", "Echo", json!({}))
        .await
        .expect_err("a loading server takes no call");
    assert_eq!(
        error.to_string(),
        "the MCP server `slow` is still loading; call mcp/get_dynamic_tools later to see when it \
         is ready"
    );
    let error = call(&pack, "local", "Echo", json!({}))
        .await
        .expect_err("a refused entry takes no call");
    assert_eq!(
        error.to_string(),
        "the MCP server `local` is unavailable: local MCP servers (command) are not supported yet"
    );
    let error = call(&pack, "rejecting", "Echo", json!({}))
        .await
        .expect_err("a failed server takes no call");
    assert_eq!(
        error.to_string(),
        format!(
            "the MCP server `rejecting` is unavailable: {}",
            reason.as_str().expect("an error has a reason")
        )
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dropped_call_makes_the_server_receive_a_cancel_for_its_request_id() {
    let fixture = Fixture::start().await;
    let pack = pack_over(json!({ "GitHub": remote(&fixture, &json!({})) }));
    ready(&pack, "GitHub").await;

    let test = TestCall::new(ToolId::parse("mcp/call_dynamic_tool").expect("parses"));
    {
        let hanging = pack.call(
            test.context(),
            json!({ "namespace": "GitHub", "toolName": "hang" }),
        );
        tokio::select! {
            _ = hanging => panic!("the hang tool never answers"),
            () = async {
                tokio::time::timeout(Duration::from_secs(30), fixture.shared.hang_started.notified())
                    .await
                    .expect("the call reaches the server");
            } => {}
        }
    }
    tokio::time::timeout(
        Duration::from_secs(30),
        fixture.shared.cancel_arrived.notified(),
    )
    .await
    .expect("dropping the call tells the server");

    let recorded = fixture.shared.recorded.lock().expect("lock");
    assert_eq!(recorded.cancelled.len(), 1);
    assert_eq!(
        Some(recorded.cancelled[0]["requestId"].clone()),
        recorded.hang_id
    );
}
