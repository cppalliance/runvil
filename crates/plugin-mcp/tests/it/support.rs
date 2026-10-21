//! What the topics share: the runtime service, the Pack `install` inserts,
//! built over a map of servers, calls of its two tools, waits on a
//! server's status, and a server that never answers.

use std::sync::Arc;
use std::time::Duration;

use runvil_plugin::testing::TestCall;
use runvil_plugin::{
    HostServices, OutputTrust, Pack, PackId, PackInfo, PackRegistry, ServiceKey, TOKIO_RUNTIME,
    ToolCaller, ToolError, ToolId, ToolOutput,
};
use serde_json::{Map, Value, json};
use tokio::net::TcpListener;
use tokio::runtime::Handle;
use tokio::task::JoinHandle;

use crate::fixture::Fixture;

/// A header value no reason, listing, or error may repeat.
pub(crate) const SECRET: &str = "ghp_secret_token_value_0042";

const RUNTIME: ServiceKey<Handle> = ServiceKey::new(TOKIO_RUNTIME);

/// Host-wide services holding the test's runtime.
pub(crate) fn runtime_services() -> HostServices {
    let mut services = HostServices::new();
    services
        .provide(&RUNTIME, Arc::new(Handle::current()))
        .expect("an empty map takes the runtime");
    services
}

/// Each Pack `install` inserts into a fresh registry with `config`, beside
/// the Pack id and config the registry holds it with.
pub(crate) fn inserted(config: Value) -> Vec<(PackInfo, PackId, Value)> {
    let mut packs = PackRegistry::new();
    plugin_mcp::install(&mut packs, config).expect("a fresh registry takes the Pack");
    packs
        .into_entries()
        .filter(|(info, _, _)| info.fqn != "runvil/subagents")
        .collect()
}

/// Builds the Pack `install` inserts, under the Pack id and with the config
/// the registry holds it with, from the server map `servers`.
pub(crate) fn construct(
    servers: Value,
    services: &HostServices,
) -> Result<Arc<dyn Pack>, ToolError> {
    let config = Map::from_iter([("mcp".to_owned(), servers)]);
    let mut inserted = inserted(Value::Object(config));
    let (info, id, config) = inserted.pop().expect("install inserts one Pack");
    (info.construct)(&id, config, services)
}

/// The Pack over the server map `servers`, on the test's runtime.
pub(crate) fn pack_over(servers: Value) -> Arc<dyn Pack> {
    construct(servers, &runtime_services()).expect("a server map constructs")
}

/// A remote entry for `fixture` with these extra headers.
pub(crate) fn remote(fixture: &Fixture, headers: &Value) -> Value {
    json!({ "url": fixture.url, "headers": headers })
}

fn tool_call(tool: &str) -> TestCall {
    TestCall::new(ToolId::parse(&format!("mcp/{tool}")).expect("a tool id parses"))
}

/// Calls the Pack's tool `mcp/<tool>` with `args` from a model round and
/// returns its text, checking that the output is untrusted.
pub(crate) async fn invoke(
    pack: &Arc<dyn Pack>,
    tool: &str,
    args: Value,
) -> Result<String, ToolError> {
    let test = tool_call(tool).with_caller(ToolCaller::Model);
    let output = pack.call(test.context(), args).await?;
    assert_eq!(
        output.trust(),
        OutputTrust::Untrusted,
        "output to a model round is untrusted"
    );
    Ok(output.text().to_owned())
}

/// Calls the Pack's tool `mcp/<tool>` with `args` from the section's own
/// script and returns the whole output.
pub(crate) async fn invoke_as_script(
    pack: &Arc<dyn Pack>,
    tool: &str,
    args: Value,
) -> Result<ToolOutput, ToolError> {
    pack.call(tool_call(tool).context(), args).await
}

/// The JSON `get_dynamic_tools` answers for `args`.
pub(crate) async fn list(pack: &Arc<dyn Pack>, args: Value) -> Value {
    let text = invoke(pack, "get_dynamic_tools", args)
        .await
        .expect("the listing answers");
    serde_json::from_str(&text).expect("the listing is JSON")
}

/// Calls the tool `tool` of the server under `namespace` through
/// `call_dynamic_tool`.
pub(crate) async fn call(
    pack: &Arc<dyn Pack>,
    namespace: &str,
    tool: &str,
    arguments: Value,
) -> Result<String, ToolError> {
    invoke(
        pack,
        "call_dynamic_tool",
        json!({ "namespace": namespace, "toolName": tool, "arguments": arguments }),
    )
    .await
}

/// Waits until the listing entry of `namespace` satisfies `done`, and
/// returns that entry.
async fn wait_for(pack: &Arc<dyn Pack>, namespace: &str, done: impl Fn(&Value) -> bool) -> Value {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let entry = list(pack, json!({ "namespace": namespace })).await;
            if done(&entry) {
                return entry;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the server reaches the status long before this guard")
}

/// Waits until the server under `namespace` leaves `loading`, and
/// returns its entry.
pub(crate) async fn settled(pack: &Arc<dyn Pack>, namespace: &str) -> Value {
    wait_for(pack, namespace, |entry| entry["status"] != "loading").await
}

/// Waits until the server under `namespace` is ready, and returns its
/// entry.
pub(crate) async fn ready(pack: &Arc<dyn Pack>, namespace: &str) -> Value {
    wait_for(pack, namespace, |entry| entry["status"] == "ready").await
}

/// A loopback server that accepts connections and never answers, so a
/// server at its URL stays `loading` for the whole startup deadline.
pub(crate) struct Silent {
    pub(crate) url: String,
    task: JoinHandle<()>,
}

impl Drop for Silent {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Silent {
    pub(crate) async fn start() -> Silent {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("binds");
        let url = format!(
            "http://{}/mcp",
            listener.local_addr().expect("has an address")
        );
        let task = tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((socket, _)) = listener.accept().await {
                held.push(socket);
            }
        });
        Silent { url, task }
    }
}
