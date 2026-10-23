//! The agent conversations' Harness setup: after the server calls the
//! three Plugins and builds its Host, every run holds the `web`,
//! `user-input`, `subagents`, and `mcp` Packs, and the ask is the fixed
//! `user-input/ask` of vendor `runvil`. A prompt declaring `web` or the built-in
//! `subagents` prepares over the server's built Packs, a Host without the
//! search provider builds web as unavailable and refuses the prompt
//! naming that service, and outside a runtime the services leave the
//! runtime out. The `mcp.json` reader returns the `mcpServers` object as
//! it stands, or an empty one, and a prompt declaring `mcp` prepares,
//! because the one mcp Pack reads the runtime the server provides. The
//! servers value is the mcp Pack's configuration, so one it refuses
//! refuses a prompt requiring `mcp`.

use std::sync::Arc;
use std::time::Duration;

use harness::pack::{HostContext, HostServices};
use harness::record::{MemoryRecorder, RunOutcome};
use harness::vfs::VfsRef;
use harness::{BoxFuture, Harness, HostSnapshot, InferenceBroker, RunRequest};
use harness_gateway_client::{CompletionError, CompletionErrorKind};
use plugin_web::SEARCH_PROVIDER;
use runvil::effect::Round;
use runvil::model::{
    Completion, CompletionOptions, Message, ModelBinding, ModelCatalog, ToolSchema,
};
use runvil::tools::ToolId;
use runvil_plugin::{ServiceKey, TOKIO_RUNTIME};
use serde_json::json;
use workshop_agents::{Conversations, SessionState, TokioTimer};
use workshop_registry::Registry;

use super::wire::AskTool;
use super::{host_context, read_mcp_servers, services, with_plugins};

/// The runtime's key, built from the Plugin contract's name as any Host
/// builds it.
const RUNTIME: ServiceKey<tokio::runtime::Handle> = ServiceKey::new(TOKIO_RUNTIME);

/// A prompt that requires the Pack `pack` and returns a fixed text.
fn requiring(pack: &str) -> String {
    format!(
        "---\nname: browses\ndescription: needs {pack}\nrunvil: 0\n\
        uses:\n  - {pack}\n---\n\n\
        # Browses\n\n## Only\n\n```lua\nreturn 'browsed'\n```\n"
    )
}

/// Lists no model and refuses every round as `Unavailable`, so a run
/// binds no model and nothing is fetched.
struct OfflineBroker;

impl InferenceBroker for OfflineBroker {
    fn models(&self) -> BoxFuture<Result<ModelCatalog, CompletionError>> {
        Box::pin(async { Ok(ModelCatalog::empty()) })
    }

    fn chat(
        &self,
        _binding: ModelBinding,
        _messages: Vec<Message>,
        _tools: Vec<ToolSchema>,
        _options: CompletionOptions,
        _round: Round,
    ) -> BoxFuture<Result<Box<Completion>, CompletionError>> {
        let kind = CompletionErrorKind::Unavailable;
        Box::pin(async move { Err(CompletionError::new(kind, kind.phrase())) })
    }
}

/// Runs the prompt `source` as one conversation over `host`, then reads
/// the conversation's state and the outcome of its one run.
async fn run_prompt(host: &Arc<HostContext>, source: String) -> Option<RunOutcome> {
    let recorder = Arc::new(MemoryRecorder::new());
    let conversation = Conversations::new().open("browses");
    let harness = Harness::new(
        conversation.recorder(recorder.clone()),
        Arc::new(OfflineBroker),
        Arc::new(TokioTimer),
        Arc::clone(host),
        conversation.run_services(),
    );
    let request = RunRequest {
        name: conversation.id().to_string(),
        source,
        args: String::new(),
        input_text: None,
        vfs: VfsRef::default(),
        host: HostSnapshot::default(),
    };
    tokio::time::timeout(Duration::from_secs(10), conversation.run(harness, request))
        .await
        .expect("the run ends in time");
    assert_eq!(
        conversation.state(),
        SessionState::Closed,
        "the conversation ends with its run"
    );
    recorder.outcome(conversation.run_id().expect("the recorder began the run"))
}

/// An `mcp.json` file in a new directory holding `text`.
fn mcp_file(text: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let path = dir.path().join("mcp.json");
    std::fs::write(&path, text).expect("write fixture");
    (dir, path)
}

#[tokio::test]
async fn every_run_holds_the_packs_the_three_plugins_add_and_the_builtin() {
    let lists = "---\nname: lists\ndescription: reads its Packs\nrunvil: 0\n---\n\n\
        # Lists\n\n## Only\n\n```lua\n\
        local names = {}\n\
        for _, id in ipairs({ 'web', 'user-input', 'subagents', 'mcp' }) do\n\
          names[#names + 1] = packs.get(id).name\n\
        end\n\
        return table.concat(names, ',')\n```\n";
    let (host, _ask) = host_context(&Registry::new(), None);
    assert_eq!(
        run_prompt(&Arc::new(host), lists.to_owned()).await,
        Some(RunOutcome::Completed {
            final_text: "web,user-input,subagents,mcp".to_owned(),
        }),
        "a run that declares nothing holds every runvil Pack the server built"
    );
}

#[tokio::test]
async fn a_prompt_declaring_web_prepares_on_the_servers_built_packs() {
    let (host, _ask) = host_context(&Registry::new(), None);
    assert_eq!(
        run_prompt(&Arc::new(host), requiring("web")).await,
        Some(RunOutcome::Completed {
            final_text: "browsed".to_owned(),
        }),
        "the server adds web and provides both services its construct reads"
    );
}

#[tokio::test]
async fn a_prompt_declaring_subagents_prepares_on_the_servers_built_packs() {
    let (host, _ask) = host_context(&Registry::new(), None);
    assert_eq!(
        run_prompt(&Arc::new(host), requiring("subagents")).await,
        Some(RunOutcome::Completed {
            final_text: "browsed".to_owned(),
        }),
        "the server's Host builds the built-in subagents"
    );
}

#[tokio::test]
async fn a_host_without_the_search_provider_refuses_a_prompt_requiring_web() {
    let mut runtime_only = HostServices::new();
    runtime_only
        .provide(&RUNTIME, Arc::new(tokio::runtime::Handle::current()))
        .expect("an empty map takes the runtime");
    let (host, _ask) = with_plugins(runtime_only, json!({}));

    let outcome = run_prompt(&Arc::new(host), requiring("web")).await;
    let Some(RunOutcome::Failed { kind, message }) = outcome else {
        panic!("the run is refused as it prepares: {outcome:?}");
    };
    assert_eq!(kind, "RequirementsUnmet");
    assert!(
        message.contains(
            "- web is unavailable: web needs runvil/search-provider, and this host provides none"
        ),
        "the refusal names the Pack and the missing service: {message}"
    );
}

#[test]
fn the_ask_is_the_runvil_vendors_fixed_user_input_ask_tool() {
    let (_host, ask) = host_context(&Registry::new(), None);
    assert_eq!(
        ask,
        Some(AskTool {
            vendor: "runvil".to_owned(),
            id: ToolId::parse("user-input/ask").expect("the id parses"),
        }),
        "the user-input Plugin added runvil/user-input, whose Pack id is user-input"
    );
}

#[test]
fn outside_a_runtime_the_services_leave_the_runtime_out() {
    let services = services(&Registry::new());
    assert!(services.provides(&SEARCH_PROVIDER.id()));
    assert!(
        !services.provides(&RUNTIME.id()),
        "a synchronous caller has no runtime to provide"
    );
}

#[tokio::test]
async fn a_prompt_declaring_mcp_prepares_on_the_runtime_the_server_provides() {
    // The server's own `services` provides the runtime, and `plugin-mcp`
    // reads it through a key of its own even with no servers, so only a
    // run over both shows the two keys name the same service.
    let (host, _ask) = host_context(&Registry::new(), None);
    assert_eq!(
        run_prompt(&Arc::new(host), requiring("mcp")).await,
        Some(RunOutcome::Completed {
            final_text: "browsed".to_owned(),
        }),
        "the server adds mcp once, and its construct finds the server's runtime"
    );
}

#[tokio::test]
async fn the_servers_value_is_the_mcp_packs_configuration() {
    // A value that is neither null nor an object is the one configuration
    // the construct refuses, so only that value reaching it fails the run.
    let (host, _ask) = with_plugins(services(&Registry::new()), json!([]));

    let outcome = run_prompt(&Arc::new(host), requiring("mcp")).await;
    let Some(RunOutcome::Failed { kind, message }) = outcome else {
        panic!("the run is refused as it prepares: {outcome:?}");
    };
    assert_eq!(kind, "RequirementsUnmet");
    assert!(
        message.contains("- mcp is unavailable: the MCP configuration must be a JSON object"),
        "the refusal names the mcp Pack and its configuration rule: {message}"
    );
}

#[test]
fn no_setting_reads_no_file_and_gives_no_servers() {
    assert_eq!(read_mcp_servers(None), json!({}));
}

#[test]
fn a_missing_mcp_file_gives_no_servers() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    assert_eq!(
        read_mcp_servers(Some(&dir.path().join("absent.json"))),
        json!({})
    );
}

#[test]
fn an_unreadable_mcp_file_gives_no_servers() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    // A directory in the file's place fails the read for a reason other
    // than a missing file.
    let path = dir.path().join("mcp.json");
    std::fs::create_dir(&path).expect("directory in the file's place");
    assert_eq!(read_mcp_servers(Some(&path)), json!({}));
}

#[test]
fn a_malformed_mcp_file_gives_no_servers() {
    for text in ["{ \"mcpServers\": ", "[]", "{ \"mcpServers\": [] }"] {
        let (_dir, path) = mcp_file(text);
        assert_eq!(
            read_mcp_servers(Some(&path)),
            json!({}),
            "`{text}` is not an object with an mcpServers object"
        );
    }
}

#[test]
fn a_file_without_mcp_servers_gives_no_servers() {
    let (_dir, path) = mcp_file(r#"{ "inputs": [] }"#);
    assert_eq!(read_mcp_servers(Some(&path)), json!({}));
}

#[test]
fn an_entry_that_is_not_an_object_is_handed_on_for_the_pack_to_refuse() {
    let (_dir, path) = mcp_file(r#"{ "mcpServers": { "a": 1, "b": { "url": "x" } } }"#);
    assert_eq!(
        read_mcp_servers(Some(&path)),
        json!({ "a": 1, "b": { "url": "x" } }),
        "one bad entry does not drop the file's other servers"
    );
}

#[test]
fn a_valid_mcp_file_reads_back_its_servers_object_and_ignores_other_keys() {
    let (_dir, path) = mcp_file(
        r#"{
            "inputs": [{ "id": "token" }],
            "mcpServers": {
                "wg21": { "url": "https://example.test/mcp" },
                "GitHub": { "command": "x", "args": ["a"] },
                "docs": { "url": "https://example.test/docs" }
            }
        }"#,
    );
    assert_eq!(
        read_mcp_servers(Some(&path)),
        json!({
            "wg21": { "url": "https://example.test/mcp" },
            "GitHub": { "command": "x", "args": ["a"] },
            "docs": { "url": "https://example.test/docs" }
        }),
        "the servers object is handed on verbatim, names and entries alike"
    );
}
