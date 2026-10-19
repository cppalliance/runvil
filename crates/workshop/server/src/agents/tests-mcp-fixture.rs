//! A minimal Streamable HTTP MCP server for the agent tests: it answers
//! `initialize`, lists one tool, `get_paper_download_url`, and answers
//! its call with a text block holding JSON beside two resource links.

use axum::Router;
use axum::body::Bytes;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// The JSON the tool's text block holds.
pub(super) const URLS_JSON: &str = r#"{"expires":"2026-01-01T00:00:00Z","urls":["https://example.test/a.pdf","https://example.test/b.pdf"]}"#;

/// The two URLs the tool's resource links name, in block order.
pub(super) const URLS: [&str; 2] = ["https://example.test/a.pdf", "https://example.test/b.pdf"];

/// A running fixture server, stopped on drop.
pub(super) struct McpFixture {
    pub(super) url: String,
    server: JoinHandle<()>,
}

impl Drop for McpFixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

impl McpFixture {
    /// Starts the server on a loopback port the OS picks.
    pub(super) async fn start() -> McpFixture {
        let app = Router::new().route("/mcp", post(post_mcp).get(get_mcp));
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("binds");
        let url = format!(
            "http://{}/mcp",
            listener.local_addr().expect("has an address")
        );
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serves");
        });
        McpFixture { url, server }
    }
}

async fn get_mcp() -> StatusCode {
    StatusCode::METHOD_NOT_ALLOWED
}

fn rpc(id: &Value, result: &Value) -> Response {
    let body = json!({ "jsonrpc": "2.0", "id": id, "result": result });
    (
        [(header::CONTENT_TYPE, "application/json")],
        body.to_string(),
    )
        .into_response()
}

async fn post_mcp(body: Bytes) -> Response {
    let message: Value = serde_json::from_slice(&body).expect("a JSON-RPC message");
    let method = message["method"].as_str().unwrap_or("");
    let Some(id) = message.get("id").filter(|_| !method.is_empty()) else {
        return StatusCode::ACCEPTED.into_response();
    };
    match method {
        "initialize" => {
            let mut response = rpc(
                id,
                &json!({
                    "protocolVersion": "2025-11-25",
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "papers-fixture", "version": "1" }
                }),
            );
            response
                .headers_mut()
                .insert("mcp-session-id", HeaderValue::from_static("papers-session"));
            response
        }
        "tools/list" => rpc(
            id,
            &json!({ "tools": [{
                "name": "get_paper_download_url",
                "description": "Download URLs for papers.",
                "inputSchema": { "type": "object" }
            }] }),
        ),
        "tools/call" => {
            let link = |uri: &str| {
                json!({
                    "type": "resource_link",
                    "uri": uri,
                    "name": uri.rsplit('/').next().unwrap_or(uri),
                    "mimeType": "application/pdf"
                })
            };
            rpc(
                id,
                &json!({ "content": [
                    { "type": "text", "text": URLS_JSON },
                    link(URLS[0]),
                    link(URLS[1]),
                ] }),
            )
        }
        other => {
            let body = json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": format!("{other} is not implemented") }
            });
            (
                [(header::CONTENT_TYPE, "application/json")],
                body.to_string(),
            )
                .into_response()
        }
    }
}
