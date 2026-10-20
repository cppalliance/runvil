//! The fixture ask Pack the Harness suites build in place of a real
//! user-input Pack, which Harness crates may not name: an ask tool that
//! survives stops, a prelude defining `input.ask()`, a need for the run's
//! broker, and a broker that fails with a `ToolError`. A prompt declares
//! it as `tests/user-input`.

use std::sync::Arc;

use runvil_plugin::{
    HostServices, Pack, PackFuture, PackId, PackInfo, ServiceId, ServiceKey, ToolContext,
    ToolError, ToolId, ToolInfo, ToolOutput,
};
use serde_json::{Value, json};

/// The `PackInfo`, whose Pack id is `user-input`.
pub(crate) const ASKER: PackInfo = PackInfo::new("tests/user-input", construct)
    .prelude(PRELUDE)
    .needs(NEEDS);

/// The key the run's broker is provided under.
pub(crate) const BROKER: ServiceKey<dyn AskBroker> = ServiceKey::new("tests/input-broker");

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

/// Run services holding `broker` under [`BROKER`].
pub(crate) fn broker_services(broker: Arc<dyn AskBroker>) -> HostServices {
    let mut services = HostServices::new();
    services.provide(&BROKER, broker).unwrap();
    services
}

struct Asker {
    tools: Vec<ToolInfo>,
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "the PackInfo construct signature fixes the return type"
)]
fn construct(
    name: &PackId,
    _config: Value,
    _services: &HostServices,
) -> Result<Arc<dyn Pack>, ToolError> {
    let ask = ToolInfo::new(
        ToolId::parse(&format!("{name}/ask")).unwrap(),
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
