//! One configured server: its state, the tools it listed, and the calls
//! that go to it.

use std::time::Duration;

use rmcp::model::{
    CallToolRequest, CallToolRequestParams, CancelledNotificationParam, ClientRequest, JsonObject,
    RequestId, Tool,
};
use rmcp::service::{Peer, PeerRequestOptions, RoleClient};
use runvil_plugin::{ToolError, ToolErrorKind, ToolOutput};
use serde_json::Value;
use tokio::runtime::Handle;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::result;

/// How long a call may take before it fails as a transport error.
pub(crate) const CALL_DEADLINE: Duration = Duration::from_mins(5);

/// Where a server is. It leaves `Starting` when the first attempt settles
/// and never returns to it. A `Failed` server moves to `Ready` when a later
/// attempt succeeds, and a `Ready` one stays `Ready`.
#[derive(Debug)]
pub(crate) enum State {
    Starting,
    Ready(Ready),
    Failed(String),
}

/// A connected server: the peer calls go through and the tools it listed.
#[derive(Debug)]
pub(crate) struct Ready {
    pub(crate) peer: Peer<RoleClient>,
    pub(crate) tools: Vec<Listed>,
}

/// One tool the server listed.
#[derive(Debug)]
pub(crate) struct Listed {
    /// The name the server lists the tool under, which a call sends.
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) input_schema: Value,
}

/// Keeps every tool the server listed, under the name it listed. A tool's
/// description is its `description`, else its `title`, else its name.
pub(crate) fn catalog(tools: Vec<Tool>) -> Vec<Listed> {
    tools
        .into_iter()
        .map(|tool| {
            let name = tool.name.to_string();
            let description = tool
                .description
                .as_deref()
                .or(tool.title.as_deref())
                .unwrap_or(&name)
                .to_owned();
            Listed {
                description,
                input_schema: Value::Object((*tool.input_schema).clone()),
                name,
            }
        })
        .collect()
}

/// A server's connection: a watch on the connection task's state, and the
/// task.
#[derive(Debug)]
pub(crate) struct Server {
    state: watch::Receiver<State>,
    connection: JoinHandle<()>,
}

impl Server {
    pub(crate) fn new(state: watch::Receiver<State>, connection: JoinHandle<()>) -> Server {
        Server { state, connection }
    }

    /// The server's state now. The borrow holds the connection task's
    /// next publish back, so it is dropped before any await.
    pub(crate) fn state(&self) -> watch::Ref<'_, State> {
        self.state.borrow()
    }
}

impl Drop for Server {
    /// Aborts the connection task, which drops the running service and so
    /// ends the HTTP session.
    fn drop(&mut self) {
        self.connection.abort();
    }
}

/// A spawned call, aborted when the call awaiting it is dropped.
struct AbortOnDrop(JoinHandle<Result<ToolOutput, ToolError>>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Tells the server to drop a request when the task holding it is aborted.
/// rmcp sends `notifications/cancelled` itself on a timeout, but dropping a
/// request's handle or response sends nothing.
struct CancelOnDrop {
    armed: Option<(Peer<RoleClient>, RequestId)>,
    runtime: Handle,
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if let Some((peer, id)) = self.armed.take() {
            self.runtime.spawn(async move {
                // The connection may be gone already, which leaves nothing to tell.
                let _ = peer
                    .notify_cancelled(CancelledNotificationParam::new(Some(id), None))
                    .await;
            });
        }
    }
}

/// Sends one `tools/call` and renders the answer. It runs on the Host's
/// runtime, so dropping the awaiting call aborts it and fires the guard.
async fn send(
    peer: Peer<RoleClient>,
    name: String,
    arguments: JsonObject,
    runtime: Handle,
) -> Result<ToolOutput, ToolError> {
    let request = ClientRequest::CallToolRequest(CallToolRequest::new(
        CallToolRequestParams::new(name).with_arguments(arguments),
    ));
    let handle = peer
        .send_cancellable_request(request, PeerRequestOptions::with_timeout(CALL_DEADLINE))
        .await
        .map_err(|e| result::service_error(&e))?;
    let mut guard = CancelOnDrop {
        armed: Some((handle.peer.clone(), handle.id.clone())),
        runtime,
    };
    let response = handle.await_response().await;
    guard.armed = None;
    result::output(response)
}

/// Calls the tool `name` through `peer` with `arguments`, on `runtime`.
/// Dropping the returned future aborts the call and tells the server to
/// drop the request.
pub(crate) async fn call(
    runtime: &Handle,
    peer: Peer<RoleClient>,
    name: String,
    arguments: JsonObject,
) -> Result<ToolOutput, ToolError> {
    let mut task = AbortOnDrop(runtime.spawn(send(peer, name, arguments, runtime.clone())));
    match (&mut task.0).await {
        Ok(outcome) => outcome,
        Err(join) => Err(ToolError::new(
            ToolErrorKind::Transport,
            "the MCP call task did not finish",
        )
        .with_source(join)),
    }
}

#[cfg(test)]
#[path = "server-tests.rs"]
mod tests;
