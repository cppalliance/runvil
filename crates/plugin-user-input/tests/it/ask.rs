//! The `user-input` Pack through the crate's `install`: the one Pack it
//! inserts, unavailable for any config but `null` or `{}`, what its
//! factory makes under the Pack id it receives, what the ask tool answers
//! from the run's broker, and the prelude a declaring prompt runs.

use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use plugin_user_input::{ASK, INPUT_BROKER, InputBroker, install};
use runvil_plugin::testing::TestCall;
use runvil_plugin::{
    Error, ErrorCode, HostServices, OutputTrust, Pack, PackFactory, PackId, PackInfo, PackRegistry,
    ToolId, make_tool_error,
};
use serde_json::{Value, json};

/// A broker whose operator always types the same text.
struct Scripted(&'static str);

#[async_trait::async_trait]
impl InputBroker for Scripted {
    async fn wait(&self) -> Result<String, Error> {
        Ok(self.0.to_owned())
    }
}

/// A broker whose every wait fails with a hidden cause behind the
/// prose.
struct Failing;

#[async_trait::async_trait]
impl InputBroker for Failing {
    async fn wait(&self) -> Result<String, Error> {
        Err(
            make_tool_error(ErrorCode::ToolBackend, "the operator's window closed")
                .with_cause(&std::io::Error::other("socket reset")),
        )
    }
}

/// Drives a future that never pends to its output.
fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
            return output;
        }
    }
}

/// A factory or the reason its Pack was inserted unavailable.
type Inserted = Result<Arc<dyn PackFactory>, Error>;

/// Each Pack `install` inserts into a fresh registry with `config`, beside
/// its Pack id and its factory or the reason it was inserted unavailable.
fn inserted(config: Value) -> Vec<(PackInfo, PackId, Inserted)> {
    let mut packs = PackRegistry::new();
    install(&mut packs, config, &HostServices::new()).expect("a fresh registry takes the Pack");
    packs
        .into_entries()
        .filter(|(info, _, _)| info.fqn != "runvil/subagents")
        .collect()
}

/// The one Pack `install` inserts with `config`, its Pack id, and the
/// reason it was inserted unavailable, if it was.
fn read(config: Value) -> Vec<(&'static str, String, Option<String>)> {
    inserted(config)
        .into_iter()
        .map(|(info, id, factory)| {
            let reason = factory.err().map(|error| error.to_string());
            (info.fqn, id.to_string(), reason)
        })
        .collect()
}

/// The `PackInfo` `install` inserts.
fn user_input() -> PackInfo {
    let mut inserted = inserted(Value::Null);
    let (info, _id, _factory) = inserted.pop().expect("install inserts one Pack");
    info
}

/// The Pack made under the Pack id `name` by the factory `install` inserts.
fn built(name: &str) -> Arc<dyn Pack> {
    let name = PackId::parse(name).expect("a one-segment Pack id parses");
    let (_info, _id, factory) = inserted(Value::Null)
        .pop()
        .expect("install inserts one Pack");
    let Ok(factory) = factory else {
        panic!("install inserts user-input usable with no configuration");
    };
    let Ok(pack) = factory.new_pack(&name, &HostServices::new()) else {
        panic!("the factory makes the Pack");
    };
    pack
}

/// Run services whose broker is `broker`.
fn with_broker(broker: impl InputBroker + 'static) -> HostServices {
    let mut services = HostServices::new();
    let broker: Arc<dyn InputBroker> = Arc::new(broker);
    services
        .provide(&INPUT_BROKER, broker)
        .expect("an empty map takes the broker");
    services
}

/// Asks once through the Pack built under `user-input`, with `services`
/// as the run's services.
fn ask(services: HostServices) -> Result<runvil_plugin::ToolOutput, Error> {
    let pack = built("user-input");
    let call = TestCall::new(pack.tools()[0].id.clone()).with_services(services);
    block_on(pack.call(call.context(), json!({})))
}

#[test]
fn the_pack_is_runvil_user_input_and_needs_the_input_broker() {
    let info = user_input();
    assert_eq!(info.fqn, "runvil/user-input");
    assert_eq!(info.needs, [INPUT_BROKER.id()]);
    assert_eq!(info.needs[0].to_string(), "runvil/input-broker");
    assert_eq!(ASK, "ask");
}

#[test]
fn install_adds_the_one_pack_usable_when_its_config_is_null_or_an_empty_object() {
    for config in [
        json!({ "user-input": {}, "web": { "x": 1 } }),
        json!({ "web": {} }),
        Value::Null,
    ] {
        assert_eq!(
            read(config.clone()),
            [("runvil/user-input", "user-input".to_owned(), None)],
            "{config}"
        );
    }
}

#[test]
fn install_adds_the_one_pack_unavailable_for_any_other_configuration() {
    for config in [json!(1), json!({ "greeting": "hi" })] {
        assert_eq!(
            read(json!({ "user-input": config })),
            [(
                "runvil/user-input",
                "user-input".to_owned(),
                Some("user-input takes no configuration".to_owned())
            )],
            "{config}"
        );
    }
}

#[test]
fn the_factory_names_the_ask_tool_under_the_pack_id_it_receives_and_marks_it_to_survive_a_stop() {
    let tools = built("operator").tools();
    assert_eq!(tools.len(), 1, "one tool: {tools:?}");
    let ask = &tools[0];
    assert_eq!(
        ask.id,
        ToolId::parse("operator/ask").expect("the id parses")
    );
    assert_eq!(
        ask.description,
        "Wait for the operator's next message and return its text."
    );
    assert_eq!(
        ask.parameters_schema,
        json!({ "type": "object", "properties": {} })
    );
    assert!(ask.survives_stop, "a stop leaves the question open");
    assert!(!ask.structured_output, "the answer is plain text");
}

#[test]
fn the_ask_tool_returns_the_brokers_text_trusted_and_byte_exact() {
    let typed = "  two lines\nwith\ttabs and trailing space  ";
    let output = ask(with_broker(Scripted(typed))).expect("the broker answers");
    assert_eq!(output.text(), typed, "no trimming or re-encoding");
    assert_eq!(output.trust(), OutputTrust::Trusted);
}

#[test]
fn a_broker_failure_is_the_ask_calls_error_unchanged() {
    let error = ask(with_broker(Failing)).expect_err("a failed wait fails the call");
    assert_eq!(error.prose, "the operator's window closed");
    assert_eq!(error.code, ErrorCode::ToolBackend);
    assert_eq!(
        error.message, "the operator's window closed: socket reset",
        "the cause stays in the message, out of the prose"
    );
}

#[test]
fn a_call_without_a_broker_fails_saying_no_operator_is_connected() {
    let error = ask(HostServices::new()).expect_err("no broker, no answer");
    assert_eq!(error.to_string(), "no operator is connected to this run");
}

#[test]
fn the_prelude_reads_the_pack_id_and_asks_through_it() {
    let prelude = user_input().prelude.expect("the Pack has a prelude");
    assert!(prelude.starts_with("local pack = ...\n"), "{prelude}");
    assert!(
        prelude.contains("tools.call(pack .. \"/ask\")"),
        "{prelude}"
    );
    assert!(
        prelude.contains("error(\"input.ask takes no arguments\", 2)"),
        "{prelude}"
    );
    assert!(
        !prelude.contains("connected"),
        "input.connected is gone: {prelude}"
    );
}
