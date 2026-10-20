//! The fixture ask Pack a suite Host builds in place of a real
//! user-input Pack, which Harness crates may not name: an ask tool that
//! survives stops, a prelude defining `input.ask()`, a need for the run's
//! broker, and a broker that fails with a `ToolError`. A prompt declares
//! it as `tests/user-input`.

use std::sync::Arc;

use harness::pack::{HostContext, HostServices, PackRegistry};
use runvil_plugin::{
    Pack, PackFuture, PackId, PackInfo, ServiceId, ServiceKey, ToolContext, ToolError,
    ToolErrorKind, ToolId, ToolInfo, ToolOutput,
};
use serde_json::{Value, json};

/// The `PackInfo`, whose Pack id is `user-input`.
const ASKER: PackInfo = PackInfo::new("tests/user-input", construct)
    .prelude(PRELUDE)
    .needs(NEEDS);

/// The key the run's broker is provided under.
const BROKER: ServiceKey<dyn AskBroker> = ServiceKey::new("tests/input-broker");

const NEEDS: &[ServiceId] = &[BROKER.id()];

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
    async fn wait(&self) -> Result<String, ToolError>;
}

/// A Host that built the fixture, as `user-input`.
pub(crate) fn asker_host() -> Arc<HostContext> {
    let mut packs = PackRegistry::new();
    if let Err(error) = packs.add(ASKER, Value::Null) {
        panic!("a new registry takes the fixture: {error}");
    }
    Arc::new(HostContext::new(packs, HostServices::new()))
}

/// Run services holding `broker` as the run's input broker.
pub(crate) fn broker_services(broker: Arc<dyn AskBroker>) -> HostServices {
    let mut services = HostServices::new();
    if let Err(error) = services.provide(&BROKER, broker) {
        panic!("an empty map takes the broker: {error}");
    }
    services
}

struct Asker {
    tools: Vec<ToolInfo>,
}

fn construct(
    name: &PackId,
    _config: Value,
    _services: &HostServices,
) -> Result<Arc<dyn Pack>, ToolError> {
    let id = ToolId::parse(&format!("{name}/ask")).map_err(|e| {
        ToolError::new(
            ToolErrorKind::Backend,
            "the fixture could not name its ask tool",
        )
        .with_source(e)
    })?;
    let ask = ToolInfo::new(
        id,
        "Wait for the operator's next message.",
        json!({ "type": "object", "properties": {} }),
    )
    .survives_stop(true);
    Ok(Arc::new(Asker { tools: vec![ask] }))
}

impl Pack for Asker {
    fn tools(&self) -> Vec<ToolInfo> {
        self.tools.clone()
    }

    fn call<'a>(
        &'a self,
        cx: ToolContext<'a>,
        _args: Value,
    ) -> PackFuture<'a, Result<ToolOutput, ToolError>> {
        Box::pin(async move {
            let broker = cx
                .service(&BROKER)
                .ok_or_else(|| ToolError::message("no operator is connected to this run"))?;
            broker.wait().await.map(ToolOutput::trusted)
        })
    }
}
