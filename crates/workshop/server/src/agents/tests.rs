//! The agent conversations' Harness setup: after the server calls the
//! three Plugins and builds its Host, every run holds the `web`,
//! `user-input`, `subagents`, and `mcp` Packs, and the ask is the fixed
//! `user-input/ask` of vendor `runvil`. A prompt declaring `web` or the built-in
//! `subagents` prepares over the server's built Packs, a Host without the
//! search provider builds web as unavailable and refuses the prompt as it
//! parses, naming that service, and outside a runtime the services leave
//! the runtime out. The `mcp.json` reader returns the `mcpServers` object
//! as it stands, or an empty one, and a prompt declaring `mcp` prepares,
//! because the one mcp Pack reads the runtime the server provides. The
//! servers value is the mcp Pack's configuration, so one it refuses
//! refuses a prompt requiring `mcp` as it parses. The Host's finder
//! refuses an unknown agent as `NotFound` and an unreadable one as `Io`,
//! each with the launch refusal's own text.

use std::sync::Arc;
use std::time::Duration;

use harness::pack::{HostServices, Packs};
use harness::record::MemoryRecorder;
use harness::vfs::VfsRef;
use harness::{BoxFuture, Host, InferenceBroker, RunSetup};
use plugin_web::SEARCH_PROVIDER;
use runvil::ToolId;
use runvil::effect::Round;
use runvil::model::{
    Completion, CompletionOptions, Message, ModelBinding, ModelCatalog, ToolSchema,
    make_completion_error, phrase,
};
use runvil::{Error, ErrorCode};
use runvil_plugin::{ServiceKey, TOKIO_RUNTIME};
use serde_json::json;
use workshop_agents::{Conversations, SessionState, TokioTimer, load_agent};
use workshop_registry::Registry;

use super::wire::AskTool;
use super::{agent_finder, host_packs, read_mcp_servers, services, with_plugins};

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
pub(super) struct OfflineBroker;

impl InferenceBroker for OfflineBroker {
    fn models(&self) -> BoxFuture<Result<ModelCatalog, Error>> {
        Box::pin(async { Ok(ModelCatalog::empty()) })
    }

    fn chat(
        &self,
        _binding: ModelBinding,
        _messages: Vec<Message>,
        _tools: Vec<ToolSchema>,
        _options: CompletionOptions,
        _round: Round,
    ) -> BoxFuture<Result<Box<Completion>, Error>> {
        let code = ErrorCode::Unavailable;
        Box::pin(async move { Err(make_completion_error(code, phrase(code))) })
    }
}

/// Runs the prompt `source` as one conversation over `packs` and the
/// default in-memory filesystem; see [`run_prompt_over`].
async fn run_prompt(packs: Packs, source: String) -> Result<Result<String, Error>, String> {
    run_prompt_over(packs, source, VfsRef::default()).await
}

/// Runs the prompt `source` as one conversation on a Host over `packs`
/// and the filesystem `vfs`, then reads the conversation's state and how
/// it ended: the result of its one run, or the message of the run-failed
/// report when the Host refused the prompt as it parsed it and no run
/// began.
pub(super) async fn run_prompt_over(
    packs: Packs,
    source: String,
    vfs: VfsRef,
) -> Result<Result<String, Error>, String> {
    let recorder = Arc::new(MemoryRecorder::new());
    let conversation = Conversations::new().open("browses");
    let mut errors = conversation.subscribe_errors();
    let host = Arc::new(Host {
        packs,
        prompts: Arc::new(move |_: &str| -> Result<String, Error> { Ok(source.clone()) }),
        timer: Arc::new(TokioTimer),
    });
    let setup = RunSetup {
        services: conversation.run_services(),
        vfs,
        ..RunSetup::new(
            conversation.recorder(recorder.clone()),
            Arc::new(OfflineBroker),
        )
    };
    tokio::time::timeout(
        Duration::from_secs(10),
        conversation.run(host, "browses", setup),
    )
    .await
    .expect("the run ends in time");
    assert_eq!(
        conversation.state(),
        SessionState::Closed,
        "the conversation ends with its run"
    );
    match conversation.run_id() {
        Some(run) => Ok(recorder.result(run).expect("the recorder ended the run")),
        None => Err(errors
            .try_recv()
            .expect("a prompt refused before its run is reported")
            .message),
    }
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
        for _, id in ipairs({ 'web', 'user-input', 'subagents', 'mcp', 'std' }) do\n\
          names[#names + 1] = tools.packs.get(id).name\n\
        end\n\
        return table.concat(names, ',')\n```\n";
    let (host, _ask) = host_packs(&Registry::new(), None);
    assert_eq!(
        run_prompt(host, lists.to_owned()).await,
        Ok(Ok("web,user-input,subagents,mcp,std".to_owned())),
        "a run that declares nothing holds every runvil Pack the server built"
    );
}

#[tokio::test]
async fn a_prompt_declaring_web_prepares_on_the_servers_built_packs() {
    let (host, _ask) = host_packs(&Registry::new(), None);
    assert_eq!(
        run_prompt(host, requiring("web")).await,
        Ok(Ok("browsed".to_owned())),
        "the server adds web and provides both services its construct reads"
    );
}

#[tokio::test]
async fn a_prompt_declaring_subagents_prepares_on_the_servers_built_packs() {
    let (host, _ask) = host_packs(&Registry::new(), None);
    assert_eq!(
        run_prompt(host, requiring("subagents")).await,
        Ok(Ok("browsed".to_owned())),
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

    let result = run_prompt(host, requiring("web")).await;
    let Err(message) = result else {
        panic!("the Host refuses the prompt as it parses: {result:?}");
    };
    assert!(
        message.contains(
            "- web is unavailable: web needs runvil/search-provider, and this host provides none"
        ),
        "the refusal names the Pack and the missing service: {message}"
    );
}

#[test]
fn the_ask_is_the_runvil_vendors_fixed_user_input_ask_tool() {
    let (_host, ask) = host_packs(&Registry::new(), None);
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

#[test]
fn the_finder_refuses_an_unknown_agent_as_not_found_and_an_unreadable_one_as_io() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    // Discovery lists `broken`, but its bytes are not UTF-8, so its read
    // fails.
    std::fs::write(dir.path().join("broken.md"), [0xff, 0xfe]).expect("write fixture");
    let finder = agent_finder(dir.path().to_path_buf());

    for (name, code) in [("missing", ErrorCode::NotFound), ("broken", ErrorCode::Io)] {
        let error = finder(name).expect_err("the finder refuses the agent");
        let refusal = load_agent(dir.path(), name).expect_err("the launch check refuses it too");
        assert_eq!(error.code, code, "{name}");
        assert_eq!(error.message, refusal.to_string(), "{name}");
        assert_eq!(error.prose, code.prose(), "{name}");
    }
}

#[tokio::test]
async fn a_prompt_declaring_mcp_prepares_on_the_runtime_the_server_provides() {
    // The server's own `services` provides the runtime, and `plugin-mcp`
    // reads it through a key of its own even with no servers, so only a
    // run over both shows the two keys name the same service.
    let (host, _ask) = host_packs(&Registry::new(), None);
    assert_eq!(
        run_prompt(host, requiring("mcp")).await,
        Ok(Ok("browsed".to_owned())),
        "the server adds mcp once, and its construct finds the server's runtime"
    );
}

#[tokio::test]
async fn the_servers_value_is_the_mcp_packs_configuration() {
    // A value that is neither null nor an object is the one configuration
    // the construct refuses, so only that value reaching it fails the run.
    let (host, _ask) = with_plugins(services(&Registry::new()), json!([]));

    let result = run_prompt(host, requiring("mcp")).await;
    let Err(message) = result else {
        panic!("the Host refuses the prompt as it parses: {result:?}");
    };
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
