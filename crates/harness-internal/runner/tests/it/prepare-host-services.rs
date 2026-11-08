//! The run's services at preparation: a declared Pack whose factory takes
//! a service from the run serves a call with the provider the run's map
//! holds, and a run whose services lack it is refused at its first step
//! naming the service.

use super::*;

use runvil_plugin::ServiceKey;

/// The test-only service the fixture's factory takes.
const GREETING: ServiceKey<str> = ServiceKey::new("tests/greeting");

/// The fixture Pack `greeter`: one tool, `greeter/greet`, answering
/// with the greeting its instance took from the run.
const GREETER: (PackInfo, Make) = (PackInfo::new("tests/greeter"), new_greeter);

/// A prompt declaring the greeter Pack and returning its greeting.
const GREETS: &str = "---\nname: greets\ndescription: d\nrunvil: 0\n\
    uses:\n  - tests/greeter\n---\n\n# Title\n\n## Only\n\n\
    ```lua\nreturn tools.call('greeter/greet')\n```\n";

struct Greeter {
    tools: Vec<ToolInfo>,
    greeting: Arc<str>,
}

fn new_greeter(name: &PackId, run: &HostServices) -> Result<Arc<dyn Pack>, Error> {
    let greeting = run.get(&GREETING).ok_or_else(|| {
        make_tool_error(
            ErrorCode::ToolOther,
            format!("{name} needs tests/greeting, and this run provides none"),
        )
    })?;
    let greet = ToolInfo::new(
        ToolId::parse(&format!("{name}/greet")).unwrap(),
        "Answer with the run's greeting.",
        json!({ "type": "object", "properties": {} }),
    );
    Ok(Arc::new(Greeter {
        tools: vec![greet],
        greeting,
    }))
}

impl Pack for Greeter {
    fn tools(&self) -> Vec<ToolInfo> {
        self.tools.clone()
    }

    fn call<'a>(
        &'a self,
        _cx: ToolContext<'a>,
        _args: Value,
    ) -> PackFuture<'a, Result<ToolOutput, Error>> {
        Box::pin(async move { Ok(ToolOutput::trusted(self.greeting.to_string())) })
    }
}

/// Runs [`GREETS`] with `run` as the run's services to its end,
/// returning the recorder and the run's id beside its result.
async fn drive_greeter(run: HostServices) -> (Arc<MemoryRecorder>, RunId, Result<String, Error>) {
    let recorder = recorder();
    let setup = RunSetup {
        services: run,
        ..setup_over(&recorder)
    };
    let (run_id, result) = prepare_and_drive(GREETS, building(GREETER), setup).await;
    (recorder, run_id, result)
}

#[tokio::test]
async fn a_pack_serves_a_call_with_the_service_the_runs_map_provides() {
    let mut run = HostServices::new();
    run.provide(&GREETING, Arc::from("hello")).unwrap();
    let (_recorder, _run_id, result) = drive_greeter(run).await;
    assert_eq!(
        completed(result),
        "hello",
        "the instance took the run's provider"
    );
}

#[tokio::test]
async fn a_pack_whose_service_the_run_lacks_is_refused_naming_the_service() {
    let (recorder, run_id, result) = drive_greeter(HostServices::new()).await;
    let notice = refusal(result);
    assert!(
        notice.contains(
            "- greeter is unavailable: greeter needs tests/greeting, and this run provides none"
        ),
        "the notice names the Pack and the missing service: {notice}"
    );
    let effects = recorder
        .records(run_id)
        .into_iter()
        .filter(|record| record.kind == RecordKind::Effect)
        .count();
    assert_eq!(effects, 0, "the Pack was never called");
}
