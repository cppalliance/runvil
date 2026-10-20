//! A fixture Plugin crate's whole Pack side: a `GREETER` constant, a
//! `construct` that reads a Host-wide service and names its tool under the
//! Pack id it receives, and an `impl Pack` whose `call` reads a per-run
//! service through the context it is lent.

use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use runvil_plugin::testing::TestCall;
use runvil_plugin::{
    HostServices, OutputTrust, Pack, PackFuture, PackId, PackInfo, ServiceId, ServiceKey,
    ToolContext, ToolError, ToolErrorKind, ToolId, ToolInfo, ToolOutput,
};
use serde_json::{Value, json};

/// The Host-wide service `construct` reads.
const GREETING: ServiceKey<str> = ServiceKey::new("acme/greeting");

/// The per-run service a call reads.
const VISITOR: ServiceKey<str> = ServiceKey::new("acme/visitor");

const NEEDS: &[ServiceId] = &[VISITOR.id()];

const GREETER: PackInfo = PackInfo::new("acme/greeter", construct)
    .prelude("local pack = ...\n")
    .needs(NEEDS);

/// The one object every run shares.
struct Greeter {
    greeting: Arc<str>,
    tools: Vec<ToolInfo>,
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "the PackInfo construct signature fixes the argument types"
)]
fn construct(
    name: &PackId,
    config: Value,
    services: &HostServices,
) -> Result<Arc<dyn Pack>, ToolError> {
    if !config.is_null() {
        return Err(ToolError::message("greeter takes no configuration"));
    }
    let greeting = services.get(&GREETING).ok_or_else(|| {
        ToolError::message("greeter needs acme/greeting, and this Host provides none")
    })?;
    let id = ToolId::parse(&format!("{name}/greet")).map_err(|e| {
        ToolError::new(ToolErrorKind::Backend, "greeter could not name its tool").with_source(e)
    })?;
    let tools = vec![ToolInfo::new(
        id,
        "Greets this run's visitor.",
        json!({"type": "object", "properties": {"mark": {"type": "string"}}}),
    )];
    Ok(Arc::new(Greeter { greeting, tools }))
}

impl Pack for Greeter {
    fn tools(&self) -> Vec<ToolInfo> {
        self.tools.clone()
    }

    fn call<'a>(
        &'a self,
        cx: ToolContext<'a>,
        args: Value,
    ) -> PackFuture<'a, Result<ToolOutput, ToolError>> {
        Box::pin(async move {
            match cx.tool().name() {
                "greet" => {
                    let visitor = cx
                        .service(&VISITOR)
                        .ok_or_else(|| ToolError::message("no visitor is in this run"))?;
                    let mark = args.get("mark").and_then(Value::as_str).unwrap_or(".");
                    Ok(ToolOutput::untrusted(format!(
                        "{}, {visitor}{mark}",
                        self.greeting
                    )))
                }
                other => Err(ToolError::message(format!(
                    "greeter has no tool named {other}"
                ))),
            }
        })
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

fn host_wide() -> HostServices {
    let mut services = HostServices::new();
    services
        .provide(&GREETING, Arc::from("Hello"))
        .expect("a valid, new id is accepted");
    services
}

fn built_as(name: &str) -> Arc<dyn Pack> {
    let name = PackId::parse(name).expect("a one-segment Pack id parses");
    let Ok(pack) = (GREETER.construct)(&name, Value::Null, &host_wide()) else {
        panic!("construct succeeds when its Host-wide service is provided");
    };
    pack
}

#[test]
fn construct_names_the_tools_under_the_pack_id_it_receives() {
    let tools = built_as("hi").tools();
    let ids: Vec<String> = tools.iter().map(|tool| tool.id.to_string()).collect();
    assert_eq!(ids, ["hi/greet"]);
}

#[test]
fn construct_fails_with_a_tool_error_when_a_host_wide_service_is_missing() {
    let name = PackId::parse("greeter").expect("a one-segment Pack id parses");
    let Err(error) = (GREETER.construct)(&name, Value::Null, &HostServices::new()) else {
        panic!("construct fails without its Host-wide service");
    };
    assert_eq!(
        error.to_string(),
        "greeter needs acme/greeting, and this Host provides none"
    );
}

#[test]
fn a_call_through_dyn_pack_answers_from_the_context_it_is_lent() {
    let pack = built_as("greeter");
    let tool = pack.tools()[0].id.clone();
    let mut run = HostServices::new();
    run.provide(&VISITOR, Arc::from("Ada"))
        .expect("a valid, new id is accepted");
    let call = TestCall::new(tool).with_services(run);
    let output =
        block_on(pack.call(call.context(), json!({"mark": "!"}))).expect("the greet call succeeds");
    assert_eq!(output.text(), "Hello, Ada!");
    assert_eq!(output.trust(), OutputTrust::Untrusted);
}

#[test]
fn a_call_without_the_per_run_service_it_needs_fails() {
    let pack = built_as("greeter");
    let call = TestCall::new(pack.tools()[0].id.clone());
    let error = block_on(pack.call(call.context(), json!({})))
        .expect_err("the greet call fails without a visitor");
    assert_eq!(error.to_string(), "no visitor is in this run");
}

#[test]
fn pack_info_from_new_has_no_prelude_and_no_needs_until_set() {
    const BARE: PackInfo = PackInfo::new("acme/bare", construct);
    const FULL: PackInfo = PackInfo::new("acme/full", construct)
        .prelude("local pack = ...\n")
        .needs(NEEDS);

    assert_eq!(BARE.fqn, "acme/bare");
    assert_eq!(BARE.prelude, None);
    assert!(BARE.needs.is_empty());
    assert_eq!(FULL.fqn, "acme/full");
    assert_eq!(FULL.prelude, Some("local pack = ...\n"));
    assert_eq!(FULL.needs, [VISITOR.id()]);
    let name = PackId::parse("hi").expect("a one-segment Pack id parses");
    for info in [BARE, FULL] {
        let Ok(pack) = (info.construct)(&name, Value::Null, &host_wide()) else {
            panic!(
                "{}: construct succeeds with its Host-wide service",
                info.fqn
            );
        };
        let ids: Vec<String> = pack.tools().iter().map(|t| t.id.to_string()).collect();
        assert_eq!(ids, ["hi/greet"], "{}", info.fqn);
    }
}

#[test]
fn pack_info_shows_its_fqn_prelude_presence_and_needs_in_debug_output() {
    let shown = format!("{GREETER:?}");
    assert!(shown.contains("acme/greeter"), "{shown}");
    assert!(shown.contains("prelude: true"), "{shown}");
    assert!(shown.contains("acme/visitor"), "{shown}");
    assert!(
        !shown.contains("local pack"),
        "the prelude's text is not shown: {shown}"
    );
}
