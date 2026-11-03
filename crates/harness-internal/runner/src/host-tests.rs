//! Tests for the Host's Packs and each run's snapshot: `new` builds every
//! registered Pack, the built-in included, and stores a construct failure
//! as unavailable; and the snapshot's roster, catalog, preludes, and
//! calls. The pick of each run's Packs and the ready wait
//! over them sit in the `pick` child module, and the snapshot's renders
//! and its routing of a call the catalog lacks in the `render` child
//! module.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use runvil::effect::{ToolCallOrigin, ToolCaller};
use runvil::error::{Error, ErrorCode};
use runvil::packs::RosterEntry;
use runvil::timestamp::Timestamp;
use runvil::tools::tool_error;
use runvil::vfs::{Origin, VfsRef};
use runvil::{Prompt, Run, RunConfig, RunOutcome, RunSeed, Step};
use runvil_plugin::{
    HostServices, Pack, PackFuture, PackId, PackInfo, PackRegistry, ServiceId, ServiceKey,
    ToolContext, ToolId, ToolInfo, ToolOutput,
};
use serde_json::{Value, json};

use super::{HostContext, HostRunContext};
use crate::performers::ToolPerformer;

#[path = "host-tests-pick.rs"]
mod pick;
#[path = "host-tests-render.rs"]
mod render;

/// A Host-wide service a fixture's `construct` reads.
const BACKEND: ServiceKey<str> = ServiceKey::new("tests/backend");

/// A Host-wide service each [`listed`] construct records its Pack id in,
/// so a test sees which Packs were built.
const BUILT: ServiceKey<Mutex<Vec<String>>> = ServiceKey::new("tests/built");

/// A per-run service a fixture's calls read.
const SESSION: ServiceKey<str> = ServiceKey::new("tests/session");

const NEEDS_SESSION: &[ServiceId] = &[SESSION.id()];

/// Offers the tools its configuration lists and answers each call with
/// its Pack id and the called tool's id. Its `ready` answers as its
/// configuration's `ready` says: `"fails"` fails, `"pends"` never
/// resolves, and anything else is ready at once.
struct Listed {
    name: PackId,
    tools: Vec<ToolInfo>,
    ready: Option<String>,
}

impl Pack for Listed {
    fn tools(&self) -> Vec<ToolInfo> {
        self.tools.clone()
    }

    fn ready(&self) -> PackFuture<'_, Result<(), Error>> {
        match self.ready.as_deref() {
            Some("fails") => {
                let reason = format!("{} will not be ready", self.name);
                Box::pin(async move { Err(tool_error(ErrorCode::ToolOther, reason)) })
            }
            Some("pends") => Box::pin(std::future::pending()),
            _ => Box::pin(async { Ok(()) }),
        }
    }

    fn call<'a>(
        &'a self,
        cx: ToolContext<'a>,
        _args: Value,
    ) -> PackFuture<'a, Result<ToolOutput, Error>> {
        Box::pin(async move { Ok(ToolOutput::trusted(format!("{}:{}", self.name, cx.tool()))) })
    }
}

/// Builds a [`Listed`] from a configuration whose `tools` is a list of
/// `{ id }` tool entries, taking each id as given, and records the Pack
/// id in [`BUILT`] when the Host provides it.
#[expect(
    clippy::needless_pass_by_value,
    clippy::unnecessary_wraps,
    reason = "the PackInfo construct signature fixes the argument and return types"
)]
fn listed(name: &PackId, config: Value, services: &HostServices) -> Result<Arc<dyn Pack>, Error> {
    if let Some(built) = services.get(&BUILT) {
        built.lock().unwrap().push(name.to_string());
    }
    let tools = config["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|entry| listed_tool(entry["id"].as_str().unwrap()))
        .collect();
    Ok(Arc::new(Listed {
        name: name.clone(),
        tools,
        ready: config["ready"].as_str().map(str::to_owned),
    }))
}

/// The tool `id` as a [`Listed`] Pack lists it.
fn listed_tool(id: &str) -> ToolInfo {
    ToolInfo::new(
        ToolId::parse(id).unwrap(),
        "A listed tool.",
        json!({ "type": "object", "properties": {} }),
    )
}

/// Builds a [`Listed`] with one `<name>/use` tool, failing when the Host
/// provides no [`BACKEND`].
fn backed(name: &PackId, _config: Value, services: &HostServices) -> Result<Arc<dyn Pack>, Error> {
    if services.get(&BACKEND).is_none() {
        return Err(tool_error(
            ErrorCode::ToolOther,
            "backed needs tests/backend, and this host provides none",
        ));
    }
    listed(
        name,
        json!({ "tools": [tool(&format!("{name}/use"))] }),
        services,
    )
}

/// A Pack with the FQN `fqn` that offers the configured tools.
const fn info(fqn: &'static str) -> PackInfo {
    PackInfo::new(fqn, listed)
}

/// One tool entry of a [`listed`] configuration.
fn tool(id: &str) -> Value {
    json!({ "id": id })
}

/// A [`listed`] configuration offering `ids`.
fn offering(ids: &[&str]) -> Value {
    let tools: Vec<Value> = ids.iter().map(|id| tool(id)).collect();
    json!({ "tools": tools })
}

fn name(text: &str) -> PackId {
    PackId::parse(text).unwrap()
}

/// A Host with no Host-wide services over a registry holding each of
/// `packs` with its config.
fn host(packs: &[(PackInfo, Value)]) -> HostContext {
    host_with(HostServices::new(), packs)
}

/// A Host over the Host-wide `services` and a registry holding each of
/// `packs` with its config.
fn host_with(services: HostServices, packs: &[(PackInfo, Value)]) -> HostContext {
    let mut registry = PackRegistry::new();
    for (info, config) in packs {
        registry.insert(*info, config.clone()).unwrap();
    }
    HostContext::new(registry, services)
}

/// A prompt with the given `uses:` and `tools:` frontmatter lines.
fn prompt(frontmatter: &str) -> Prompt {
    let source = format!(
        "---\nname: host-test\ndescription: d\nrunvil: 0\n{frontmatter}---\n\n\
         # Title\n\n## Only\n\nDone.\n"
    );
    let (prompt, _events) = Prompt::parse(&source, "prompt", "host-test");
    prompt.unwrap()
}

/// Picks `prompt`'s Packs and takes the run's snapshot with `services`
/// and `failures`.
fn begin(
    host: &HostContext,
    services: HostServices,
    failures: BTreeMap<&'static str, String>,
    prompt: &Prompt,
) -> HostRunContext {
    let packs = host.pick(prompt);
    host.begin_run(&packs, services, failures)
}

/// The ids in a run's catalog, in catalog order.
fn catalog_ids(host: &HostContext, services: HostServices, prompt: &Prompt) -> Vec<String> {
    let run = begin(host, services, BTreeMap::new(), prompt);
    run.catalog()
        .tools()
        .iter()
        .map(|tool| tool.id.to_string())
        .collect()
}

/// The run's usable Pack ids, in id order.
fn usable(host: &HostContext, prompt: &Prompt) -> Vec<String> {
    let run = begin(host, HostServices::new(), BTreeMap::new(), prompt);
    run.roster()
        .into_iter()
        .filter(|(_, entry)| matches!(entry, RosterEntry::Usable { .. }))
        .map(|(name, _)| name.to_string())
        .collect()
}

/// The roster entry of a Pack usable from `vendor`.
fn usable_from(vendor: &str) -> RosterEntry {
    RosterEntry::Usable {
        vendor: vendor.to_owned(),
    }
}

/// The roster entry of a Pack unavailable for `reason`.
fn unavailable_for(reason: &str) -> RosterEntry {
    RosterEntry::Unavailable {
        reason: reason.to_owned(),
    }
}

/// The refusal notice the first step of a run of `prompt` over `run`'s
/// roster fails with.
fn refusal(run: &HostRunContext, prompt: &Prompt) -> String {
    let ctx = RunConfig::new(
        "host-test",
        RunSeed::from_bytes([0; 32]),
        Timestamp::UNIX_EPOCH,
    )
    .with_packs(run.roster());
    match Run::new(Arc::new(prompt.clone()), "", ctx).step() {
        Step::Done {
            outcome: RunOutcome::Failed { message, .. },
            ..
        } => message,
        other => panic!("the run's first step refuses it: {other:?}"),
    }
}

#[test]
fn the_roster_hands_each_usable_pack_with_the_vendor_the_run_picked() {
    let host = host(&[
        (info("runvil/kit"), offering(&["kit/a"])),
        (info("acme/kit"), offering(&["kit/b"])),
        (info("runvil/web"), offering(&["web/fetch"])),
        (info("acme/poop"), Value::Null),
    ]);
    let run = begin(
        &host,
        HostServices::new(),
        BTreeMap::new(),
        &prompt("uses:\n  - acme/kit\n"),
    );
    assert_eq!(
        run.roster(),
        BTreeMap::from([
            (name("kit"), usable_from("acme")),
            (name("subagents"), usable_from("runvil")),
            (name("web"), usable_from("runvil")),
        ]),
        "the declared acme/kit reports acme, and every undeclared Pack reports runvil"
    );
}

#[test]
fn new_builds_every_registered_pack_once_with_its_config_the_builtin_included() {
    let built = Arc::new(Mutex::new(Vec::new()));
    let mut wide = HostServices::new();
    wide.provide(&BUILT, Arc::clone(&built)).unwrap();
    let host = host_with(
        wide,
        &[
            (info("runvil/kit"), offering(&["kit/a"])),
            (info("acme/kit"), offering(&["kit/b"])),
            (info("acme/poop"), Value::Null),
        ],
    );
    assert_eq!(
        *built.lock().unwrap(),
        ["kit", "poop", "kit"],
        "each linked Pack is built once, in FQN order"
    );
    let shown = format!("{host:?}");
    for fqn in ["acme/kit", "acme/poop", "runvil/kit", "runvil/subagents"] {
        assert!(shown.contains(fqn), "{fqn} is built: {shown}");
    }
    assert_eq!(
        catalog_ids(&host, HostServices::new(), &prompt("uses:\n  - acme/kit\n")),
        ["kit/b"],
        "the stored config reached the declared Pack's construct"
    );
}

#[test]
fn a_run_over_a_fresh_registry_holds_a_usable_subagents_pack_with_no_tools() {
    let host = host(&[]);
    assert_eq!(usable(&host, &prompt("")), ["subagents"]);
    assert!(catalog_ids(&host, HostServices::new(), &prompt("")).is_empty());
}

#[test]
fn a_construct_failure_is_stored_and_reported_as_unavailable_with_its_reason() {
    let backed_info = PackInfo::new("acme/backed", backed);
    let bare = host(&[(backed_info, Value::Null)]);
    let declares = prompt("uses:\n  - acme/backed\n");
    let run = begin(&bare, HostServices::new(), BTreeMap::new(), &declares);
    assert_eq!(
        run.roster().get(&name("backed")),
        Some(&unavailable_for(
            "backed needs tests/backend, and this host provides none"
        ))
    );
    assert_eq!(
        refusal(&run, &declares),
        "the environment cannot satisfy this prompt:\n\
         - backed is unavailable: backed needs tests/backend, and this host provides none",
        "the run names the reason, and never calls the built Pack missing"
    );

    let mut wide = HostServices::new();
    wide.provide(&BACKEND, Arc::from("up")).unwrap();
    let provided = host_with(wide, &[(backed_info, Value::Null)]);
    let run = begin(&provided, HostServices::new(), BTreeMap::new(), &declares);
    assert_eq!(
        run.roster().get(&name("backed")),
        Some(&usable_from("acme")),
        "construct reads the Host-wide services"
    );
}

#[test]
fn a_declared_builtin_is_met_over_a_fresh_registry() {
    let host = host(&[]);
    let run = begin(
        &host,
        HostServices::new(),
        BTreeMap::new(),
        &prompt("uses:\n  - subagents\n"),
    );
    assert_eq!(
        run.roster().get(&name("subagents")),
        Some(&usable_from("runvil"))
    );
}

#[test]
fn the_builtin_adds_no_catalog_tool_and_no_prelude() {
    let host = host(&[]);
    let declares = prompt("uses:\n  - subagents\n");
    assert!(catalog_ids(&host, HostServices::new(), &declares).is_empty());
    let run = begin(&host, HostServices::new(), BTreeMap::new(), &declares);
    assert!(run.preludes(&declares).is_empty());
}

#[test]
fn the_roster_lists_the_builtin_stub_as_usable_and_each_pack_that_cannot_serve_with_its_reason() {
    let needy = info("acme/needy").needs(NEEDS_SESSION);
    let host = host(&[(info("acme/kit"), Value::Null), (needy, Value::Null)]);
    let failures = BTreeMap::from([("acme/kit", "the kit server is down".to_owned())]);
    let run = begin(
        &host,
        HostServices::new(),
        failures,
        &prompt("uses:\n  - acme/kit\n  - acme/needy\n"),
    );
    assert_eq!(
        run.roster(),
        BTreeMap::from([
            (name("kit"), unavailable_for("the kit server is down")),
            (
                name("needy"),
                unavailable_for("the run lacks the services it needs: tests/session"),
            ),
            (name("subagents"), usable_from("runvil")),
        ]),
        "the undeclared stub with no tools is on the roster, and a failed ready and an \
         unmet need each carry their reason"
    );
}

#[test]
fn begin_run_drops_a_tool_outside_its_pack_and_a_repeated_id() {
    let host = host(&[(
        info("runvil/kit"),
        offering(&["kit/good", "other/escape", "kit/good"]),
    )]);
    assert_eq!(
        catalog_ids(&host, HostServices::new(), &prompt("")),
        ["kit/good"]
    );
}

#[test]
fn a_pack_whose_needs_the_run_lacks_offers_it_no_tools() {
    let needy = info("runvil/needy").needs(NEEDS_SESSION);
    let host = host(&[(needy, offering(&["needy/use"]))]);
    assert!(catalog_ids(&host, HostServices::new(), &prompt("")).is_empty());

    let mut run = HostServices::new();
    run.provide(&SESSION, Arc::from("ada")).unwrap();
    assert_eq!(catalog_ids(&host, run, &prompt("")), ["needy/use"]);
}

#[test]
fn an_undeclared_runvil_packs_tools_reach_the_catalog_but_its_prelude_does_not() {
    let declared = info("runvil/declared").prelude("declared = {}\n");
    let extra = info("runvil/extra").prelude("extra = {}\n");
    let host = host(&[
        (declared, offering(&["declared/a"])),
        (extra, offering(&["extra/b"])),
    ]);
    let prompt = prompt("uses:\n  - declared\n");
    assert_eq!(
        catalog_ids(&host, HostServices::new(), &prompt),
        ["declared/a", "extra/b"]
    );
    let run = begin(&host, HostServices::new(), BTreeMap::new(), &prompt);
    let preludes: Vec<String> = run
        .preludes(&prompt)
        .iter()
        .map(|prelude| prelude.pack().to_string())
        .collect();
    assert_eq!(preludes, ["declared"]);
}

#[test]
fn preludes_follow_the_prompts_declaration_order_and_carry_the_pack_id() {
    let with_prelude = |fqn: &'static str| (info(fqn).prelude("local pack = ...\n"), Value::Null);
    let host = host(&[
        with_prelude("runvil/first"),
        with_prelude("acme/second"),
        with_prelude("runvil/third"),
    ]);
    let prompt = prompt("uses:\n  - third\n  - acme/second\n  - first\n");
    let run = begin(&host, HostServices::new(), BTreeMap::new(), &prompt);
    let order: Vec<String> = run
        .preludes(&prompt)
        .iter()
        .map(|prelude| prelude.pack().to_string())
        .collect();
    assert_eq!(order, ["third", "second", "first"]);
}

/// Calls `tool` through `run` from a script and returns its result.
async fn call(run: &dyn ToolPerformer, tool: &str) -> Result<ToolOutput, Error> {
    let access = VfsRef::default().acquire(Origin::new("host test")).unwrap();
    let origin = ToolCallOrigin::new("host-test", "Only", ToolCaller::Script);
    let tool = ToolId::parse(tool).unwrap();
    run.call(
        tool,
        "alias".to_owned(),
        Arc::new(access),
        origin,
        json!({}),
    )
    .await
}

#[tokio::test]
async fn a_call_goes_to_the_pack_its_tool_ids_first_segment_names() {
    let host = host(&[
        (info("runvil/one"), offering(&["one/echo"])),
        (info("runvil/two"), offering(&["two/echo"])),
    ]);
    let run = begin(&host, HostServices::new(), BTreeMap::new(), &prompt(""));

    let answer = |result: Result<ToolOutput, Error>| result.unwrap().text().to_owned();
    assert_eq!(answer(call(&run, "one/echo").await), "one:one/echo");
    assert_eq!(answer(call(&run, "two/echo").await), "two:two/echo");
    let error = call(&run, "three/echo")
        .await
        .expect_err("no Pack of the run has the id three");
    assert!(error.to_string().contains("three/echo"), "{error}");
}

#[tokio::test]
async fn a_call_to_a_pack_the_run_cannot_use_fails_naming_the_tool() {
    let needy = info("runvil/needy").needs(NEEDS_SESSION);
    let host = host(&[(needy, offering(&["needy/use"]))]);
    let run = begin(&host, HostServices::new(), BTreeMap::new(), &prompt(""));
    let error = call(&run, "needy/use")
        .await
        .expect_err("the run lacks the Pack's needs");
    assert!(error.to_string().contains("needy/use"), "{error}");
}
