//! The context a tool call lends its Pack: the Pack writes the run's
//! files through `cx.access()`, the calling section reads what it wrote
//! on its next step, and `cx.origin()` names the section's script as the
//! caller.

use std::sync::Arc;

use harness_runner::effect_loop::drive_run;
use harness_runner::recorder::MemoryRecorder;
use runvil::cancel::CancelHandle;
use runvil::error::{Error, ErrorCode};
use runvil::tools::tool_error;
use runvil_plugin::{
    HostServices, Pack, PackFuture, PackId, PackInfo, ToolContext, ToolId, ToolInfo, ToolOutput,
};
use serde_json::{Value, json};

use crate::prepare::{begin_and_prepare, building, services};

/// What the fixture tool writes to `/from-tool.md`.
const WRITTEN: &str = "written through the context";

/// A prompt calling the fixture tool by id from the section's script and
/// returning its answer beside the file it wrote.
const CALLS_RECORD: &str = "---\nname: calls-record\ndescription: d\nrunvil: 0\n\
    uses:\n  - tests/context\n---\n\n\
    # Title\n\n## Only\n\n```lua\nlocal caller = tools.call('context/record')\n\
    return caller .. '|' .. store.read('from-tool.md')\n```\n";

/// The fixture Pack `context`: one tool, `context/record`, which writes
/// [`WRITTEN`] to `/from-tool.md` through the access its call lends it
/// and answers with the caller its origin names.
const CONTEXT: PackInfo = PackInfo::new("tests/context", construct);

struct Record {
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
) -> Result<Arc<dyn Pack>, Error> {
    let record = ToolInfo::new(
        ToolId::parse(&format!("{name}/record")).unwrap(),
        "Write a file through the call's access and name the caller.",
        json!({ "type": "object", "properties": {} }),
    );
    Ok(Arc::new(Record {
        tools: vec![record],
    }))
}

impl Pack for Record {
    fn tools(&self) -> Vec<ToolInfo> {
        self.tools.clone()
    }

    fn call<'a>(
        &'a self,
        cx: ToolContext<'a>,
        _args: Value,
    ) -> PackFuture<'a, Result<ToolOutput, Error>> {
        Box::pin(async move {
            cx.access()
                .write("/from-tool.md", WRITTEN.as_bytes())
                .map_err(|error| tool_error(ErrorCode::ToolOther, format!("record: {error}")))?;
            Ok(ToolOutput::trusted(format!("{:?}", cx.origin().caller)))
        })
    }
}

#[tokio::test]
async fn a_tool_writes_through_its_context_and_the_calling_script_reads_it_back() {
    let recorder = Arc::new(MemoryRecorder::new());
    let prepared = begin_and_prepare(CALLS_RECORD, services(&recorder, building(CONTEXT)))
        .await
        .unwrap();
    let result = drive_run(
        prepared.run,
        prepared.performers,
        recorder.clone(),
        prepared.run_id,
        CancelHandle::new(),
    )
    .await;
    assert_eq!(
        result,
        Ok(format!("Script|{WRITTEN}")),
        "the tool's origin names the script, and the section reads the file the tool wrote"
    );
}
