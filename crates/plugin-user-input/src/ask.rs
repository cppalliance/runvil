//! The Pack object and the `construct` that builds it: one ask tool,
//! named under the Pack id `construct` receives.

use std::sync::Arc;

use runvil_plugin::{
    HostServices, Pack, PackFuture, PackId, ToolContext, ToolError, ToolErrorKind, ToolId,
    ToolInfo, ToolOutput,
};
use serde_json::{Value, json};

use crate::{ASK, INPUT_BROKER};

/// The one object every run shares: its one-tool list.
struct UserInput {
    tools: Vec<ToolInfo>,
}

/// Builds the Pack under `name`. It takes no configuration and reads
/// no Host-wide service; the broker arrives with each run.
#[expect(
    clippy::needless_pass_by_value,
    reason = "the PackInfo construct signature fixes the argument types"
)]
pub(crate) fn construct(
    name: &PackId,
    config: Value,
    _services: &HostServices,
) -> Result<Arc<dyn Pack>, ToolError> {
    if !(config.is_null() || config == json!({})) {
        return Err(ToolError::message("user-input takes no configuration"));
    }
    let id = ToolId::parse(&format!("{name}/{ASK}")).map_err(|e| {
        ToolError::new(
            ToolErrorKind::Backend,
            "user-input could not name its ask tool",
        )
        .with_source(e)
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
    ) -> PackFuture<'a, Result<ToolOutput, ToolError>> {
        Box::pin(async move {
            // The Pack's needs keep a run without a broker from seeing
            // the ask tool, so this branch is defensive.
            let broker = cx
                .service(&INPUT_BROKER)
                .ok_or_else(|| ToolError::message("no operator is connected to this run"))?;
            broker.wait().await.map(ToolOutput::trusted)
        })
    }
}
