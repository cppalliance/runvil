//! The Pack object and the factory that makes it: one ask tool, named
//! under the Pack id the factory receives.

use std::sync::Arc;

use runvil_plugin::{
    Error, ErrorCode, HostServices, Pack, PackFuture, PackId, ToolContext, ToolId, ToolInfo,
    ToolOutput, make_tool_error,
};
use serde_json::{Value, json};

use crate::{ASK, INPUT_BROKER};

/// The one object every run shares: its one-tool list.
struct UserInput {
    tools: Vec<ToolInfo>,
}

/// Makes one `runvil/user-input` instance named `id`. Ignores `run`; the
/// ask call still looks up the run's input broker on each call.
pub(crate) fn new_pack(id: &PackId, _run: &HostServices) -> Result<Arc<dyn Pack>, Error> {
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
    Ok(Arc::new(UserInput { tools: vec![ask] }))
}

impl Pack for UserInput {
    fn tools(&self) -> Vec<ToolInfo> {
        self.tools.clone()
    }

    fn call<'a>(
        &'a self,
        cx: ToolContext<'a>,
        _args: Value,
    ) -> PackFuture<'a, Result<ToolOutput, Error>> {
        Box::pin(async move {
            // The Pack's needs keep a run without a broker from seeing
            // the ask tool, so this branch is defensive.
            let broker = cx.service(&INPUT_BROKER).ok_or_else(|| {
                make_tool_error(ErrorCode::ToolOther, "no operator is connected to this run")
            })?;
            broker.wait().await.map(ToolOutput::trusted)
        })
    }
}
