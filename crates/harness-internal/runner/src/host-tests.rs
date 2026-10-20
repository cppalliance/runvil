//! Tests for the Host's install and each run's snapshot: the default name,
//! the refused names, a construct failure stored as unavailable, the
//! requirements a run reports, the built-in that offers nothing, the tools
//! its catalog keeps, the preludes it installs, and where each call goes.

use std::collections::BTreeMap;
use std::sync::Arc;

use runvil::effect::{ToolCallOrigin, ToolCaller};
use runvil::vfs::{Origin, VfsRef};
use runvil::{MissingService, Prompt, UnavailablePack};
use runvil_plugin::{
    HostServices, Pack, PackFuture, PackId, PackInfo, ServiceId, ServiceKey, ToolContext,
    ToolError, ToolId, ToolInfo, ToolOutput,
};
use serde_json::{Value, json};

use super::{HostContext, InstallError};
use crate::performers::ToolPerformer;
use crate::registry::PackRegistry;

/// A Host-wide service a fixture's `construct` reads.
const BACKEND: ServiceKey<str> = ServiceKey::new("tests/backend");

/// A per-run service a fixture's calls read.
const SESSION: ServiceKey<str> = ServiceKey::new("tests/session");

const NEEDS_SESSION: &[ServiceId] = &[SESSION.id()];

/// Offers the tools its configuration lists and answers each call with
/// the name it was installed under and the called tool's id.
struct Listed {
    name: PackId,
    tools: Vec<ToolInfo>,
}

impl Pack for Listed {
    fn tools(&self) -> Vec<ToolInfo> {
        self.tools.clone()
    }

    fn call<'a>(
        &'a self,
        cx: ToolContext<'a>,
        _args: Value,
    ) -> PackFuture<'a, Result<ToolOutput, ToolError>> {
        Box::pin(async move { Ok(ToolOutput::trusted(format!("{}:{}", self.name, cx.tool()))) })
    }
}

/// Builds a [`Listed`] from a configuration that is a list of
/// `{ id, survives_stop }` tool entries, taking each id as given.
#[expect(
    clippy::needless_pass_by_value,
    clippy::unnecessary_wraps,
    reason = "the PackInfo construct signature fixes the argument and return types"
)]
fn listed(
    name: &PackId,
    config: Value,
    _services: &HostServices,
) -> Result<Arc<dyn Pack>, ToolError> {
    let mut tools = Vec::new();
    for entry in config.as_array().into_iter().flatten() {
        let id = ToolId::parse(entry["id"].as_str().unwrap()).unwrap();
        let schema = json!({ "type": "object", "properties": {} });
        let info = ToolInfo::new(id, "A listed tool.", schema)
            .survives_stop(entry["survives_stop"].as_bool().unwrap_or(false));
        tools.push(info);
    }
    Ok(Arc::new(Listed {
        name: name.clone(),
        tools,
    }))
}

/// Builds a [`Listed`] with one `<name>/use` tool, failing when the Host
/// provides no [`BACKEND`].
fn backed(
    name: &PackId,
    _config: Value,
    services: &HostServices,
) -> Result<Arc<dyn Pack>, ToolError> {
    if services.get(&BACKEND).is_none() {
        return Err(ToolError::message(
            "backed needs tests/backend, and this host provides none",
        ));
    }
    listed(name, json!([tool(&format!("{name}/use"))]), services)
}

/// A Pack with the FQN `fqn` that offers the configured tools.
const fn info(fqn: &'static str) -> PackInfo {
    PackInfo::new(fqn, listed)
}

/// One tool entry of a [`listed`] configuration.
fn tool(id: &str) -> Value {
    json!({ "id": id })
}

fn name(text: &str) -> PackId {
    PackId::parse(text).unwrap()
}

/// A Host with no Host-wide services over a registry holding `infos`.
fn host(infos: &[PackInfo]) -> HostContext {
    host_with(HostServices::new(), infos)
}

/// A Host over the Host-wide `services` and a registry holding `infos`.
fn host_with(services: HostServices, infos: &[PackInfo]) -> HostContext {
    let mut packs = PackRegistry::new();
    for info in infos {
        packs.add(*info).unwrap();
    }
    HostContext::new(packs, services)
}

/// A prompt with the given `uses:` and `tools:` frontmatter lines.
fn prompt(frontmatter: &str) -> Prompt {
    let source = format!(
        "---\nname: host-test\ndescription: d\nrunvil: 0\n{frontmatter}---\n\n\
         # Title\n\n## Only\n\nDone.\n"
    );
    let (prompt, _events) = Prompt::parse(&source, "host-test");
    prompt.unwrap()
}

/// The ids in a run's catalog, in catalog order.
fn catalog_ids(host: &HostContext, services: HostServices, prompt: &Prompt) -> Vec<String> {
    let (run, _requirements) = host.begin_run(services, BTreeMap::new(), prompt);
    run.catalog()
        .tools()
        .iter()
        .map(|tool| tool.id.to_string())
        .collect()
}

#[test]
fn install_names_the_pack_after_its_fqns_second_segment_unless_the_host_picks_one() {
    let mut host = host(&[info("acme/web")]);
    assert_eq!(host.install("acme/web", None, Value::Null), Ok(name("web")));
    assert_eq!(
        host.install("acme/web", Some(name("browse")), Value::Null),
        Ok(name("browse"))
    );
}

#[test]
fn install_refuses_a_name_already_installed_and_its_punctuation_twin() {
    let mut host = host(&[
        info("acme/user-input"),
        info("other/user_input"),
        info("other/ask"),
    ]);
    host.install("acme/user-input", None, Value::Null).unwrap();
    let taken = |name_: &str| InstallError::NameTaken {
        name: name(name_),
        existing: name("user-input"),
    };
    assert_eq!(
        host.install("acme/user-input", None, Value::Null),
        Err(taken("user-input"))
    );
    assert_eq!(
        host.install("other/user_input", None, Value::Null),
        Err(taken("user_input"))
    );
    assert_eq!(
        host.install("other/ask", Some(name("user.input")), Value::Null),
        Err(taken("user.input"))
    );
    assert_eq!(
        taken("user_input").to_string(),
        "Pack id user_input is taken by the installed user-input; install it under another Pack id"
    );
}

#[test]
fn the_registry_and_install_refusals_name_the_pack() {
    let refusals = [
        InstallError::InvalidFqn { fqn: "web" },
        InstallError::DuplicatePack { fqn: "acme/web" },
        InstallError::UnknownPlugin {
            fqn: "acme/web".to_owned(),
        },
    ];
    let messages: Vec<String> = refusals.iter().map(ToString::to_string).collect();
    assert_eq!(
        messages,
        [
            "Pack fully qualified name `web` is not a vendor/name pair",
            "Pack `acme/web` is already in the registry",
            "Pack `acme/web` is not in the registry; add it with PackRegistry::add before building the HostContext",
        ]
    );
}

#[test]
fn a_construct_failure_is_stored_and_reported_as_unavailable_with_its_reason() {
    let backed_info = PackInfo::new("acme/backed", backed);
    let mut bare = host(&[backed_info]);
    assert_eq!(
        bare.install(backed_info.fqn, None, Value::Null),
        Ok(name("backed")),
        "a construct failure is not an install error"
    );
    let (_run, requirements) = bare.begin_run(
        HostServices::new(),
        BTreeMap::new(),
        &prompt("uses:\n  - backed\n"),
    );
    assert_eq!(
        requirements.unavailable,
        [UnavailablePack::new(
            name("backed"),
            "backed needs tests/backend, and this host provides none"
        )]
    );
    assert!(requirements.missing_required.is_empty());

    let mut wide = HostServices::new();
    wide.provide(&BACKEND, Arc::from("up")).unwrap();
    let mut provided = host_with(wide, &[backed_info]);
    provided
        .install(backed_info.fqn, None, Value::Null)
        .unwrap();
    let (_run, requirements) = provided.begin_run(
        HostServices::new(),
        BTreeMap::new(),
        &prompt("uses:\n  - backed\n"),
    );
    assert!(
        requirements.is_satisfied(),
        "construct reads the Host-wide services: {requirements:?}"
    );
}

#[test]
fn begin_run_reports_every_declared_pack_it_cannot_serve() {
    let needy = info("acme/needy").needs(NEEDS_SESSION);
    let broken = PackInfo::new("acme/broken", backed);
    let mut host = host(&[needy, broken, info("acme/fine")]);
    host.install(needy.fqn, None, Value::Null).unwrap();
    host.install(broken.fqn, None, Value::Null).unwrap();
    host.install("acme/fine", None, Value::Null).unwrap();

    let (_run, requirements) = host.begin_run(
        HostServices::new(),
        BTreeMap::new(),
        &prompt("uses:\n  - needy\n  - absent\n  - fine\n  - broken\n"),
    );
    assert_eq!(requirements.missing_required, [name("absent")]);
    assert_eq!(
        requirements.missing_services,
        [MissingService::new(name("needy"), "tests/session")]
    );
    assert_eq!(
        requirements.unavailable.len(),
        1,
        "the declared Pack that failed to build is reported: {requirements:?}"
    );
    assert_eq!(requirements.unavailable[0].pack, name("broken"));
}

#[test]
fn a_declared_builtin_is_met_when_installed_and_missing_when_not() {
    let declares = prompt("uses:\n  - subagents\n");
    let mut host = host(&[]);
    let (_run, requirements) = host.begin_run(HostServices::new(), BTreeMap::new(), &declares);
    assert_eq!(requirements.missing_required, [name("subagents")]);

    assert_eq!(
        host.install("runvil/subagents", None, Value::Null),
        Ok(name("subagents"))
    );
    let (_run, requirements) = host.begin_run(HostServices::new(), BTreeMap::new(), &declares);
    assert!(requirements.is_satisfied(), "{requirements:?}");
}

#[test]
fn an_installed_builtin_adds_no_catalog_tool_and_no_prelude() {
    let mut host = host(&[]);
    host.install("runvil/subagents", None, Value::Null).unwrap();
    let declares = prompt("uses:\n  - subagents\n");
    assert!(catalog_ids(&host, HostServices::new(), &declares).is_empty());
    let (run, _requirements) = host.begin_run(HostServices::new(), BTreeMap::new(), &declares);
    assert!(run.preludes(&declares).is_empty());
}

#[test]
fn the_roster_lists_the_builtin_stub_as_usable_and_each_pack_that_cannot_serve_with_its_reason() {
    let needy = info("acme/needy").needs(NEEDS_SESSION);
    let mut host = host(&[info("acme/kit"), needy]);
    host.install("runvil/subagents", None, Value::Null).unwrap();
    host.install("acme/kit", None, Value::Null).unwrap();
    host.install(needy.fqn, None, Value::Null).unwrap();
    let failures = BTreeMap::from([(name("kit"), "the kit server is down".to_owned())]);
    let (run, _requirements) = host.begin_run(HostServices::new(), failures, &prompt(""));
    let (usable, unavailable) = run.roster();
    assert_eq!(
        usable,
        [name("subagents")],
        "the undeclared stub with no tools is on the roster"
    );
    assert_eq!(
        unavailable,
        [
            UnavailablePack::new(name("kit"), "the kit server is down"),
            UnavailablePack::new(
                name("needy"),
                "the run lacks the services it needs: tests/session"
            ),
        ],
        "a failed ready and an unmet need each carry their reason"
    );
}

#[test]
fn begin_run_drops_a_tool_outside_its_pack_and_a_repeated_id() {
    let mut host = host(&[info("acme/kit")]);
    let config = json!([tool("kit/good"), tool("other/escape"), tool("kit/good")]);
    host.install("acme/kit", None, config).unwrap();
    assert_eq!(
        catalog_ids(&host, HostServices::new(), &prompt("")),
        ["kit/good"]
    );
}

#[test]
fn a_pack_whose_needs_the_run_lacks_offers_it_no_tools() {
    let needy = info("acme/needy").needs(NEEDS_SESSION);
    let mut host = host(&[needy]);
    host.install(needy.fqn, None, json!([tool("needy/use")]))
        .unwrap();
    assert!(catalog_ids(&host, HostServices::new(), &prompt("")).is_empty());

    let mut run = HostServices::new();
    run.provide(&SESSION, Arc::from("ada")).unwrap();
    assert_eq!(catalog_ids(&host, run, &prompt("")), ["needy/use"]);
}

#[test]
fn an_undeclared_packs_tools_reach_the_catalog_but_its_prelude_does_not() {
    let declared = info("acme/declared").prelude("declared = {}\n");
    let extra = info("acme/extra").prelude("extra = {}\n");
    let mut host = host(&[declared, extra]);
    host.install(declared.fqn, None, json!([tool("declared/a")]))
        .unwrap();
    host.install(extra.fqn, None, json!([tool("extra/b")]))
        .unwrap();
    let prompt = prompt("uses:\n  - declared\n");
    assert_eq!(
        catalog_ids(&host, HostServices::new(), &prompt),
        ["declared/a", "extra/b"]
    );
    let (run, _requirements) = host.begin_run(HostServices::new(), BTreeMap::new(), &prompt);
    let preludes: Vec<String> = run
        .preludes(&prompt)
        .iter()
        .map(|prelude| prelude.pack().to_string())
        .collect();
    assert_eq!(preludes, ["declared"]);
}

#[test]
fn preludes_follow_the_prompts_declaration_order_and_carry_the_installed_name() {
    let with_prelude = |fqn: &'static str| info(fqn).prelude("local pack = ...\n");
    let mut host = host(&[with_prelude("acme/first"), with_prelude("acme/second")]);
    for fqn in ["acme/first", "acme/second"] {
        host.install(fqn, None, Value::Null).unwrap();
    }
    host.install("acme/first", Some(name("third")), Value::Null)
        .unwrap();
    let prompt = prompt("uses:\n  - third\n  - second\n  - first\n");
    let (run, _requirements) = host.begin_run(HostServices::new(), BTreeMap::new(), &prompt);
    let order: Vec<String> = run
        .preludes(&prompt)
        .iter()
        .map(|prelude| prelude.pack().to_string())
        .collect();
    assert_eq!(order, ["third", "second", "first"]);
}

/// Calls `tool` through `run` from a script and returns its result.
async fn call(run: &dyn ToolPerformer, tool: &str) -> Result<ToolOutput, ToolError> {
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
    let mut host = host(&[info("acme/one"), info("acme/two")]);
    host.install("acme/one", None, json!([tool("one/echo")]))
        .unwrap();
    host.install("acme/two", None, json!([tool("two/echo")]))
        .unwrap();
    let (run, _requirements) = host.begin_run(HostServices::new(), BTreeMap::new(), &prompt(""));

    let answer = |result: Result<ToolOutput, ToolError>| result.unwrap().text().to_owned();
    assert_eq!(answer(call(&run, "one/echo").await), "one:one/echo");
    assert_eq!(answer(call(&run, "two/echo").await), "two:two/echo");
    let error = call(&run, "three/echo")
        .await
        .expect_err("no Pack is installed under three");
    assert!(error.to_string().contains("three/echo"), "{error}");
}

#[tokio::test]
async fn a_call_to_a_pack_the_run_cannot_use_fails_naming_the_tool() {
    let needy = info("acme/needy").needs(NEEDS_SESSION);
    let mut host = host(&[needy]);
    host.install(needy.fqn, None, json!([tool("needy/use")]))
        .unwrap();
    let (run, _requirements) = host.begin_run(HostServices::new(), BTreeMap::new(), &prompt(""));
    let error = call(&run, "needy/use")
        .await
        .expect_err("the run lacks the Pack's needs");
    assert!(error.to_string().contains("needy/use"), "{error}");
}

#[test]
fn survives_stop_answers_from_the_runs_snapshot_of_tool_info() {
    let mut host = host(&[info("acme/kit")]);
    let config = json!([
        { "id": "kit/ask", "survives_stop": true },
        tool("kit/fetch"),
    ]);
    host.install("acme/kit", None, config).unwrap();
    let (run, _requirements) = host.begin_run(HostServices::new(), BTreeMap::new(), &prompt(""));
    let survives = |id: &str| run.survives_stop(&ToolId::parse(id).unwrap());
    assert!(survives("kit/ask"));
    assert!(!survives("kit/fetch"));
    assert!(
        !survives("other/ask"),
        "a tool the run lacks does not survive"
    );
}
