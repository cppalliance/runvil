//! The `user-input` Pack through the crate's `install`: the one Pack it
//! adds with its config, what `construct` builds under the Pack id it
//! receives, what the ask tool answers from the run's broker, and the
//! prelude a declaring prompt runs.

use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use plugin_user_input::{ASK, INPUT_BROKER, InputBroker, install};
use runvil_plugin::testing::TestCall;
use runvil_plugin::{
    HostServices, OutputTrust, Pack, PackId, PackInfo, PackRegistry, ToolError, ToolErrorKind,
    ToolId,
};
use serde_json::{Value, json};

/// A broker whose operator always types the same text.
struct Scripted(&'static str);

#[async_trait::async_trait]
impl InputBroker for Scripted {
    async fn wait(&self) -> Result<String, ToolError> {
        Ok(self.0.to_owned())
    }
}

/// A broker whose every wait fails with a hidden cause behind the
/// message.
struct Failing;

#[async_trait::async_trait]
impl InputBroker for Failing {
    async fn wait(&self) -> Result<String, ToolError> {
        Err(
            ToolError::new(ToolErrorKind::Backend, "the operator's window closed")
                .with_source(std::io::Error::other("socket reset")),
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

/// Each Pack `install` adds to a fresh registry with `config`, beside
/// the Pack id and config the registry holds it with.
fn added(config: Value) -> Vec<(PackInfo, PackId, Value)> {
    let mut packs = PackRegistry::new();
    install(&mut packs, config).expect("a fresh registry takes the Pack");
    packs
        .into_entries()
        .filter(|(info, _, _)| info.fqn != "runvil/subagents")
        .collect()
}

/// The `PackInfo` `install` adds.
fn user_input() -> PackInfo {
    let mut added = added(Value::Null);
    let (info, _id, _config) = added.pop().expect("install adds one Pack");
    info
}

/// The Pack built under the Pack id `name` with `config`.
fn construct(name: &str, config: Value) -> Result<Arc<dyn Pack>, ToolError> {
    let name = PackId::parse(name).expect("a one-segment Pack id parses");
    (user_input().construct)(&name, config, &HostServices::new())
}

fn built(name: &str) -> Arc<dyn Pack> {
    let Ok(pack) = construct(name, Value::Null) else {
        panic!("construct succeeds with no configuration");
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
fn ask(services: HostServices) -> Result<runvil_plugin::ToolOutput, ToolError> {
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
fn install_adds_the_one_pack_with_the_config_under_its_pack_id() {
    for (config, expected) in [
        (json!({ "user-input": {}, "web": { "x": 1 } }), json!({})),
        (json!({ "web": {} }), Value::Null),
        (Value::Null, Value::Null),
    ] {
        let read: Vec<(&str, String, Value)> = added(config.clone())
            .into_iter()
            .map(|(info, id, config)| (info.fqn, id.to_string(), config))
            .collect();
        assert_eq!(
            read,
            [("runvil/user-input", "user-input".to_owned(), expected)],
            "{config}"
        );
    }
}

#[test]
fn construct_names_the_ask_tool_under_the_pack_id_it_receives_and_marks_it_to_survive_a_stop() {
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
fn construct_accepts_no_configuration_or_an_empty_object_and_refuses_anything_else() {
    assert!(construct("user-input", json!({})).is_ok());
    let Err(error) = construct("user-input", json!({ "greeting": "hi" })) else {
        panic!("a configuration with settings is refused");
    };
    assert_eq!(error.to_string(), "user-input takes no configuration");
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
    assert_eq!(error.to_string(), "the operator's window closed");
    assert_eq!(error.kind(), ToolErrorKind::Backend);
    let cause = std::error::Error::source(&error).map(ToString::to_string);
    assert_eq!(cause.as_deref(), Some("socket reset"));
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
