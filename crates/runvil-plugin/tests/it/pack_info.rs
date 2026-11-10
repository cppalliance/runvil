//! A fixture Plugin crate's whole Pack side: a `GREETER` constant, an
//! `install` that reads a Host-wide service into the Pack's factory, a
//! factory that names its tool under the Pack id it receives, a plain
//! function of the Pack id that is a factory too, and an `impl Pack` whose
//! `call` answers from what its instance holds and whose `render` is the
//! default.

use std::future::Future;
use std::num::NonZeroU32;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use runvil_plugin::testing::TestCall;
use runvil_plugin::{
    BoxFuture, Error, ErrorCode, HostServices, ModelId, ModelInfo, OutputTrust, Pack, PackFactory,
    PackId, PackInfo, PackRegistry, RenderContext, ServiceKey, ThinkingMode, ToolContext, ToolId,
    ToolInfo, ToolOutput, make_tool_error,
};
use serde_json::{Value, json};

/// The Host-wide service `install` reads.
const GREETING: ServiceKey<str> = ServiceKey::new("acme/greeting");

const GREETER: PackInfo = PackInfo::new("acme/greeter").prelude("local pack = ...\n");

/// The fixture's Plugin: inserts `GREETER` with a factory over the
/// Host-wide greeting, or unavailable when its config is set or the
/// greeting is missing.
#[expect(
    clippy::needless_pass_by_value,
    reason = "the Plugin signature fixes the argument types"
)]
fn install(packs: &mut PackRegistry, config: Value, services: &HostServices) -> Result<(), Error> {
    if !config["greeter"].is_null() {
        return packs.insert_unavailable(
            GREETER,
            make_tool_error(ErrorCode::ToolOther, "greeter takes no configuration"),
        );
    }
    match services.get(&GREETING) {
        Some(greeting) => packs.insert(GREETER, Greetings { greeting }),
        None => packs.insert_unavailable(
            GREETER,
            make_tool_error(
                ErrorCode::ToolOther,
                "greeter needs acme/greeting, and this Host provides none",
            ),
        ),
    }
}

/// The factory: what every instance shares, the Host-wide greeting.
struct Greetings {
    greeting: Arc<str>,
}

impl PackFactory for Greetings {
    fn new_pack(&self, id: &PackId) -> Result<Arc<dyn Pack>, Error> {
        new_greeter(id, Arc::clone(&self.greeting))
    }
}

/// An instance under the Pack id `id` that greets with `greeting`.
fn new_greeter(id: &PackId, greeting: Arc<str>) -> Result<Arc<dyn Pack>, Error> {
    let tool = ToolId::parse(&format!("{id}/greet")).map_err(|e| {
        make_tool_error(ErrorCode::ToolBackend, "greeter could not name its tool").with_cause(&e)
    })?;
    let tools = vec![ToolInfo::new(
        tool,
        "Greets whoever runs the prompt.",
        json!({"type": "object", "properties": {"mark": {"type": "string"}}}),
    )];
    Ok(Arc::new(Greeter { greeting, tools }))
}

/// A Pack that shares nothing, so a plain function of the Pack id is its
/// factory.
fn new_plain(id: &PackId) -> Result<Arc<dyn Pack>, Error> {
    new_greeter(id, Arc::from("Hi"))
}

/// One run's instance: the greeting it took from its factory.
struct Greeter {
    greeting: Arc<str>,
    tools: Vec<ToolInfo>,
}

impl Pack for Greeter {
    fn tools(&self) -> Vec<ToolInfo> {
        self.tools.clone()
    }

    fn call<'a>(
        &'a self,
        cx: ToolContext<'a>,
        args: Value,
    ) -> BoxFuture<'a, Result<ToolOutput, Error>> {
        Box::pin(async move {
            match cx.tool().name() {
                "greet" => {
                    let mark = args.get("mark").and_then(Value::as_str).unwrap_or(".");
                    Ok(ToolOutput::untrusted(format!("{}{mark}", self.greeting)))
                }
                other => Err(make_tool_error(
                    ErrorCode::ToolOther,
                    format!("greeter has no tool named {other}"),
                )),
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

/// The factory `install` inserts as `fqn` into a registry, or the reason
/// it inserted the Pack unavailable.
fn factory_of(packs: PackRegistry, fqn: &str) -> Result<Arc<dyn PackFactory>, Error> {
    let (_info, _id, factory) = packs
        .into_entries()
        .find(|(info, _, _)| info.fqn == fqn)
        .expect("the registry holds the Pack");
    factory
}

/// The greeter's factory `install` inserts with `config` and the Host-wide
/// `services`, or the reason it inserted the Pack unavailable.
fn installed(config: Value, services: &HostServices) -> Result<Arc<dyn PackFactory>, Error> {
    let mut packs = PackRegistry::new();
    install(&mut packs, config, services).expect("a fresh registry takes the greeter");
    factory_of(packs, GREETER.fqn)
}

/// The greeter's factory, inserted usable over the Host-wide greeting.
fn greetings() -> Arc<dyn PackFactory> {
    let Ok(factory) = installed(Value::Null, &host_wide()) else {
        panic!("install inserts the greeter usable when its Host-wide service is provided");
    };
    factory
}

/// The instance made under the Pack id `name`.
fn built_as(name: &str) -> Arc<dyn Pack> {
    let name = PackId::parse(name).expect("a one-segment Pack id parses");
    let Ok(pack) = greetings().new_pack(&name) else {
        panic!("the factory makes the Pack");
    };
    pack
}

#[test]
fn the_factory_names_the_tools_under_the_pack_id_it_receives() {
    let tools = built_as("hi").tools();
    let ids: Vec<String> = tools.iter().map(|tool| tool.id.to_string()).collect();
    assert_eq!(ids, ["hi/greet"]);
}

#[test]
fn install_inserts_the_pack_unavailable_when_its_setup_fails() {
    for (config, services, reason) in [
        (
            Value::Null,
            HostServices::new(),
            "greeter needs acme/greeting, and this Host provides none",
        ),
        (
            json!({ "greeter": 1 }),
            host_wide(),
            "greeter takes no configuration",
        ),
    ] {
        let Err(error) = installed(config, &services) else {
            panic!("{reason}: install inserts the greeter unavailable");
        };
        assert_eq!(error.to_string(), reason);
    }
}

#[test]
fn a_call_through_dyn_pack_answers_from_what_its_instance_took() {
    let pack = built_as("greeter");
    let call = TestCall::new(pack.tools()[0].id.clone());
    let output =
        block_on(pack.call(call.context(), json!({"mark": "!"}))).expect("the greet call succeeds");
    assert_eq!(output.text(), "Hello!");
    assert_eq!(output.trust(), OutputTrust::Untrusted);
}

#[test]
fn a_plain_function_of_the_pack_id_alone_is_a_factory() {
    let mut packs = PackRegistry::new();
    packs
        .insert(PackInfo::new("acme/plain"), new_plain)
        .expect("a fresh registry takes the Pack");
    let Ok(factory) = factory_of(packs, "acme/plain") else {
        panic!("the plain function is inserted as the Pack's factory");
    };
    let name = PackId::parse("plain").expect("a one-segment Pack id parses");
    let Ok(pack) = factory.new_pack(&name) else {
        panic!("the plain function makes the Pack from its id alone");
    };
    let call = TestCall::new(pack.tools()[0].id.clone());
    let output = block_on(pack.call(call.context(), json!({}))).expect("the greet call succeeds");
    assert_eq!(pack.tools()[0].id.to_string(), "plain/greet");
    assert_eq!(output.text(), "Hi.");
}

#[test]
fn the_default_render_returns_the_offered_tools_unchanged() {
    let pack = built_as("greeter");
    let model = ModelInfo::new(
        ModelId::gateway("analyst").expect("a valid model id"),
        "A careful analysis model",
        NonZeroU32::new(8_192).expect("a non-zero context window"),
        ThinkingMode::Never,
    );
    let farewell = ToolInfo::new(
        ToolId::parse("greeter/farewell").expect("a two-segment tool id parses"),
        "Bids whoever runs the prompt goodbye.",
        json!({"type": "object"}),
    );
    let offered = vec![pack.tools()[0].clone().wire_name("hello"), farewell];
    let rendered = block_on(pack.render(RenderContext::new("chat-1", &model), offered.clone()));
    assert_eq!(
        rendered, offered,
        "a Pack without its own render shows the model what the section offered, in offer order"
    );
}

#[test]
fn pack_info_from_new_has_no_description_or_prelude_until_set() {
    const BARE: PackInfo = PackInfo::new("acme/bare");
    const FULL: PackInfo = PackInfo::new("acme/full")
        .description("Greets the visitor")
        .prelude("local pack = ...\n");

    assert_eq!(BARE.fqn, "acme/bare");
    assert_eq!(BARE.description, "");
    assert_eq!(BARE.prelude, None);
    assert_eq!(FULL.fqn, "acme/full");
    assert_eq!(FULL.description, "Greets the visitor");
    assert_eq!(FULL.prelude, Some("local pack = ...\n"));
}

#[test]
fn pack_info_shows_its_fqn_and_prelude_presence_in_debug_output() {
    let shown = format!("{GREETER:?}");
    assert!(shown.contains("acme/greeter"), "{shown}");
    assert!(shown.contains("prelude: true"), "{shown}");
    assert!(
        !shown.contains("local pack"),
        "the prelude's text is not shown: {shown}"
    );
}
