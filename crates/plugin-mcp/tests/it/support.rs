//! What the topics share: the runtime service, the Pack `install` inserts
//! for a map of servers and the instances its factory makes, calls of its
//! two tools, waits on a server's status, and a server that never answers.

use std::sync::Arc;
use std::time::Duration;

use runvil_plugin::testing::TestCall;
use runvil_plugin::{
    Error, HostServices, OutputTrust, Pack, PackFactory, PackId, PackInfo, PackRegistry,
    ServiceKey, TOKIO_RUNTIME, ToolCaller, ToolId, ToolOutput,
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

/// A factory or the reason its Pack was inserted unavailable.
pub(crate) type Inserted = Result<Arc<dyn PackFactory>, Error>;

/// Each Pack `install` inserts into a fresh registry with `config` and the
/// Host-wide `services`, beside its Pack id and its factory or the reason
/// it was inserted unavailable.
pub(crate) fn inserted(
    config: Value,
    services: &HostServices,
) -> Vec<(PackInfo, PackId, Inserted)> {
    let mut packs = PackRegistry::new();
    plugin_mcp::install(&mut packs, config, services).expect("a fresh registry takes the Pack");
    packs
        .into_entries()
        .filter(|(info, _, _)| info.fqn != "runvil/subagents")
        .collect()
}

/// The factory `install` inserts for the server map `servers` with the
/// Host-wide `services`, or the reason it inserted the Pack unavailable.
pub(crate) fn factory(servers: Value, services: &HostServices) -> Inserted {
    let config = Map::from_iter([("mcp".to_owned(), servers)]);
    let mut inserted = inserted(Value::Object(config), services);
    let (_info, _id, factory) = inserted.pop().expect("install inserts one Pack");
    factory
}

/// One Pack instance `factory` makes under the Pack id `mcp`.
pub(crate) fn instance(factory: &Arc<dyn PackFactory>) -> Arc<dyn Pack> {
    let id = PackId::parse("mcp").expect("a one-segment Pack id parses");
    factory
        .new_pack(&id, &HostServices::new())
        .expect("the factory makes the Pack")
}

/// The Pack over the server map `servers`, on the test's runtime.
pub(crate) fn pack_over(servers: Value) -> Arc<dyn Pack> {
    let factory = factory(servers, &runtime_services()).expect("a server map inserts the Pack");
    instance(&factory)
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
pub(crate) async fn invoke(pack: &Arc<dyn Pack>, tool: &str, args: Value) -> Result<String, Error> {
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
) -> Result<ToolOutput, Error> {
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
) -> Result<String, Error> {
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
