//! A prompt calls the fixture library's tools through the real Harness:
//! the Pack the library inserted is declared, each run gets its own
//! instance, a call that takes several steps finishes, and a canceled run
//! cancels its pending call and deletes its instance. The fixture's
//! counters show what the library still holds.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use harness_plugin_loader::{PluginReport, insert_plugin_dir};
use runvil::effect::Round;
use runvil::harness::{InferenceBroker, MemoryRecorder, Packs, Timer};
use runvil::model::{
    Completion, CompletionOptions, Message, ModelBinding, ModelCatalog, ToolSchema,
    make_completion_error, phrase,
};
use runvil::vfs::{MemoryBackend, Op, Origin, Policy, Verdict, VfsPath, VfsRef};
use runvil::{BoxFuture, Error, ErrorCode, Harness, Host, RunSetup};
use runvil_plugin::PackRegistry;

use crate::support::{already_loaded, copy_fixture, counter, remove, scratch};

/// The name each Host finds its prompt by.
const PROMPT: &str = "prompt.md";

/// A prompt that declares the fixture's Pack and runs `lua`.
fn prompt(lua: &str) -> String {
    format!(
        "---\nname: fixture\ndescription: calls the fixture's tools\nrunvil: 0\n\
         uses:\n  - fixture/test\n---\n\n# Fixture\n\n## Only\n\n```lua\n{lua}\n```\n"
    )
}

/// Lists no model and refuses every round, since no prompt here asks one.
struct Offline;

impl InferenceBroker for Offline {
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

/// Sleeps on tokio's timer.
struct Clock;

impl Timer for Clock {
    fn sleep(&self, duration: Duration) -> BoxFuture<'static, ()> {
        Box::pin(tokio::time::sleep(duration))
    }
}

/// A Host over the library Plugins in `dir` that finds `source` by
/// [`PROMPT`], and the report of what `dir` loaded.
fn host_over(dir: &Path, source: String) -> (Arc<Host>, PluginReport) {
    let mut packs = PackRegistry::new();
    let report = insert_plugin_dir(&mut packs, dir).expect("the directory reads");
    assert!(report.failures().is_empty(), "{:?}", report.failures());
    let host = Arc::new(Host {
        packs: Packs::new(packs),
        prompts: Arc::new(move |name: &str| {
            if name == PROMPT {
                return Ok(source.clone());
            }
            let code = ErrorCode::NotFound;
            Err(Error::new(
                code,
                format!("no prompt named {name}"),
                code.prose(),
            ))
        }),
        timer: Arc::new(Clock),
    });
    (host, report)
}

/// A Harness for one run of the Host's prompt.
fn harness(host: &Arc<Host>) -> Harness {
    harness_over(host, VfsRef::default())
}

/// A Harness for one run of the Host's prompt over the files in `vfs`.
fn harness_over(host: &Arc<Host>, vfs: VfsRef) -> Harness {
    let prompt = host.parse(PROMPT).expect("the prompt parses");
    let setup = RunSetup {
        vfs,
        ..RunSetup::new(Arc::new(MemoryRecorder::new()), Arc::new(Offline))
    };
    Harness::new(Arc::clone(host), prompt, setup)
}

#[tokio::test]
async fn a_prompt_calls_a_one_step_tool_and_a_three_step_tool_of_the_library() {
    let dir = scratch("a_prompt_calls_the_library_tools");
    let file =
        std::path::absolute(copy_fixture(&dir, "plugin")).expect("the working directory is known");
    let lua = "local hello = tools.call('test/hello', { name = 'x' })\n\
               local steps = tools.call('test/steps', {})\n\
               return hello .. '|' .. steps";
    let (host, report) = host_over(&dir, prompt(lua));
    let library = already_loaded(&file).expect("the loader holds the library open");

    for run in 1..=2 {
        let report = harness(&host)
            .run()
            .await
            .expect("the recorder takes the run");
        assert_eq!(
            report.result,
            Ok(r#"hello {"name":"x"}|3 steps"#.to_owned()),
            "run {run}"
        );
        assert_eq!(
            (counter(&library, "packs"), counter(&library, "calls")),
            (0, 0),
            "run {run}: the run's Pack instance and its calls' state are gone when it ends"
        );
    }

    drop(report);
    assert_eq!(
        counter(&library, "live"),
        1,
        "the Host's Packs keep the Plugin loaded"
    );
    drop(host);
    assert_eq!(
        (counter(&library, "unloads"), counter(&library, "live")),
        (1, 0)
    );
    drop(library);
    remove(&dir);
}

#[tokio::test]
async fn a_prompt_reads_the_files_of_its_run_through_the_librarys_file_tools() {
    let dir = scratch("a_prompt_reads_the_files_of_its_run");
    let file =
        std::path::absolute(copy_fixture(&dir, "plugin")).expect("the working directory is known");
    let lua = "local function tool(name, path)\n\
               \x20 return tools.call('test/' .. name, { path = path })\n\
               end\n\
               return table.concat({\n\
               \x20 tool('stat', '/notes/a.md'), tool('stat', '/notes'),\n\
               \x20 tool('list', '/notes'), tool('read', 'notes/a.md'),\n\
               \x20 tool('read', '/absent'), tool('list', '/notes/a.md'),\n\
               \x20 tool('read', '/notes'),\n\
               }, '|')";
    let (host, report) = host_over(&dir, prompt(lua));
    let library = already_loaded(&file).expect("the loader holds the library open");

    let vfs = VfsRef::default();
    let seed = vfs
        .acquire(Origin::new("fixture seed"))
        .expect("a fresh memory filesystem acquires");
    seed.write("/notes/a.md", b"hello")
        .expect("a seeded file is written");
    seed.write("/notes/sub/b.md", b"")
        .expect("a seeded file is written");
    drop(seed);

    let report_of_run = harness_over(&host, vfs)
        .run()
        .await
        .expect("the recorder takes the run");
    assert_eq!(
        report_of_run.result,
        Ok([
            "file 5",
            "directory",
            "a.md,sub/",
            "hello",
            "failed (1): not found: /absent",
            "failed (2): not a directory: /notes/a.md",
            "failed (3): is a directory: /notes",
        ]
        .join("|"))
    );
    assert_eq!(
        (counter(&library, "packs"), counter(&library, "calls")),
        (0, 0)
    );

    drop((host, report));
    assert_eq!(counter(&library, "live"), 0);
    drop(library);
    remove(&dir);
}

/// Denies every read under `/secret`, and panics on every read under
/// `/broken`. A write passes, so a test can seed both.
struct Guard;

impl Policy for Guard {
    fn check(&self, op: Op, path: &VfsPath) -> Verdict {
        if !matches!(op, Op::Read | Op::List | Op::Stat) {
            return Verdict::Allow;
        }
        let path = path.as_str();
        if path.starts_with("/secret") {
            return Verdict::Deny("This run may not read under /secret.".to_owned());
        }
        assert!(!path.starts_with("/broken"), "the policy broke on {path}");
        Verdict::Allow
    }
}

#[tokio::test]
async fn a_read_the_policy_denies_or_panics_on_is_a_file_error_and_the_run_goes_on() {
    let dir = scratch("a_read_the_policy_denies_or_panics_on");
    let file =
        std::path::absolute(copy_fixture(&dir, "plugin")).expect("the working directory is known");
    let lua = "local function tool(name, path)\n\
               \x20 return tools.call('test/' .. name, { path = path })\n\
               end\n\
               return table.concat({\n\
               \x20 tool('read', '/secret/key'), tool('stat', '/secret/key'),\n\
               \x20 tool('list', '/secret'), tool('read', '/broken/a.md'),\n\
               \x20 tool('read', '/notes/a.md'),\n\
               }, '|')";
    let (host, report) = host_over(&dir, prompt(lua));
    let library = already_loaded(&file).expect("the loader holds the library open");

    let vfs = VfsRef::builder()
        .store("/", MemoryBackend::new())
        .policy(Guard)
        .build();
    let seed = vfs
        .acquire(Origin::new("fixture seed"))
        .expect("a fresh memory filesystem acquires");
    for path in ["/secret/key", "/broken/a.md", "/notes/a.md"] {
        seed.write(path, b"hello")
            .expect("a seeded file is written");
    }
    drop(seed);

    let report_of_run = harness_over(&host, vfs)
        .run()
        .await
        .expect("the recorder takes the run");
    let denied = "failed (4): This run may not read under /secret.";
    assert_eq!(
        report_of_run.result,
        Ok([
            denied,
            denied,
            denied,
            "failed (0): the file operation panicked",
            "hello",
        ]
        .join("|"))
    );
    assert_eq!(
        (counter(&library, "packs"), counter(&library, "calls")),
        (0, 0)
    );

    drop((host, report));
    assert_eq!(counter(&library, "live"), 0);
    drop(library);
    remove(&dir);
}

#[tokio::test]
async fn a_canceled_run_cancels_its_pending_call_and_deletes_its_pack_instance() {
    let dir = scratch("a_canceled_run_cancels_its_pending_call");
    let file =
        std::path::absolute(copy_fixture(&dir, "plugin")).expect("the working directory is known");
    let (host, report) = host_over(&dir, prompt("return tools.call('test/never', {})"));
    let library = already_loaded(&file).expect("the loader holds the library open");

    let harness = harness(&host);
    let control = harness.control();
    let mut run = std::pin::pin!(harness.run());
    let mut canceled = false;
    let report_of_run = loop {
        tokio::select! {
            biased;
            report = &mut run => break report.expect("the recorder takes the run"),
            () = tokio::task::yield_now() => {
                // The call holds state from its first step on, and it is
                // pending, so the run cannot end by itself.
                if !canceled && counter(&library, "calls") == 1 {
                    assert_eq!(counter(&library, "packs"), 1, "the run holds its instance");
                    control.cancel();
                    canceled = true;
                }
            }
        }
    };
    assert!(canceled, "the call was pending when the run was canceled");
    assert!(
        report_of_run.result.as_ref().is_err_and(Error::is_canceled),
        "the run ends canceled: {:?}",
        report_of_run.result
    );
    assert_eq!(
        (counter(&library, "calls"), counter(&library, "packs")),
        (0, 0),
        "the cancel freed the call's state, and the run's end deleted its instance"
    );

    drop((host, report));
    assert_eq!(counter(&library, "live"), 0);
    drop(library);
    remove(&dir);
}
