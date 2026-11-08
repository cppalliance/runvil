//! The fixture ask Pack the Harness suites build in place of a real
//! user-input Pack, which Harness crates may not name: an ask tool that
//! survives stops, a prelude defining `input.ask()`, a factory that takes
//! the run's broker and refuses a run without one, and a broker that
//! fails with a tool error. A prompt declares it as `tests/user-input`.

use std::sync::Arc;

use runvil::make_tool_error;
use runvil::{Error, ErrorCode};
use runvil_plugin::{
    HostServices, Pack, PackFuture, PackId, PackInfo, ServiceKey, ToolContext, ToolId, ToolInfo,
    ToolOutput,
};
use serde_json::{Value, json};

use crate::support::Make;

/// The `PackInfo`, whose Pack id is `user-input`, beside its factory.
pub(crate) const ASKER: (PackInfo, Make) = (
    PackInfo::new("tests/user-input").prelude(PRELUDE),
    new_asker,
);

/// The key the run's broker is provided under.
pub(crate) const BROKER: ServiceKey<dyn AskBroker> = ServiceKey::new("tests/input-broker");

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

/// Run services holding `broker` under [`BROKER`].
pub(crate) fn broker_services(broker: Arc<dyn AskBroker>) -> HostServices {
    let mut services = HostServices::new();
    services.provide(&BROKER, broker).unwrap();
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
    let ask = ToolInfo::new(
        ToolId::parse(&format!("{name}/ask")).unwrap(),
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
