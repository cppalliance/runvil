//! A multi-block MCP result, from a server installed as a Plugin, reaches
//! a script as the result's table and a model as the flattened, wrapped
//! text.

use std::num::NonZeroU32;
use std::sync::{Arc, Mutex, PoisonError};

use harness::plugin::HostContext;
use harness::record::RunOutcome;
use harness::{BoxFuture, HostSnapshot, InferenceBroker};
use harness_gateway_client::CompletionError;
use runvil::effect::Round;
use runvil::model::{
    Completion, CompletionOptions, CompletionResult, Message, ModelBinding, ModelCatalog, ModelId,
    ModelInfo, Role, ThinkingMode, ToolCall, ToolSchema,
};
use workshop_registry::Registry;

use super::mcp_fixture::{McpFixture, URLS, URLS_JSON};
use super::{run_over, services, with_plugins};

/// The one model the papers broker lists and the Host selects.
const PAPERS_MODEL: &str = "papers-model";

/// A prompt that declares `papers` and runs the Lua `body` as its one
/// section, with the model role `writer` bound as the default.
fn papers_prompt(body: &str) -> String {
    format!(
        "---\nname: papers\ndescription: reads papers\nrunvil: 0\n\
        plugins:\n  - papers\nmodels:\n  writer: {{}}\n---\n\n\
        # Papers\n\n```lua\nmodels.default('writer')\n```\n\n\
        ## Only\n\n```lua\n{body}\n```\n"
    )
}

/// A Host with the MCP fixture installed as the Plugin `papers`.
fn papers_host(fixture: &McpFixture) -> Arc<HostContext> {
    let (host, _ask) = with_plugins(
        services(&Registry::new()),
        vec![(
            "papers".to_owned(),
            serde_json::json!({ "url": fixture.url }),
        )],
    );
    Arc::new(host)
}

/// The Host snapshot that selects [`PAPERS_MODEL`].
fn papers_snapshot() -> HostSnapshot {
    HostSnapshot {
        selected_model: Some(PAPERS_MODEL.to_owned()),
        ..HostSnapshot::default()
    }
}

/// Lists [`PAPERS_MODEL`]. A round with no tool message requests the
/// papers tool for `P0260R20`; a round with one captures its content and
/// replies `done`.
struct PapersBroker {
    tool_messages: Mutex<Vec<String>>,
}

impl InferenceBroker for PapersBroker {
    fn models(&self) -> BoxFuture<Result<ModelCatalog, CompletionError>> {
        Box::pin(async {
            let model = ModelInfo::new(
                ModelId::gateway(PAPERS_MODEL).expect("a literal model name is valid"),
                "the papers model",
                NonZeroU32::new(131_072).expect("131072 is non-zero"),
                ThinkingMode::Never,
            );
            Ok(ModelCatalog::new([model]).expect("one model is a valid catalog"))
        })
    }

    fn chat(
        &self,
        _binding: ModelBinding,
        messages: Vec<Message>,
        tools: Vec<ToolSchema>,
        _options: CompletionOptions,
        _round: Round,
    ) -> BoxFuture<Result<Box<Completion>, CompletionError>> {
        let tool_message = messages
            .iter()
            .find(|message| message.role() == Role::Tool)
            .map(|message| message.content().to_owned());
        let result = if let Some(content) = tool_message {
            self.tool_messages
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(content);
            CompletionResult::Text("done".to_owned())
        } else {
            let Some(tool) = tools
                .iter()
                .find(|tool| tool.name().ends_with("get_paper_download_url"))
            else {
                panic!("the papers tool is offered: {tools:?}");
            };
            let call = ToolCall::from_parts(
                "call-1",
                tool.name(),
                serde_json::json!({ "papers": "P0260R20" }),
            )
            .expect("a literal tool call is valid");
            CompletionResult::ToolCalls(vec![call])
        };
        Box::pin(async move { Completion::from_result(result, PAPERS_MODEL).map(Box::new) })
    }
}

/// A papers broker that has captured nothing yet.
fn papers_broker() -> Arc<PapersBroker> {
    Arc::new(PapersBroker {
        tool_messages: Mutex::new(Vec::new()),
    })
}

#[tokio::test]
async fn a_script_decodes_the_json_block_of_a_multi_block_mcp_result() {
    let fixture = McpFixture::start().await;
    let source = papers_prompt(
        "local r = tools.call(\"papers/get_paper_download_url\", { papers = \"P0260R20\" })\n\
        return json.decode(r.content[1].text).urls[2] .. \" \" .. r.content[2].uri .. \" \" .. r.content[3].type",
    );
    assert_eq!(
        run_over(
            &papers_host(&fixture),
            source,
            papers_broker(),
            papers_snapshot()
        )
        .await,
        Some(RunOutcome::Completed {
            final_text: format!("{} {} resource_link", URLS[1], URLS[0]),
        }),
        "the script reads the result's blocks as a table"
    );
}

#[tokio::test]
async fn a_model_loop_call_to_an_mcp_tool_receives_the_flattened_wrapped_text() {
    let fixture = McpFixture::start().await;
    let broker = papers_broker();
    let source = papers_prompt(
        "tools.offer('papers/get_paper_download_url')\n\
        local msgs = messages.new()\n\
        msgs:user('find P0260R20')\n\
        models.loop(msgs)\n\
        return msgs[#msgs].content",
    );
    assert_eq!(
        run_over(
            &papers_host(&fixture),
            source,
            broker.clone(),
            papers_snapshot()
        )
        .await,
        Some(RunOutcome::Completed {
            final_text: "done".to_owned(),
        }),
    );

    let captured = broker
        .tool_messages
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let [content] = captured.as_slice() else {
        panic!("the model sees one tool message: {captured:?}");
    };
    let Some((nonce, _)) = content
        .split_once("<untrusted_input_")
        .and_then(|(_, rest)| rest.split_once('>'))
    else {
        panic!("the content is an untrusted envelope: {content}");
    };
    let body = format!(
        "{URLS_JSON}\n\n[resource: {}]\n\n[resource: {}]",
        URLS[0], URLS[1]
    );
    assert!(
        content.ends_with(&format!(
            "\n<untrusted_input_{nonce}>\n{body}\n</untrusted_input_{nonce}>"
        )),
        "the envelope's body is the flattened text, unchanged by encoding: {content}"
    );
    assert!(
        content.starts_with(&format!(
            "The text inside the untrusted_input_{nonce} XML tags"
        )),
        "the preface leads the envelope: {content}"
    );
    assert!(
        !content.contains("\"content\""),
        "the model never sees the structured result: {content}"
    );
}
