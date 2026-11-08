//! The fixture ask Pack a suite Host builds in place of a real
//! user-input Pack, which Harness crates may not name: an ask tool that
//! survives stops, a prelude defining `input.ask()`, a factory that takes
//! the run's broker and refuses a run without one, and a broker that
//! fails with a tool error. A prompt declares it as `tests/user-input`.

use std::sync::Arc;

use harness::pack::{HostServices, PackRegistry, Packs};
use runvil::make_tool_error;
use runvil::{Error, ErrorCode};
use runvil_plugin::{
    Pack, PackFuture, PackId, PackInfo, ServiceKey, ToolContext, ToolId, ToolInfo, ToolOutput,
};
use serde_json::{Value, json};

/// The `PackInfo`, whose Pack id is `user-input`.
const ASKER: PackInfo = PackInfo::new("tests/user-input").prelude(PRELUDE);

/// The key the run's broker is provided under.
const BROKER: ServiceKey<dyn AskBroker> = ServiceKey::new("tests/input-broker");

const PRELUDE: &str = r#"local pack = ...
input = {}
function input.ask(...)
  if select('#', ...) > 0 then
    error("input.ask takes no arguments", 2)
  end
  return tools.call(pack .. "/ask")
end
"#;

/// Delivers the operator's answer to the fixture's ask tool.
#[async_trait::async_trait]
pub(crate) trait AskBroker: Send + Sync {
    async fn wait(&self) -> Result<String, Error>;
}

/// A Host's Packs: the fixture, installed as `user-input`.
pub(crate) fn asker_packs() -> Packs {
    let mut packs = PackRegistry::new();
    if let Err(error) = packs.insert(ASKER, new_asker) {
        panic!("a new registry takes the fixture: {error}");
    }
    Packs::new(packs)
}

/// Run services holding `broker` as the run's input broker.
pub(crate) fn broker_services(broker: Arc<dyn AskBroker>) -> HostServices {
    let mut services = HostServices::new();
    if let Err(error) = services.provide(&BROKER, broker) {
        panic!("an empty map takes the broker: {error}");
    }
    services
}

/// One run's instance: its ask tool and the broker it took from the run.
struct Asker {
    tools: Vec<ToolInfo>,
    broker: Arc<dyn AskBroker>,
}

fn new_asker(name: &PackId, run: &HostServices) -> Result<Arc<dyn Pack>, Error> {
    let broker = run.get(&BROKER).ok_or_else(|| {
        make_tool_error(
            ErrorCode::ToolOther,
            format!("{name} needs tests/input-broker, and this run provides none"),
        )
    })?;
    let id = ToolId::parse(&format!("{name}/ask")).map_err(|e| {
        make_tool_error(
            ErrorCode::ToolBackend,
            "the fixture could not name its ask tool",
        )
        .with_cause(&e)
    })?;
    let ask = ToolInfo::new(
        id,
        "Wait for the operator's next message.",
        json!({ "type": "object", "properties": {} }),
    )
    .survives_stop(true);
    Ok(Arc::new(Asker {
        tools: vec![ask],
        broker,
    }))
}

impl Pack for Asker {
    fn tools(&self) -> Vec<ToolInfo> {
        self.tools.clone()
    }

    fn call<'a>(
        &'a self,
        _cx: ToolContext<'a>,
        _args: Value,
    ) -> PackFuture<'a, Result<ToolOutput, Error>> {
        Box::pin(async move { self.broker.wait().await.map(ToolOutput::trusted) })
    }
}
