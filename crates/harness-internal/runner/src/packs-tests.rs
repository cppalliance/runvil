//! Tests for the Host's Packs and each run's snapshot: `new` makes no
//! Pack, and each snapshot makes its own instance of each Pack the run
//! holds and of no other, a factory's refusal or panic leaving that Pack
//! unavailable to that run alone and never waited on, and a run with no
//! user holding `runvil/user` unavailable; and the snapshot's roster,
//! catalog, preludes, and calls. The pick of each run's Packs and
//! the ready wait over them sit in the `pick` child module, and the
//! snapshot's renders and its routing of a call the catalog lacks in the
//! `render` child module.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use futures_util::FutureExt as _;
use runvil_engine::cancel::CancelHandle;
use runvil_engine::effect::{ToolCallOrigin, ToolCaller};
use runvil_engine::make_tool_error;
use runvil_engine::packs::RosterEntry;
use runvil_engine::vfs::{Origin, VfsRef};
use runvil_engine::{Error, ErrorCode};
use runvil_engine::{Prompt, Run, RunParams, RunSeed, Step, Timestamp};
use runvil_plugin::{
    BoxFuture, HostServices, Pack, PackFactory, PackId, PackInfo, PackRegistry, ServiceKey,
    ToolContext, ToolId, ToolInfo, ToolOutput,
};
use serde_json::{Value, json};

use super::{Packs, RunPacks};
use crate::performers::ToolPerformer;

#[path = "packs-tests-pick.rs"]
mod pick;
#[path = "packs-tests-render.rs"]
mod render;

/// A Host-wide service a fixture's factory reads.
const BACKEND: ServiceKey<str> = ServiceKey::new("tests/backend");

/// A Host-wide service each [`listed`] Pack records its Pack id in as it
/// is made, so a test sees which Packs were built.
const BUILT: ServiceKey<Mutex<Vec<String>>> = ServiceKey::new("tests/built");

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

    fn ready(&self) -> BoxFuture<'_, Result<(), Error>> {
        match self.ready.as_deref() {
            Some("fails") => {
                let reason = format!("{} will not be ready", self.name);
                Box::pin(async move { Err(make_tool_error(ErrorCode::ToolOther, reason)) })
            }
            Some("pends") => Box::pin(std::future::pending()),
            _ => Box::pin(async { Ok(()) }),
        }
    }

    fn call<'a>(
        &'a self,
        cx: ToolContext<'a>,
        _args: Value,
    ) -> BoxFuture<'a, Result<ToolOutput, Error>> {
        Box::pin(async move { Ok(ToolOutput::trusted(format!("{}:{}", self.name, cx.tool()))) })
    }
}

/// How a fixture factory makes a run's instance: from the Pack id, the
/// config a test gives it, and the Host-wide services it holds.
type Make = fn(&PackId, &Value, &HostServices) -> Result<Arc<dyn Pack>, Error>;

/// A factory that makes each run's instance with `make` from `config` and
/// the Host-wide `services`, which it holds as a Plugin's factory holds
/// what its `install` read.
fn factory(make: Make, config: Value, services: HostServices) -> impl PackFactory {
    move |id: &PackId| -> Result<Arc<dyn Pack>, Error> { make(id, &config, &services) }
}

/// Makes a [`Listed`] from a configuration whose `tools` is a list of
/// `{ id }` tool entries, taking each id as given, and records the Pack
/// id in [`BUILT`] when the Host provides it.
#[expect(
    clippy::unnecessary_wraps,
    reason = "the Make signature fixes the return type"
)]
fn listed(name: &PackId, config: &Value, wide: &HostServices) -> Result<Arc<dyn Pack>, Error> {
    if let Some(built) = wide.get(&BUILT) {
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

/// Makes a [`Listed`] with one `<name>/use` tool, refusing each run while
/// the Host provides no [`BACKEND`].
fn backed(name: &PackId, _config: &Value, wide: &HostServices) -> Result<Arc<dyn Pack>, Error> {
    if wide.get(&BACKEND).is_none() {
        return Err(make_tool_error(
            ErrorCode::ToolOther,
            "backed needs tests/backend, and this host provides none",
        ));
    }
    listed(
        name,
        &json!({ "tools": [tool(&format!("{name}/use"))] }),
        wide,
    )
}

/// A Pack with the FQN `fqn`, which [`host`] makes as a [`Listed`] Pack
/// that offers the configured tools.
const fn info(fqn: &'static str) -> PackInfo {
    PackInfo::new(fqn)
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
/// `packs` as a [`Listed`] Pack made from its config.
fn host(packs: &[(PackInfo, Value)]) -> Packs {
    let packs: Vec<(PackInfo, Make, Value)> = packs
        .iter()
        .map(|(info, config)| (*info, listed as Make, config.clone()))
        .collect();
    host_with(&HostServices::new(), &packs)
}

/// A Host over a registry holding each of `packs`, whose factory makes it
/// with its `Make` from its config and the Host-wide `services`.
fn host_with(services: &HostServices, packs: &[(PackInfo, Make, Value)]) -> Packs {
    let mut registry = PackRegistry::new();
    for (info, make, config) in packs {
        registry
            .insert(*info, factory(*make, config.clone(), services.clone()))
            .unwrap();
    }
    Packs::new(registry)
}

/// A prompt with the given `uses:` and `tools:` frontmatter lines.
fn prompt(frontmatter: &str) -> Prompt {
    let source = format!(
        "---\nname: host-test\ndescription: d\nrunvil: 0\n{frontmatter}---\n\n\
         # Title\n\n## Only\n\nDone.\n"
    );
    let prompt = Prompt::parse(&|_: &str| Ok(source.clone()), "prompt");
    prompt.unwrap()
}

/// Picks `prompt`'s Packs, makes them for a run with no user, and takes
/// the run's snapshot of them with `failures` as the Packs whose `ready`
/// failed, without waiting on any `ready`.
fn begin(host: &Packs, failures: BTreeMap<&'static str, String>, prompt: &Prompt) -> RunPacks {
    let picked = host.pick(prompt);
    let made = host.make(&picked, false);
    RunPacks::snapshot(made, failures, &picked.unresolved)
}

/// The snapshot of a run with no user of `prompt`'s Packs, or `None` when
/// it waits on a Pack's `ready`.
fn snapshot(host: &Packs, prompt: &Prompt) -> Option<RunPacks> {
    host.snapshot(prompt, false, &CancelHandle::new())
        .now_or_never()
}

/// The ids in `run`'s catalog, in catalog order.
fn snapshot_ids(run: &RunPacks) -> Vec<String> {
    run.catalog()
        .tools()
        .iter()
        .map(|tool| tool.id.to_string())
        .collect()
}

/// The ids in a run's catalog, in catalog order.
fn catalog_ids(host: &Packs, prompt: &Prompt) -> Vec<String> {
    snapshot_ids(&begin(host, BTreeMap::new(), prompt))
}

/// The run's usable Pack ids, in id order.
fn usable(host: &Packs, prompt: &Prompt) -> Vec<String> {
    let run = begin(host, BTreeMap::new(), prompt);
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
fn refusal(run: &RunPacks, prompt: &Prompt) -> String {
    let ctx = RunParams::new(RunSeed::from_bytes([0; 32]), Timestamp::UNIX_EPOCH)
        .with_packs(run.roster());
    match Run::new(Arc::new(prompt.clone()), "", ctx).step() {
        Step::Done {
            result: Err(error), ..
        } => error.located_message(),
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
    let run = begin(&host, BTreeMap::new(), &prompt("uses:\n  - acme/kit\n"));
    assert_eq!(
        run.roster(),
        BTreeMap::from([
            (name("kit"), usable_from("acme")),
            (name("subagents"), usable_from("runvil")),
            (name("user"), unavailable_for("this run has no user")),
            (name("web"), usable_from("runvil")),
        ]),
        "the declared acme/kit reports acme, every undeclared usable Pack reports runvil, and \
         a run with no user cannot use runvil/user"
    );
}

#[test]
fn each_snapshot_makes_each_pack_the_run_holds_once_and_each_run_gets_its_own_instance() {
    let built = Arc::new(Mutex::new(Vec::new()));
    let mut wide = HostServices::new();
    wide.provide(&BUILT, Arc::clone(&built)).unwrap();
    let host = host_with(
        &wide,
        &[
            (info("runvil/kit"), listed, offering(&["kit/a"])),
            (info("runvil/web"), listed, offering(&["web/fetch"])),
        ],
    );
    assert!(
        built.lock().unwrap().is_empty(),
        "installing the Packs makes none of them"
    );
    let plain = prompt("");
    let first = snapshot(&host, &plain).unwrap();
    assert_eq!(
        *built.lock().unwrap(),
        ["kit", "web"],
        "the snapshot makes each Pack the run holds once"
    );
    let second = snapshot(&host, &plain).unwrap();
    assert_eq!(
        *built.lock().unwrap(),
        ["kit", "web", "kit", "web"],
        "the next run makes them again"
    );
    for id in ["kit", "web"] {
        let mine = first.usable(&name(id)).unwrap();
        let theirs = second.usable(&name(id)).unwrap();
        assert!(
            !Arc::ptr_eq(mine, theirs),
            "each run gets its own {id} instance"
        );
    }
    assert_eq!(snapshot_ids(&first), ["kit/a", "web/fetch"]);
    let shown = format!("{host:?}");
    for fqn in [
        "runvil/kit",
        "runvil/subagents",
        "runvil/user",
        "runvil/web",
    ] {
        assert!(shown.contains(fqn), "{fqn} is installed: {shown}");
    }
}

#[test]
fn a_pack_the_run_does_not_hold_is_never_made() {
    let built = Arc::new(Mutex::new(Vec::new()));
    let mut wide = HostServices::new();
    wide.provide(&BUILT, Arc::clone(&built)).unwrap();
    let host = host_with(
        &wide,
        &[
            (info("runvil/kit"), listed, offering(&["kit/a"])),
            (info("acme/kit"), listed, offering(&["kit/b"])),
            (info("acme/poop"), listed, Value::Null),
        ],
    );
    let run = snapshot(&host, &prompt("uses:\n  - acme/kit\n")).unwrap();
    assert_eq!(
        *built.lock().unwrap(),
        ["kit"],
        "only one Pack is made: neither the runvil/kit the declaration replaces nor the \
         undeclared acme/poop"
    );
    assert_eq!(
        snapshot_ids(&run),
        ["kit/b"],
        "the one Pack made is the declared acme/kit, from its own config"
    );
}

#[test]
fn a_refusing_factory_leaves_its_pack_unavailable_to_that_run_only_and_the_next_run_makes_it_again()
{
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    let mut registry = PackRegistry::new();
    registry
        .insert(
            info("acme/flaky"),
            move |id: &PackId| -> Result<Arc<dyn Pack>, Error> {
                if counted.fetch_add(1, Ordering::SeqCst) == 0 {
                    return Err(make_tool_error(
                        ErrorCode::ToolOther,
                        "flaky is down for its first run",
                    ));
                }
                Ok(Arc::new(Listed {
                    name: id.clone(),
                    tools: vec![listed_tool("flaky/use")],
                    ready: None,
                }))
            },
        )
        .unwrap();
    let host = Packs::new(registry);
    let declares = prompt("uses:\n  - acme/flaky\n");
    assert!(
        host.refusals(&declares).is_empty(),
        "no run has made the Pack yet, so the Host refuses nothing at parse"
    );

    let first = snapshot(&host, &declares).unwrap();
    assert_eq!(
        first.roster().get(&name("flaky")),
        Some(&unavailable_for("flaky is down for its first run"))
    );
    let second = snapshot(&host, &declares).unwrap();
    assert_eq!(
        second.roster().get(&name("flaky")),
        Some(&usable_from("acme")),
        "the next run made it again"
    );
    assert_eq!(snapshot_ids(&second), ["flaky/use"]);
    assert_eq!(calls.load(Ordering::SeqCst), 2, "one factory call per run");
}

#[test]
fn a_panicking_factory_leaves_its_pack_unavailable_naming_the_panic() {
    let mut registry = PackRegistry::new();
    registry
        .insert(
            info("acme/brittle"),
            |_id: &PackId| -> Result<Arc<dyn Pack>, Error> { panic!("the brittle factory breaks") },
        )
        .unwrap();
    let host = Packs::new(registry);
    let declares = prompt("uses:\n  - acme/brittle\n");
    let run = snapshot(&host, &declares).unwrap();
    assert_eq!(
        run.roster().get(&name("brittle")),
        Some(&unavailable_for("the Pack's factory panicked"))
    );
    assert_eq!(
        refusal(&run, &declares),
        "the environment cannot satisfy this prompt:\n\
         - brittle is unavailable: the Pack's factory panicked (at prompt:5)"
    );
}

#[test]
fn a_pack_its_factory_refused_never_has_its_ready_polled() {
    let calls = AtomicUsize::new(0);
    let mut registry = PackRegistry::new();
    registry
        .insert(
            info("acme/pending"),
            move |id: &PackId| -> Result<Arc<dyn Pack>, Error> {
                if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    return Err(make_tool_error(
                        ErrorCode::ToolOther,
                        "pending is down for its first run",
                    ));
                }
                Ok(Arc::new(Listed {
                    name: id.clone(),
                    tools: Vec::new(),
                    ready: Some("pends".to_owned()),
                }))
            },
        )
        .unwrap();
    let host = Packs::new(registry);
    let declares = prompt("uses:\n  - acme/pending\n");

    let run = snapshot(&host, &declares).expect("a Pack its factory refused is never waited on");
    assert_eq!(
        run.roster().get(&name("pending")),
        Some(&unavailable_for("pending is down for its first run"))
    );
    assert!(
        snapshot(&host, &declares).is_none(),
        "the next run makes the Pack and waits on its ready, which never resolves"
    );
}

#[test]
fn a_run_over_a_fresh_registry_holds_a_usable_subagents_pack_with_no_tools() {
    let host = host(&[]);
    assert_eq!(usable(&host, &prompt("")), ["subagents"]);
    assert!(catalog_ids(&host, &prompt("")).is_empty());
}

#[test]
fn a_factory_refusal_is_reported_as_unavailable_with_its_reason() {
    let backed_pack = (PackInfo::new("acme/backed"), backed as Make, Value::Null);
    let bare = host_with(&HostServices::new(), std::slice::from_ref(&backed_pack));
    let declares = prompt("uses:\n  - acme/backed\n");
    let run = begin(&bare, BTreeMap::new(), &declares);
    assert_eq!(
        run.roster().get(&name("backed")),
        Some(&unavailable_for(
            "backed needs tests/backend, and this host provides none"
        ))
    );
    assert_eq!(
        refusal(&run, &declares),
        "the environment cannot satisfy this prompt:\n\
         - backed is unavailable: backed needs tests/backend, and this host provides none \
         (at prompt:5)",
        "the run names the reason, and never calls the installed Pack missing"
    );

    let mut wide = HostServices::new();
    wide.provide(&BACKEND, Arc::from("up")).unwrap();
    let provided = host_with(&wide, &[backed_pack]);
    let run = begin(&provided, BTreeMap::new(), &declares);
    assert_eq!(
        run.roster().get(&name("backed")),
        Some(&usable_from("acme")),
        "the factory reads the Host-wide services it holds"
    );
}

#[test]
fn a_declared_builtin_is_met_over_a_fresh_registry() {
    let host = host(&[]);
    let run = begin(&host, BTreeMap::new(), &prompt("uses:\n  - subagents\n"));
    assert_eq!(
        run.roster().get(&name("subagents")),
        Some(&usable_from("runvil"))
    );
}

#[test]
fn the_builtin_adds_no_catalog_tool_and_no_prelude() {
    let host = host(&[]);
    let declares = prompt("uses:\n  - subagents\n");
    assert!(catalog_ids(&host, &declares).is_empty());
    let run = begin(&host, BTreeMap::new(), &declares);
    assert!(run.preludes(&declares).is_empty());
}

#[test]
fn the_roster_lists_the_builtin_stub_as_usable_and_each_pack_that_cannot_serve_with_its_reason() {
    let host = host_with(
        &HostServices::new(),
        &[
            (info("acme/kit"), listed, Value::Null),
            (info("acme/backed"), backed, Value::Null),
        ],
    );
    let failures = BTreeMap::from([("acme/kit", "the kit server is down".to_owned())]);
    let run = begin(
        &host,
        failures,
        &prompt("uses:\n  - acme/kit\n  - acme/backed\n"),
    );
    assert_eq!(
        run.roster(),
        BTreeMap::from([
            (
                name("backed"),
                unavailable_for("backed needs tests/backend, and this host provides none"),
            ),
            (name("kit"), unavailable_for("the kit server is down")),
            (name("subagents"), usable_from("runvil")),
            (name("user"), unavailable_for("this run has no user")),
        ]),
        "the undeclared stub with no tools is on the roster, and a failed ready, a factory's \
         refusal, and a run with no user each carry their reason"
    );
}

#[test]
fn only_a_run_with_a_user_holds_runvil_user_usable() {
    let host = host(&[]);
    let roster = |prompt: &Prompt, has_user: bool| {
        let picked = host.pick(prompt);
        let made = host.make(&picked, has_user);
        RunPacks::snapshot(made, BTreeMap::new(), &picked.unresolved).roster()
    };
    for prompt in [prompt(""), prompt("uses:\n  - user\n")] {
        assert_eq!(
            roster(&prompt, true).get(&name("user")),
            Some(&usable_from("runvil")),
            "a run with a user holds runvil/user, declared or not"
        );
        assert_eq!(
            roster(&prompt, false).get(&name("user")),
            Some(&unavailable_for("this run has no user")),
            "a run with no user holds it unavailable, declared or not"
        );
    }
    let declares = prompt("uses:\n  - user\n");
    let picked = host.pick(&declares);
    let run = RunPacks::snapshot(
        host.make(&picked, true),
        BTreeMap::new(),
        &picked.unresolved,
    );
    assert!(
        run.catalog().tools().is_empty() && run.preludes(&declares).is_empty(),
        "runvil/user adds no catalog tool and no prelude"
    );
}

#[test]
fn begin_run_drops_a_tool_outside_its_pack_and_a_repeated_id() {
    let host = host(&[(
        info("runvil/kit"),
        offering(&["kit/good", "other/escape", "kit/good"]),
    )]);
    assert_eq!(catalog_ids(&host, &prompt("")), ["kit/good"]);
}

/// A Host holding `runvil/backed` over the Host-wide `services`, offering
/// `backed/use` to each run while they provide [`BACKEND`] and refusing
/// each run otherwise.
fn backed_host(services: &HostServices) -> Packs {
    host_with(services, &[(info("runvil/backed"), backed, Value::Null)])
}

#[test]
fn a_pack_whose_factory_refuses_the_run_offers_it_no_tools() {
    assert!(catalog_ids(&backed_host(&HostServices::new()), &prompt("")).is_empty());

    let mut wide = HostServices::new();
    wide.provide(&BACKEND, Arc::from("up")).unwrap();
    assert_eq!(
        catalog_ids(&backed_host(&wide), &prompt("")),
        ["backed/use"]
    );
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
    assert_eq!(catalog_ids(&host, &prompt), ["declared/a", "extra/b"]);
    let run = begin(&host, BTreeMap::new(), &prompt);
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
    let run = begin(&host, BTreeMap::new(), &prompt);
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
    let origin = ToolCallOrigin::new("Only", ToolCaller::Script);
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
    let run = begin(&host, BTreeMap::new(), &prompt(""));

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
    let host = backed_host(&HostServices::new());
    let run = begin(&host, BTreeMap::new(), &prompt(""));
    let error = call(&run, "backed/use")
        .await
        .expect_err("the Pack's factory refused the run");
    assert!(error.to_string().contains("backed/use"), "{error}");
}
