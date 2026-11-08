//! The run's optional input broker at preparation, supplied among its own
//! services under the fixture's `BROKER`: each Pack the run makes can
//! take it at `new_pack`, and finds none when the Host has nobody to ask;
//! and the fixture ask Pack's `input.ask()` reaches it, and is refused on
//! a run without one. The ask tool, read by its id, runs beside the
//! Pack's `input` prelude global.

use super::*;

use crate::asker::{ASKER, AskBroker, BROKER, broker_services};

/// A prompt declaring the probe Pack and returning what its one call
/// saw.
const PROBES: &str = "---\nname: probes\ndescription: d\nrunvil: 0\n\
    uses:\n  - tests/probe\n---\n\n# Title\n\n## Only\n\n\
    ```lua\nreturn tools.call('probe/saw')\n```\n";

/// A broker whose operator always types the same text.
struct Scripted(&'static str);

#[async_trait::async_trait]
impl AskBroker for Scripted {
    async fn wait(&self) -> Result<String, Error> {
        Ok(self.0.to_owned())
    }
}

/// A broker whose every wait fails, with a hidden cause behind the
/// prose.
struct Failing;

#[async_trait::async_trait]
impl AskBroker for Failing {
    async fn wait(&self) -> Result<String, Error> {
        Err(
            make_tool_error(ErrorCode::ToolBackend, "the operator's window closed")
                .with_cause(&std::io::Error::other("socket reset")),
        )
    }
}

/// The fixture Pack `probe`: one tool, `probe/saw`, answering whether
/// its instance found a broker among the run's services. Its factory
/// refuses no run, so a run without a broker can use it.
const PROBE: (PackInfo, Make) = (PackInfo::new("tests/probe"), new_probe);

struct Probe {
    tools: Vec<ToolInfo>,
    saw: bool,
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "the PackFactory signature fixes the return type"
)]
fn new_probe(name: &PackId, run: &HostServices) -> Result<Arc<dyn Pack>, Error> {
    let saw = ToolInfo::new(
        ToolId::parse(&format!("{name}/saw")).unwrap(),
        "Answer whether the instance found a broker.",
        json!({ "type": "object", "properties": {} }),
    );
    Ok(Arc::new(Probe {
        tools: vec![saw],
        saw: run.get(&BROKER).is_some(),
    }))
}

impl Pack for Probe {
    fn tools(&self) -> Vec<ToolInfo> {
        self.tools.clone()
    }

    fn call<'a>(
        &'a self,
        _cx: ToolContext<'a>,
        _args: Value,
    ) -> PackFuture<'a, Result<ToolOutput, Error>> {
        let saw = self.saw;
        Box::pin(async move { Ok(ToolOutput::trusted(saw.to_string())) })
    }
}

/// Run services holding `input` under the fixture's `BROKER`, or none
/// when the Host has nobody to ask.
fn with_input(input: Option<Arc<dyn AskBroker>>) -> HostServices {
    input.map_or_else(HostServices::new, broker_services)
}

/// Runs `source` on a Host over `packs` with `setup` and returns the
/// run's result.
async fn drive_prompt(source: &str, packs: Packs, setup: RunSetup) -> Result<String, Error> {
    prepare_and_drive(source, packs, setup).await.1
}

/// What the probe's one call answers, on a run with `input` as its
/// broker.
async fn probe_sees(input: Option<Arc<dyn AskBroker>>) -> String {
    let recorder = recorder();
    let setup = RunSetup {
        services: with_input(input),
        ..setup_over(&recorder)
    };
    completed(drive_prompt(PROBES, building(PROBE), setup).await)
}

#[tokio::test]
async fn a_packs_factory_finds_the_runs_broker() {
    assert_eq!(probe_sees(Some(Arc::new(Scripted("unused")))).await, "true");
}

#[tokio::test]
async fn a_packs_factory_finds_no_broker_when_the_host_has_none() {
    assert_eq!(probe_sees(None).await, "false");
}

/// The frontmatter line declaring the fixture ask Pack by its FQN, whose
/// Pack id is `user-input`, as `chat.md` declares the real one.
const REQUIRED: &str = "  - tests/user-input\n";

/// Asks once and returns the answer.
const ASKS_ONCE: &str = "local text = input.ask()\nreturn text";

/// A one-section prompt that declares the user-input Pack with
/// `declaration` (none when empty) and runs `lua`.
fn user_input_prompt(declaration: &str, lua: &str) -> String {
    let packs = if declaration.is_empty() {
        String::new()
    } else {
        format!("uses:\n{declaration}")
    };
    format!(
        "---\nname: asks-input\ndescription: d\nrunvil: 0\n{packs}---\n\n\
         # Title\n\n## Only\n\n```lua\n{lua}\n```\n"
    )
}

/// Runs `source` over `recorder` on a Host with the fixture ask Pack
/// built as `user-input`, with `input` as the run's broker, and returns
/// the run's result.
async fn drive_asking(
    source: &str,
    recorder: &Arc<MemoryRecorder>,
    input: Option<Arc<dyn AskBroker>>,
) -> Result<String, Error> {
    let setup = RunSetup {
        services: with_input(input),
        ..setup_over(recorder)
    };
    drive_prompt(source, building(ASKER), setup).await
}

#[tokio::test]
async fn a_required_user_input_declaration_on_a_host_without_a_broker_is_refused() {
    let recorder = recorder();
    let notice =
        refusal(drive_asking(&user_input_prompt(REQUIRED, ASKS_ONCE), &recorder, None).await);
    assert!(
        notice.contains(
            "- user-input is unavailable: user-input needs tests/input-broker, and this run \
             provides none"
        ),
        "the notice names the Pack and the missing service: {notice}"
    );
    assert!(
        !notice.contains("missing required Pack: user-input"),
        "the notice does not call the registered Pack missing: {notice}"
    );
}

#[tokio::test]
async fn a_failed_ask_raises_at_the_call_site_where_pcall_catches_it() {
    let recorder = recorder();
    let catches = "local ok, err = pcall(input.ask)\n\
        return tostring(ok) .. '|' .. err.kind .. '|' .. tostring(err)";
    let result = drive_asking(
        &user_input_prompt(REQUIRED, catches),
        &recorder,
        Some(Arc::new(Failing)),
    )
    .await;
    let text = completed(result);
    assert!(text.starts_with("false|tool|"), "a tool failure: {text}");
    assert!(
        text.contains("the operator's window closed"),
        "the broker's message reaches the script: {text}"
    );
    assert!(
        !text.contains("socket reset"),
        "the broker's hidden cause never reaches the script: {text}"
    );
}

#[tokio::test]
async fn an_uncaught_failed_ask_ends_the_run_as_a_tool_failure() {
    let recorder = recorder();
    let error = drive_asking(
        &user_input_prompt(REQUIRED, "return (input.ask())"),
        &recorder,
        Some(Arc::new(Failing)),
    )
    .await
    .expect_err("an uncaught failed ask fails the run");
    assert_eq!(
        error.code,
        ErrorCode::ToolBackend,
        "the broker's tool failure"
    );
    let message = error.located_message();
    assert!(
        message.contains("the operator's window closed"),
        "the broker's message reaches the run's failure: {message}"
    );
}

#[tokio::test]
async fn input_ask_with_an_argument_raises() {
    let recorder = recorder();
    let passes_one = "local ok, err = pcall(input.ask, 1)\n\
        return tostring(ok) .. '|' .. tostring(err)";
    let result = drive_asking(
        &user_input_prompt(REQUIRED, passes_one),
        &recorder,
        Some(Arc::new(Scripted("unused"))),
    )
    .await;
    let text = completed(result);
    assert!(text.starts_with("false|"), "the call raised: {text}");
    assert!(
        text.contains("input.ask takes no arguments"),
        "the error says why: {text}"
    );
}

#[tokio::test]
async fn the_ask_tool_read_by_id_runs_beside_the_untouched_prelude_and_engine_globals() {
    let recorder = recorder();
    let result = drive_asking(
        &user_input_prompt(
            REQUIRED,
            "local ask = tools.get('user-input/ask')\n\
             return tools.call(ask) .. '|' .. ask.id .. '|' .. type(input.ask) .. '|' \
             .. type(store.read) .. '|' .. type(tools.call) .. '|' .. type(user_input_ask)",
        ),
        &recorder,
        Some(Arc::new(Scripted("hello"))),
    )
    .await;
    assert_eq!(
        completed(result),
        "hello|user-input/ask|function|function|function|nil",
        "the tool object is the ask tool, no tool is a global, and input, store, and \
         tools are the Pack's and the Engine's"
    );
}

#[tokio::test]
async fn a_prompt_that_does_not_declare_user_input_has_no_input_global() {
    let recorder = recorder();
    let reaches = "local ok, err = pcall(function() return input.ask() end)\n\
        return tostring(ok) .. '|' .. tostring(err)";
    let result = drive_asking(
        &user_input_prompt("", reaches),
        &recorder,
        Some(Arc::new(Scripted("unused"))),
    )
    .await;
    let text = completed(result);
    assert!(text.starts_with("false|"), "the call raised: {text}");
    assert!(
        text.contains("attempt to index a nil value (global 'input')"),
        "Lua's own error names the missing global: {text}"
    );
}
