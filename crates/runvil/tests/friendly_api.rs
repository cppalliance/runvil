//! The public front door: typed tools, owned runs, and explicit failure boundaries.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use runvil::harness::{MemoryRecorder, Timer};
use runvil::plugin::{PackInfo, PackRegistry};
use runvil::{BoxFuture, Error, ErrorCode, Host, OutputTrust};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

fn source(uses: &str, lua: &str) -> String {
    format!(
        "---\nname: demo\ndescription: Demonstrates the API\nrunvil: 0\n{uses}---\n\n# Demo\n\n## Only\n\n```lua\n{lua}\n```\n"
    )
}

fn host(source: String) -> Host {
    Host::new(".").with_prompt_finder(move |_: &str| Ok(source.clone()))
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Amount {
    /// The number to add.
    value: usize,
    note: Option<String>,
}

struct State {
    value: AtomicUsize,
    dropped: Arc<AtomicUsize>,
}

impl Drop for State {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn typed_handlers_get_fresh_state_per_run_and_drop_it_at_the_end() -> Result<(), Error> {
    let built = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicUsize::new(0));
    let mut packs = PackRegistry::new();
    packs.define(PackInfo::new("tests/counter"), {
        let built = built.clone();
        let dropped = dropped.clone();
        move |pack| {
            built.fetch_add(1, Ordering::SeqCst);
            let state = Arc::new(State {
                value: AtomicUsize::new(0),
                dropped: dropped.clone(),
            });
            pack.tool(
                "add",
                "Adds to this run's count",
                OutputTrust::Trusted,
                move |args: Amount| {
                    let state = state.clone();
                    async move {
                        assert!(args.note.is_none());
                        Ok(state.value.fetch_add(args.value, Ordering::SeqCst) + args.value)
                    }
                },
            )
        }
    })?;
    let app = host(source(
        "uses:\n  - tests/counter\n",
        "return tools.call('counter/add', {value=2}) + tools.call('counter/add', {value=3})",
    ))
    .with_packs(packs);
    app.parse("demo.md")?;
    assert_eq!(built.load(Ordering::SeqCst), 0);
    for runs in 1..=2 {
        assert_eq!(app.run("demo.md").await?.result?, "7");
        assert_eq!(built.load(Ordering::SeqCst), runs);
        assert_eq!(dropped.load(Ordering::SeqCst), runs);
    }
    Ok(())
}

#[tokio::test]
async fn invalid_typed_arguments_do_not_call_the_handler() -> Result<(), Error> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut packs = PackRegistry::new();
    packs.define(PackInfo::new("tests/counter"), {
        let calls = calls.clone();
        move |pack| {
            let calls = calls.clone();
            pack.tool("add", "Adds", OutputTrust::Trusted, move |args: Amount| {
                calls.fetch_add(1, Ordering::SeqCst);
                async move { Ok(args.value) }
            })
        }
    })?;
    let app = host(source("uses:\n  - tests/counter\n", "local ok, err = pcall(tools.call, 'counter/add', {value='wrong'})\nreturn ok and 'bad' or err.kind")).with_packs(packs);
    assert_eq!(app.run("demo.md").await?.result?, "tool");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn context_handlers_borrow_the_calls_filesystem() -> Result<(), Error> {
    let mut packs = PackRegistry::new();
    packs.define(PackInfo::new("tests/files"), |pack| {
        pack.tool_with_context(
            "write",
            "Writes a file",
            OutputTrust::Trusted,
            |cx, args: Amount| {
                Box::pin(async move {
                    tokio::task::yield_now().await;
                    cx.access()
                        .write("/reply.txt", args.value.to_string().as_bytes())
                        .map_err(|error| {
                            runvil::make_tool_error(ErrorCode::ToolOther, error.to_string())
                        })?;
                    Ok("written")
                })
            },
        )
    })?;
    let app = host(source(
        "uses:\n  - tests/files\n",
        "tools.call('files/write', {value=42})\nreturn store.read('reply.txt')",
    ))
    .with_packs(packs);
    assert_eq!(app.run("demo.md").await?.result?, "42");
    Ok(())
}

#[tokio::test]
async fn a_run_owns_its_configuration_and_inherits_the_hosts_recorder() -> Result<(), Error> {
    let history = Arc::new(MemoryRecorder::new());
    let pending = {
        let app = host(source("", "return 'done'")).with_recorder(history.clone());
        app.run("demo.md")
    };
    let report = pending.await?;
    assert_eq!(report.result, Ok("done".to_owned()));
    assert_eq!(history.result(report.run_id), Some(report.result));
    Ok(())
}

#[tokio::test]
async fn each_default_run_gets_a_fresh_filesystem() -> Result<(), Error> {
    let app = host(source(
        "",
        "local ok = pcall(store.read, 'old.txt')\nstore.write('old.txt', 'here')\nreturn ok and 'shared' or 'fresh'",
    ));
    for _ in 0..2 {
        assert_eq!(app.run("demo.md").await?.result?, "fresh");
    }
    Ok(())
}

#[tokio::test]
async fn prepared_runs_keep_control_and_record_cancellation() -> Result<(), Error> {
    let history = Arc::new(MemoryRecorder::new());
    let run = host(source("", "return 'done'"))
        .run("demo.md")
        .recorder(history.clone())
        .prepare()?;
    run.control().cancel();
    let report = run.run().await?;
    assert_eq!(
        report.result.as_ref().unwrap_err().code,
        ErrorCode::Canceled
    );
    assert_eq!(history.result(report.run_id), Some(report.result));
    Ok(())
}

#[tokio::test]
async fn finder_errors_and_execution_errors_remain_distinct() -> Result<(), Error> {
    let missing = Host::new(".")
        .with_prompt_finder(|_: &str| Err(Error::new(ErrorCode::NotFound, "missing", "missing")));
    assert_eq!(
        missing.run("missing.md").await.unwrap_err().code,
        ErrorCode::NotFound
    );
    let report = host(source("", "error('failed in Lua')"))
        .run("demo.md")
        .await?;
    assert_eq!(report.result.unwrap_err().code, ErrorCode::LuaRuntime);
    Ok(())
}

struct Clock(Arc<AtomicUsize>);

impl Timer for Clock {
    fn sleep(&self, _: Duration) -> BoxFuture<'static, ()> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {})
    }
}

#[tokio::test]
async fn raw_tools_remain_available_without_typed_schemas() -> Result<(), Error> {
    let mut packs = PackRegistry::new();
    packs.define(PackInfo::new("tests/raw"), |pack| {
        pack.raw_tool(
            "echo",
            "Echoes JSON",
            json!({"type":"object"}),
            |args| async move {
                Ok(runvil::ToolOutput::trusted(
                    args["text"].as_str().unwrap_or_default(),
                ))
            },
        )
    })?;
    let calls = Arc::new(AtomicUsize::new(0));
    let app = host(source(
        "uses:\n  - tests/raw\n",
        "return tools.call('raw/echo', {text='plain'})",
    ))
    .with_packs(packs)
    .with_timer(Clock(calls));
    assert_eq!(app.run("demo.md").await?.result?, "plain");
    Ok(())
}

#[tokio::test]
async fn interactive_runs_accept_input_through_the_prepared_control() -> Result<(), Error> {
    let run = host(source("uses:\n  - user\n", "return tasks.receive()"))
        .run("demo.md")
        .with_user()
        .prepare()?;
    assert!(run.control().send("hello".to_owned()));
    assert_eq!(run.run().await?.result?, "hello");
    Ok(())
}

#[tokio::test]
async fn input_and_output_flow_through_the_configured_filesystem() -> Result<(), Error> {
    let source = source(
        "input: {path: in.txt, description: Input}\noutput: {path: out.txt, description: Output}\n",
        "store.write('out.txt', string.upper(store.read('in.txt')))",
    );
    let vfs = runvil::vfs::VfsRef::default();
    let report = host(source)
        .run("demo.md")
        .vfs(vfs.clone())
        .input("hello")
        .await?;
    report.result?;
    assert_eq!(report.output?, "HELLO");
    assert_eq!(
        vfs.acquire_store(runvil::vfs::Origin::new("check"))
            .unwrap()
            .read_string("out.txt")
            .unwrap(),
        "HELLO"
    );
    Ok(())
}

struct BorrowingTool {
    prefix: String,
    calls: AtomicUsize,
}

impl runvil::plugin::Tool for BorrowingTool {
    const NAME: &'static str = "next";
    const DESCRIPTION: &'static str = "Uses this run's borrowed state";
    const TRUST: OutputTrust = OutputTrust::Trusted;
    type Args = Amount;
    type Output = String;

    async fn call(
        &self,
        _cx: runvil::plugin::ToolContext<'_>,
        args: Amount,
    ) -> Result<String, Error> {
        let prefix = &self.prefix;
        tokio::task::yield_now().await;
        let count = self.calls.fetch_add(args.value, Ordering::SeqCst) + args.value;
        Ok(format!("{prefix}:{count}"))
    }
}

#[tokio::test]
async fn stateful_tools_borrow_their_own_instance_across_awaits() -> Result<(), Error> {
    let mut packs = PackRegistry::new();
    packs.define(PackInfo::new("tests/borrowed"), |pack| {
        pack.add(BorrowingTool {
            prefix: "run".to_owned(),
            calls: AtomicUsize::new(0),
        })
    })?;
    let app = host(source("uses:\n  - tests/borrowed\n", "return tools.call('borrowed/next', {value=1}) .. '|' .. tools.call('borrowed/next', {value=1})")).with_packs(packs);
    for _ in 0..2 {
        assert_eq!(app.run("demo.md").await?.result?, "run:1|run:2");
    }
    Ok(())
}
