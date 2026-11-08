//! The Pack instance and the factory that makes it: one ask tool, named
//! under the Pack id the factory receives, and the run's input broker.

use std::sync::Arc;

use runvil_plugin::{
    Error, ErrorCode, HostServices, Pack, PackFuture, PackId, ToolContext, ToolId, ToolInfo,
    ToolOutput, make_tool_error,
};
use serde_json::{Value, json};

use crate::{ASK, INPUT_BROKER, InputBroker};

/// One run's instance: its one-tool list and the broker it took from the
/// run.
struct UserInput {
    tools: Vec<ToolInfo>,
    broker: Arc<dyn InputBroker>,
}

/// Makes this run's `runvil/user-input` instance named `id`, holding the
/// input broker from `run`. Fails with "user-input needs
/// runvil/input-broker, and this run provides none" when `run` has none.
pub(crate) fn new_pack(id: &PackId, run: &HostServices) -> Result<Arc<dyn Pack>, Error> {
    let broker = run.get(&INPUT_BROKER).ok_or_else(|| {
        make_tool_error(
            ErrorCode::ToolOther,
            "user-input needs runvil/input-broker, and this run provides none",
        )
    })?;
    let id = ToolId::parse(&format!("{id}/{ASK}")).map_err(|e| {
        make_tool_error(
            ErrorCode::ToolBackend,
            "user-input could not name its ask tool",
        )
        .with_cause(&e)
    })?;
    let ask = ToolInfo::new(
        id,
        "Wait for the operator's next message and return its text.",
        json!({ "type": "object", "properties": {} }),
    )
    .survives_stop(true);
    Ok(Arc::new(UserInput {
        tools: vec![ask],
        broker,
    }))
}

impl Pack for UserInput {
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
