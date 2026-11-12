//! The agent conversations' Harness setup: after the server calls the
//! two linked Plugins, loads the std Plugin's library from its Plugin
//! directory, and builds its Host, every run of a conversation, which
//! has a user, holds the `web`, `subagents`, `mcp`, `std`, and `user`
//! Packs. A prompt declaring `web` or the built-in
//! `subagents` prepares over the server's installed Packs, a Host without
//! the search provider installs web unavailable and refuses the prompt as
//! it parses, naming that service, and outside a runtime the services leave
//! the runtime out. The `mcp.json` reader returns the `mcpServers` object
//! as it stands, or an empty one, and a prompt declaring `mcp` prepares,
//! because the one mcp Pack reads the runtime the server provides. The
//! servers value is the mcp Pack's configuration, so one it refuses
//! refuses a prompt requiring `mcp` as it parses. The Host's finder
//! refuses an unknown agent as `NotFound` and an unreadable one as `Io`,
//! each with the launch refusal's own text. A configured Plugin directory
//! loads each library Plugin in it and reports each file that is none,
//! and a directory that does not exist loads nothing. A prompt reads a
//! file of its run through the std library's `Read` tool and through the
//! bashkit library's `Shell` tool, and a Host with no Plugin directory
//! refuses a prompt that requires `std` or `bash`.

use std::env::consts::{DLL_EXTENSION, DLL_PREFIX, DLL_SUFFIX};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use plugin_web::SEARCH_PROVIDER;
use runvil::effect::Round;
use runvil::harness::{InferenceBroker, MemoryRecorder, Packs, PluginSource};
use runvil::model::{
    Completion, CompletionOptions, Message, ModelBinding, ModelCatalog, ToolSchema,
    make_completion_error, phrase,
};
use runvil::plugin::{HostServices, PackRegistry, ServiceKey, TOKIO_RUNTIME};
use runvil::vfs::{Origin, VfsRef};
use runvil::{BoxFuture, Error, ErrorCode, ErrorFact, Host, RunSetup};
use serde_json::json;
use workshop_agents::{Conversations, SessionState, TokioTimer, load_agent};
use workshop_registry::Registry;

use super::{
    PluginListing, SHIPPED_PACKS, agent_finder, holds, host_packs, list_plugin_dir,
    load_plugin_dir, read_mcp_servers, services, with_plugins,
};

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
    fn models(&self) -> BoxFuture<'static, Result<ModelCatalog, Error>> {
        Box::pin(async { Ok(ModelCatalog::empty()) })
    }

    fn chat(
        &self,
        _binding: ModelBinding,
        _messages: Vec<Message>,
        _tools: Vec<ToolSchema>,
        _options: CompletionOptions,
        _round: Round,
    ) -> BoxFuture<'static, Result<Box<Completion>, Error>> {
        let code = ErrorCode::Unavailable;
        Box::pin(async move { Err(make_completion_error(code, phrase(code))) })
    }
}

/// Runs the prompt `source` as one conversation over `packs` and the
/// default in-memory filesystem; see [`run_prompt_over`].
async fn run_prompt(packs: Packs, source: String) -> Result<Result<String, Error>, String> {
    run_prompt_over(packs, source, VfsRef::default()).await
}

/// Runs the prompt `source` as one conversation, which has a user, on a
/// Host over `packs` and the filesystem `vfs`, then reads the
/// conversation's state and how it ended: the result of its one run, or
/// the message of the run-failed report when the Host refused the prompt
/// as it parsed it and no run began.
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
        vfs,
        has_user: true,
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
async fn every_run_holds_the_packs_the_three_plugins_add_and_the_builtins() {
    let lists = "---\nname: lists\ndescription: reads its Packs\nrunvil: 0\n---\n\n\
        # Lists\n\n## Only\n\n```lua\n\
        local names = {}\n\
        for _, id in ipairs({ 'web', 'subagents', 'mcp', 'std', 'user' }) do\n\
          names[#names + 1] = tools.packs.get(id).name\n\
        end\n\
        return table.concat(names, ',') .. '|' .. tostring(tools.packs.get('user-input'))\n```\n";
    let (dir, _library) = plugin_dir("every-run-holds");
    let (host, _) = host_packs(&Registry::new(), None, Some(dir.path()));
    assert_eq!(
        run_prompt(host, lists.to_owned()).await,
        Ok(Ok("web,subagents,mcp,std,user|nil".to_owned())),
        "a run that declares nothing holds every runvil Pack the server installed, the \
         built-ins, and the user Pack its user brings, and no user-input Pack"
    );
}

#[tokio::test]
async fn a_prompt_declaring_web_prepares_on_the_servers_built_packs() {
    let (host, _) = host_packs(&Registry::new(), None, None);
    assert_eq!(
        run_prompt(host, requiring("web")).await,
        Ok(Ok("browsed".to_owned())),
        "the server adds web and provides both services its install reads"
    );
}

#[tokio::test]
async fn a_prompt_declaring_subagents_prepares_on_the_servers_built_packs() {
    let (host, _) = host_packs(&Registry::new(), None, None);
    assert_eq!(
        run_prompt(host, requiring("subagents")).await,
        Ok(Ok("browsed".to_owned())),
        "the server's Host installs the built-in subagents"
    );
}

#[tokio::test]
async fn a_host_without_the_search_provider_refuses_a_prompt_requiring_web() {
    let mut runtime_only = HostServices::new();
    runtime_only
        .provide(&RUNTIME, Arc::new(tokio::runtime::Handle::current()))
        .expect("an empty map takes the runtime");
    let (host, _) = with_plugins(&runtime_only, json!({}), None);

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
    let (host, _) = host_packs(&Registry::new(), None, None);
    assert_eq!(
        run_prompt(host, requiring("mcp")).await,
        Ok(Ok("browsed".to_owned())),
        "the server adds mcp once, and its install finds the server's runtime"
    );
}

#[tokio::test]
async fn the_servers_value_is_the_mcp_packs_configuration() {
    // A value that is neither null nor an object is the one configuration
    // the install refuses, so only that value reaching it fails the run.
    let (host, _) = with_plugins(&services(&Registry::new()), json!([]), None);

    let result = run_prompt(host, requiring("mcp")).await;
    let Err(message) = result else {
        panic!("the Host refuses the prompt as it parses: {result:?}");
    };
    assert!(
        message.contains("- mcp is unavailable: the MCP configuration must be a JSON object"),
        "the refusal names the mcp Pack and its configuration rule: {message}"
    );
}

/// Counts the Plugin directories this process has made, so two tests
/// never share one.
static PLUGIN_DIRS: AtomicUsize = AtomicUsize::new(0);

/// The std Plugin's library, as Cargo names it.
const STD_LIBRARY: &str = "plugin_std";

/// The bashkit Plugin's library, as Cargo names it.
const BASHKIT_LIBRARY: &str = "plugin_bashkit";

/// The file name of the dynamic library `library`.
fn library_file(library: &str) -> String {
    format!("{DLL_PREFIX}{library}{DLL_SUFFIX}")
}

/// A Plugin directory for the test named `test`, holding a copy of the
/// dynamic library `library`, and that copy's path. Cargo builds the
/// library beside this test executable, because the crate depends on the
/// `workshop-plugin-*` crate that exports it. The directory's name comes
/// from the test, the process id, and a counter.
fn plugin_dir_of(test: &str, library: &str) -> (tempfile::TempDir, PathBuf) {
    let prefix = format!(
        "{test}-{}-{}-",
        std::process::id(),
        PLUGIN_DIRS.fetch_add(1, Ordering::Relaxed)
    );
    let dir = tempfile::Builder::new()
        .prefix(&prefix)
        .tempdir()
        .expect("a temporary directory is made");
    let exe = std::env::current_exe().expect("the test executable has a path");
    let built = exe
        .parent()
        .expect("the test executable has a directory")
        .join(library_file(library));
    let copy = dir.path().join(library_file(library));
    std::fs::copy(built, &copy).expect("the Plugin's library is copied");
    (dir, copy)
}

/// A Plugin directory for the test named `test`, holding a copy of the
/// std Plugin's library, and that copy's path.
pub(super) fn plugin_dir(test: &str) -> (tempfile::TempDir, PathBuf) {
    plugin_dir_of(test, STD_LIBRARY)
}

#[test]
fn a_plugin_directory_loads_the_std_plugin_from_its_library() {
    let (dir, library) = plugin_dir("loads-std");
    let mut packs = PackRegistry::new();
    let report = load_plugin_dir(&mut packs, dir.path()).expect("the directory is read");

    assert!(report.failures().is_empty(), "{:?}", report.failures());
    let [plugin] = report.plugins() else {
        panic!("one library loads one Plugin: {:?}", report.plugins());
    };
    assert_eq!(plugin.name(), "std");
    assert_eq!(plugin.version(), "0.1.0");
    assert_eq!(
        plugin.source(),
        &PluginSource::Library(std::path::absolute(&library).expect("the path is made absolute"))
    );
}

#[test]
fn a_file_that_is_no_library_is_reported_and_the_std_plugin_still_loads() {
    let (dir, _library) = plugin_dir("reports-a-bad-file");
    let bad = dir.path().join(format!("a-text-file.{DLL_EXTENSION}"));
    std::fs::write(&bad, "not a library").expect("write fixture");
    let mut packs = PackRegistry::new();
    let report = load_plugin_dir(&mut packs, dir.path()).expect("the directory is read");

    assert_eq!(report.plugins().len(), 1, "{:?}", report.plugins());
    let [failure] = report.failures() else {
        panic!("the text file is the one failure: {:?}", report.failures());
    };
    assert_eq!(failure.code, ErrorCode::PluginLoadFailed);
    assert_eq!(failure.facts.get(&ErrorFact::Reason), Some(&json!("open")));
    let expected = std::path::absolute(&bad).expect("the path is made absolute");
    assert_eq!(
        failure.facts.get(&ErrorFact::Path),
        Some(&json!(expected.display().to_string()))
    );
}

#[test]
fn a_listing_names_each_plugin_that_loads_and_each_library_that_does_not() {
    let (dir, _library) = plugin_dir("lists-std");
    assert_eq!(
        list_plugin_dir(dir.path()).expect("the directory is read"),
        PluginListing {
            loaded: vec!["std 0.1.0".to_owned()],
            failed: Vec::new(),
        }
    );

    let bad = dir.path().join(format!("a-text-file.{DLL_EXTENSION}"));
    std::fs::write(&bad, "not a library").expect("write fixture");
    let listing = list_plugin_dir(dir.path()).expect("the directory is read");
    assert_eq!(listing.loaded, ["std 0.1.0"]);
    let [failed] = listing.failed.as_slice() else {
        panic!("the text file is the one failure: {:?}", listing.failed);
    };
    let path = std::path::absolute(&bad).expect("the path is made absolute");
    assert!(
        failed.starts_with(&format!("{}: ", path.display())),
        "the line opens with the library's path: {failed}"
    );

    let missing = list_plugin_dir(&dir.path().join("absent")).expect_err("no such directory");
    assert_eq!(missing.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn a_plugin_directory_that_does_not_exist_loads_nothing() {
    let (dir, _library) = plugin_dir("missing-directory");
    let mut packs = PackRegistry::new();
    assert!(
        load_plugin_dir(&mut packs, &dir.path().join("absent")).is_none(),
        "a missing directory is a state, not a failure"
    );
}

#[tokio::test]
async fn the_servers_host_loads_its_plugin_directory_beside_the_linked_plugins() {
    let (dir, _library) = plugin_dir("server-host");
    let (host, report) = host_packs(&Registry::new(), None, Some(dir.path()));

    let report = report.expect("the configured directory is read");
    let names: Vec<&str> = report
        .plugins()
        .iter()
        .map(|plugin| plugin.name())
        .collect();
    assert_eq!(names, ["std"]);
    assert_eq!(
        run_prompt(host, requiring("std")).await,
        Ok(Ok("browsed".to_owned())),
        "the library's Pack is installed, so a prompt that requires std runs"
    );
}

#[test]
fn only_a_plugin_directory_with_a_shipped_library_gives_the_host_its_pack() {
    for (library, pack) in [
        (STD_LIBRARY, "runvil/std"),
        (BASHKIT_LIBRARY, "runvil/bash"),
    ] {
        assert!(SHIPPED_PACKS.contains(&pack), "{pack}");
        let (dir, _library) = plugin_dir_of("holds-a-shipped-pack", library);
        let mut packs = PackRegistry::new();
        load_plugin_dir(&mut packs, dir.path()).expect("the directory is read");
        assert!(holds(&packs, pack), "{library} inserts {pack}");
        assert!(
            !holds(&PackRegistry::new(), pack),
            "{pack} is not among the built-ins"
        );
    }
}

/// A prompt that declares `std` and returns what `std/read` answers for
/// `/notes.md`, then what `std/write` answers.
const READS_NOTES: &str = "---\nname: reads\ndescription: reads a file\nrunvil: 0\n\
    uses:\n  - std\n---\n\n# Reads\n\n## Only\n\n```lua\n\
    local read = tools.call('std/read', { path = '/notes.md' })\n\
    local write = tools.call('std/write', { path = '/out.md', contents = 'x' })\n\
    return read .. '|' .. write\n```\n";

#[tokio::test]
async fn a_prompt_reads_a_file_of_its_run_through_the_std_library_plugin() {
    let (dir, _library) = plugin_dir("reads-notes");
    let (host, report) = host_packs(&Registry::new(), None, Some(dir.path()));
    let report = report.expect("the configured directory is read");
    assert!(report.failures().is_empty(), "{:?}", report.failures());

    let vfs = VfsRef::default();
    vfs.acquire(Origin::new("seed"))
        .expect("a fresh memory filesystem acquires")
        .write("/notes.md", b"hello\nworld\n")
        .expect("the file is seeded");

    let result = run_prompt_over(host, READS_NOTES.to_owned(), vfs).await;
    let Ok(Ok(text)) = result else {
        panic!("the run ends with its text: {result:?}");
    };
    // Each numbered line holds a `|`, and the stub's text holds none.
    let (read, write) = text.rsplit_once('|').expect("the prompt joins two answers");
    assert!(
        read.contains("     1|hello\n     2|world\n"),
        "Read answers the file's numbered lines: {read}"
    );
    assert!(
        read.contains("<untrusted_input_"),
        "file text reaches the script marked as untrusted: {read}"
    );
    assert_eq!(
        write,
        "The Write tool is not implemented in this build. The implementation is missing and \
         nothing in this session will change that. Calling Write again is safe, but it will \
         return this same result every time, whatever the arguments. Carry on without it.",
        "Write is still a stub, and its text is trusted, so it comes back as it is"
    );
}

#[tokio::test]
async fn without_a_plugin_directory_a_prompt_requiring_a_shipped_pack_is_refused() {
    for pack in ["std", "bash"] {
        let (host, report) = host_packs(&Registry::new(), None, None);
        assert!(report.is_none(), "no directory is read");

        let result = run_prompt(host, requiring(pack)).await;
        let Err(message) = result else {
            panic!("the Host refuses the prompt as it parses: {result:?}");
        };
        assert!(
            message.contains(&format!(
                "- {pack} is unavailable: runvil/{pack} is not installed"
            )),
            "the refusal names the Pack no Plugin inserted: {message}"
        );
    }
}

/// A prompt that declares `bash` and returns what `bash/shell` answers
/// for a command that reads `/notes.md` and then writes a file.
const RUNS_A_COMMAND: &str = "---\nname: shells\ndescription: runs a command\nrunvil: 0\n\
    uses:\n  - bash\n---\n\n# Shells\n\n## Only\n\n```lua\n\
    return tools.call('bash/shell', { command = 'cat /notes.md; echo x > /out.md' })\n```\n";

#[tokio::test]
async fn a_prompt_runs_a_command_over_its_runs_files_through_the_bashkit_library_plugin() {
    let (dir, _library) = plugin_dir_of("runs-a-command", BASHKIT_LIBRARY);
    let (host, report) = host_packs(&Registry::new(), None, Some(dir.path()));
    let report = report.expect("the configured directory is read");
    assert!(report.failures().is_empty(), "{:?}", report.failures());
    let names: Vec<&str> = report
        .plugins()
        .iter()
        .map(|plugin| plugin.name())
        .collect();
    assert_eq!(names, ["bashkit"]);

    let vfs = VfsRef::default();
    vfs.acquire(Origin::new("seed"))
        .expect("a fresh memory filesystem acquires")
        .write("/notes.md", b"hello\nworld\n")
        .expect("the file is seeded");

    let result = run_prompt_over(host, RUNS_A_COMMAND.to_owned(), vfs.clone()).await;
    let Ok(Ok(text)) = result else {
        panic!("the run ends with its text: {result:?}");
    };
    assert!(
        text.contains("<untrusted_input_"),
        "a command's output reaches the script marked as untrusted: {text}"
    );
    assert!(
        text.contains("Exit code: 1\n") && text.contains("```\nhello\nworld\n"),
        "the shell reads the file through the run's filesystem, and the write fails the \
         command: {text}"
    );
    assert!(
        text.contains("/out.md: the shell cannot change files in this build"),
        "the shell says why the write failed: {text}"
    );
    let written = vfs
        .acquire(Origin::new("check"))
        .expect("the filesystem acquires")
        .exists("/out.md")
        .expect("the filesystem answers");
    assert!(!written, "the shell changes no file of the run");
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
