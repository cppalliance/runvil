//! The Pack's two tools and the listing `get_dynamic_tools` answers: its
//! three shapes, each server's status, and the errors a model reads.

use std::time::Duration;

use runvil_plugin::ToolErrorKind;
use serde_json::{Value, json};

use crate::fixture::Fixture;
use crate::support::{SECRET, Silent, invoke, list, pack_over, ready, remote};

/// The fixture's tools as the listing with no arguments shows them.
fn named_tools() -> Value {
    json!([
        { "tool": "Echo", "description": "Echoes its arguments." },
        { "tool": "fail", "description": "Always fails." },
        { "tool": "hang", "description": "Never answers." },
    ])
}

#[tokio::test(flavor = "multi_thread")]
async fn the_pack_offers_two_tools_under_its_name_and_is_ready_while_a_server_loads() {
    let silent = Silent::start().await;
    let pack = pack_over(json!({ "slow": { "url": silent.url } }));

    let tools = pack.tools();
    let ids: Vec<String> = tools.iter().map(|t| t.id.to_string()).collect();
    assert_eq!(ids, ["mcp/get_dynamic_tools", "mcp/call_dynamic_tool"]);
    assert_eq!(
        tools[0].description,
        "List the tools of the configured MCP servers. With no arguments, list each server's \
         namespace, status, and tool names; with namespace, that server's tools and their input \
         schemas; with namespace and toolName, one tool."
    );
    assert_eq!(
        tools[1].description,
        "Call a tool of a configured MCP server by namespace and toolName, with arguments as a \
         JSON object that matches the tool's input schema. List the servers' tools first to see it."
    );
    assert_eq!(
        tools[1].parameters_schema["required"],
        json!(["namespace", "toolName"])
    );
    tokio::time::timeout(Duration::from_secs(1), pack.ready())
        .await
        .expect("ready answers at once")
        .expect("ready is Ok while a server is still connecting");
    assert_eq!(
        list(&pack, json!({})).await,
        json!({ "namespaces": [{ "namespace": "slow", "status": "loading" }] })
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_null_config_gives_no_namespaces() {
    for config in [Value::Null, json!({})] {
        let pack = pack_over(config);
        assert_eq!(list(&pack, json!({})).await, json!({ "namespaces": [] }));
        assert_eq!(pack.tools().len(), 2, "the two tools stay with no servers");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_listing_shows_each_server_by_its_own_name_with_the_tools_it_lists() {
    let fixture = Fixture::start().await;
    let pack = pack_over(json!({ "GitHub": remote(&fixture, &json!({})) }));
    ready(&pack, "GitHub").await;

    assert_eq!(
        list(&pack, json!({})).await,
        json!({ "namespaces": [{ "namespace": "GitHub", "status": "ready", "tools": named_tools() }] })
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_namespace_lists_its_tools_with_their_schemas_and_a_tool_name_lists_one() {
    let fixture = Fixture::start().await;
    let pack = pack_over(json!({ "GitHub": remote(&fixture, &json!({})) }));
    ready(&pack, "GitHub").await;
    let schema = json!({ "type": "object" });

    assert_eq!(
        list(&pack, json!({ "namespace": "GitHub" })).await,
        json!({
            "namespace": "GitHub",
            "status": "ready",
            "tools": [
                { "tool": "Echo", "description": "Echoes its arguments.", "inputSchema": schema },
                { "tool": "fail", "description": "Always fails.", "inputSchema": schema },
                { "tool": "hang", "description": "Never answers.", "inputSchema": schema },
            ]
        })
    );
    assert_eq!(
        list(&pack, json!({ "namespace": "GitHub", "toolName": "Echo" })).await,
        json!({
            "namespace": "GitHub",
            "tool": "Echo",
            "description": "Echoes its arguments.",
            "inputSchema": schema
        })
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn bad_entries_beside_a_remote_one_are_error_namespaces_and_the_remote_still_connects() {
    let fixture = Fixture::start().await;
    let pack = pack_over(json!({
        "local": { "command": "npx", "args": ["-y", "server"], "env": { "KEY": SECRET } },
        "remote": remote(&fixture, &json!({})),
        "unset": { "url": "https://example.test/${env:RUNVIL_TEST_UNSET_VARIABLE_0042}" },
    }));
    ready(&pack, "remote").await;

    let local = json!({
        "namespace": "local",
        "status": "error",
        "reason": "local MCP servers (command) are not supported yet"
    });
    let unset = json!({
        "namespace": "unset",
        "status": "error",
        "reason": "`url` uses ${env:RUNVIL_TEST_UNSET_VARIABLE_0042}, and \
                   RUNVIL_TEST_UNSET_VARIABLE_0042 is not set"
    });
    let listing = list(&pack, json!({})).await;
    assert_eq!(
        listing,
        json!({ "namespaces": [
            local,
            { "namespace": "remote", "status": "ready", "tools": named_tools() },
            unset,
        ] })
    );
    assert!(!listing.to_string().contains(SECRET), "{listing}");
    assert_eq!(
        list(&pack, json!({ "namespace": "local" })).await,
        local,
        "a server that is not ready answers with its status and no tools"
    );
    assert_eq!(
        list(
            &pack,
            json!({ "namespace": "local", "toolName": "anything" })
        )
        .await,
        local
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tool_name_without_a_namespace_an_unknown_namespace_and_an_unknown_tool_are_refused() {
    let fixture = Fixture::start().await;
    let pack = pack_over(json!({ "GitHub": remote(&fixture, &json!({})) }));
    ready(&pack, "GitHub").await;
    let refused = |args: Value| {
        let pack = &pack;
        async move {
            invoke(pack, "get_dynamic_tools", args)
                .await
                .expect_err("the listing refuses these arguments")
        }
    };

    let error = refused(json!({ "toolName": "Echo" })).await;
    assert_eq!(error.kind(), ToolErrorKind::InvalidArguments);
    assert_eq!(
        error.to_string(),
        "`toolName` names a tool of one server, so it needs `namespace` too"
    );
    let error = refused(json!({ "namespace": "github" })).await;
    assert_eq!(error.kind(), ToolErrorKind::InvalidArguments);
    assert_eq!(
        error.to_string(),
        "no configured MCP server has the namespace `github`; call mcp/get_dynamic_tools with \
         no arguments to list the namespaces"
    );
    let error = refused(json!({ "namespace": "GitHub", "toolName": "echo" })).await;
    assert_eq!(error.kind(), ToolErrorKind::InvalidArguments);
    assert_eq!(
        error.to_string(),
        "the MCP server `GitHub` has no tool `echo`; call mcp/get_dynamic_tools with namespace \
         `GitHub` to list its tools"
    );
    let error = refused(json!({ "namespace": 1 })).await;
    assert_eq!(error.kind(), ToolErrorKind::InvalidArguments);
    let error = refused(json!(["GitHub"])).await;
    assert_eq!(error.kind(), ToolErrorKind::InvalidArguments);
}
