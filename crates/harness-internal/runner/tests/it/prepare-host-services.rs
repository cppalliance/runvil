//! The run's services at preparation: a declared Pack that needs a
//! service serves a call with the provider the run's map holds, and a run
//! whose services lack it is refused naming the service.

use super::*;

use runvil_plugin::{ServiceId, ServiceKey};

/// The test-only service the fixture needs.
const GREETING: ServiceKey<str> = ServiceKey::new("tests/greeting");

const NEEDS: &[ServiceId] = &[GREETING.id()];

/// The fixture Pack `greeter`: one tool, `greeter/greet`, answering
/// with the run's greeting.
const GREETER: PackInfo = PackInfo::new("tests/greeter", construct_greeter).needs(NEEDS);

/// A prompt declaring the greeter Pack and returning its greeting.
const GREETS: &str = "---\nname: greets\ndescription: d\nrunvil: 0\n\
    uses:\n  - greeter\n---\n\n# Title\n\n## Only\n\n\
    ```lua\nreturn tools.call('greeter/greet')\n```\n";

struct Greeter {
    tools: Vec<ToolInfo>,
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "the PackInfo construct signature fixes the return type"
)]
fn construct_greeter(
    name: &PackId,
    _config: Value,
    _services: &HostServices,
) -> Result<Arc<dyn Pack>, ToolError> {
    let greet = ToolInfo::new(
        ToolId::parse(&format!("{name}/greet")).unwrap(),
        "Answer with the run's greeting.",
        json!({ "type": "object", "properties": {} }),
    );
    Ok(Arc::new(Greeter { tools: vec![greet] }))
}

impl Pack for Greeter {
    fn tools(&self) -> Vec<ToolInfo> {
        self.tools.clone()
    }

    fn call<'a>(
        &'a self,
        cx: ToolContext<'a>,
        _args: Value,
    ) -> PackFuture<'a, Result<ToolOutput, ToolError>> {
        Box::pin(async move {
            let greeting = cx
                .service(&GREETING)
                .ok_or_else(|| ToolError::message("no greeting in this run"))?;
            Ok(ToolOutput::trusted(greeting.to_string()))
        })
    }
}

/// Prepares [`GREETS`] with `run` as the run's services.
async fn prepare_greeter(
    run: HostServices,
) -> (Arc<MemoryRecorder>, Result<Prepared, PrepareError>) {
    let recorder = recorder();
    let mut services = services(&recorder, installing(GREETER));
    services.services = run;
    let prepared = prepare(GREETS, "", services).await;
    (recorder, prepared)
}

#[tokio::test]
async fn a_pack_serves_a_call_with_the_service_the_runs_map_provides() {
    let mut run = HostServices::new();
    run.provide(&GREETING, Arc::from("hello")).unwrap();
    let (recorder, prepared) = prepare_greeter(run).await;
    let prepared = prepared.expect("the run's service meets the need");
    let outcome = drive_run(
        prepared.run,
        prepared.performers,
        recorder.clone(),
        prepared.run_id,
        CancelHandle::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        completed(outcome),
        "hello",
        "the call read the run's provider"
    );
}

#[tokio::test]
async fn a_pack_whose_service_the_run_lacks_is_refused_naming_the_service() {
    let (recorder, prepared) = prepare_greeter(HostServices::new()).await;
    let Err(PrepareError::Refused { run_id, error }) = prepared else {
        panic!("a required Pack without its service refuses the run");
    };
    assert!(
        error
            .to_string()
            .contains("- greeter needs tests/greeting, and the environment provides none"),
        "the notice names the Pack and the missing service: {error}"
    );
    let effects = recorder
        .records(run_id)
        .into_iter()
        .filter(|record| record.kind == RecordKind::Effect)
        .count();
    assert_eq!(effects, 0, "the Pack was never called");
}
