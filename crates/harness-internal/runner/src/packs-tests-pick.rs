//! The pick of each run's Packs: a bare entry means the runvil Pack of
//! its id, a declared third-party Pack stands in for the runvil Pack of
//! its id, an undeclared third-party Pack is in no run, what the pick
//! cannot resolve is on the roster with its reason or, with none, left
//! off it, the Host refuses at parse only a declared Pack not installed or
//! inserted unavailable, and the ready wait covers only the run's Packs,
//! keying each failure by FQN.

use super::*;
use crate::packs::wait_ready;

/// A Host holding `runvil/web`, `acme/web`, and `acme/poop`, each
/// offering one tool named after its vendor: `web/runvil`, `web/acme`,
/// and `poop/acme`. `acme/web`'s `ready` answers as `acme_ready` says.
fn vendors(acme_ready: &str) -> Packs {
    host(&[
        (info("runvil/web"), offering(&["web/runvil"])),
        (
            info("acme/web"),
            json!({ "tools": [tool("web/acme")], "ready": acme_ready }),
        ),
        (info("acme/poop"), offering(&["poop/acme"])),
    ])
}

#[test]
fn a_bare_entry_runs_with_the_runvil_pack_of_that_id() {
    let host = vendors("ready");
    let declares = prompt("uses:\n  - web\n");
    assert_eq!(
        catalog_ids(&host, &HostServices::new(), &declares),
        ["web/runvil"]
    );
    let run = begin(&host, &HostServices::new(), BTreeMap::new(), &declares);
    assert_eq!(run.roster().get(&name("web")), Some(&usable_from("runvil")));
}

#[test]
fn a_declared_third_party_pack_replaces_the_runvil_pack_of_its_id() {
    let host = vendors("ready");
    assert_eq!(
        catalog_ids(
            &host,
            &HostServices::new(),
            &prompt("uses:\n  - acme/web\n")
        ),
        ["web/acme"]
    );
    assert_eq!(
        catalog_ids(
            &host,
            &HostServices::new(),
            &prompt("uses:\n  - acme/poop\n")
        ),
        ["poop/acme", "web/runvil"],
        "a third-party Pack of a new id joins the runvil Packs"
    );
}

#[test]
fn a_run_that_declares_nothing_holds_every_runvil_pack_and_no_third_party_one() {
    let host = vendors("ready");
    assert_eq!(
        catalog_ids(&host, &HostServices::new(), &prompt("")),
        ["web/runvil"]
    );
    assert_eq!(usable(&host, &prompt("")), ["subagents", "web"]);
}

#[test]
fn a_bare_entry_that_only_another_vendor_has_is_unavailable_naming_each_fqn() {
    let one = vendors("ready");
    let declares = prompt("uses:\n  - poop\n");
    let run = begin(&one, &HostServices::new(), BTreeMap::new(), &declares);
    assert_eq!(
        run.roster().get(&name("poop")),
        Some(&unavailable_for(
            "poop means runvil/poop, which is not installed; write acme/poop to use that Pack"
        ))
    );
    assert_eq!(
        refusal(&run, &declares),
        "the environment cannot satisfy this prompt:\n\
         - poop is unavailable: poop means runvil/poop, which is not installed; \
         write acme/poop to use that Pack (at prompt:5)",
        "the run names the reason and does not call the Pack missing"
    );

    let both = host(&[
        (info("zed/poop"), Value::Null),
        (info("acme/poop"), Value::Null),
    ]);
    let run = begin(&both, &HostServices::new(), BTreeMap::new(), &declares);
    assert_eq!(
        run.roster().get(&name("poop")),
        Some(&unavailable_for(
            "poop means runvil/poop, which is not installed; \
             write acme/poop or zed/poop to use that Pack"
        ))
    );
}

#[test]
fn an_entry_no_vendor_has_is_missing_and_an_absent_vendor_name_is_unavailable() {
    let host = vendors("ready");
    let declares = prompt("uses:\n  - nope\n  - acme/gone\n");
    let run = begin(&host, &HostServices::new(), BTreeMap::new(), &declares);
    let roster = run.roster();
    assert_eq!(
        roster.get(&name("nope")),
        None,
        "no reason, so off the roster"
    );
    assert_eq!(
        roster.get(&name("gone")),
        Some(&unavailable_for("acme/gone is not installed"))
    );
    assert_eq!(
        refusal(&run, &declares),
        "the environment cannot satisfy this prompt:\n\
         - missing required Pack: nope\n\
         - gone is unavailable: acme/gone is not installed (at prompt:5)"
    );
}

#[test]
fn the_roster_covers_every_declared_pack_the_run_cannot_serve() {
    let host = host_with(
        &HostServices::new(),
        &[
            (info("acme/needy"), needy, Value::Null),
            (PackInfo::new("acme/broken"), backed, Value::Null),
            (info("acme/fine"), listed, Value::Null),
        ],
    );
    let declares = prompt("uses:\n  - acme/needy\n  - absent\n  - acme/fine\n  - acme/broken\n");
    let run = begin(&host, &HostServices::new(), BTreeMap::new(), &declares);
    assert_eq!(
        run.roster(),
        BTreeMap::from([
            (
                name("broken"),
                unavailable_for("backed needs tests/backend, and this host provides none"),
            ),
            (name("fine"), usable_from("acme")),
            (
                name("needy"),
                unavailable_for("needy needs tests/session, and this run provides none"),
            ),
            (name("subagents"), usable_from("runvil")),
        ]),
        "the absent Pack, which no vendor has, stays off the roster"
    );
    assert_eq!(
        refusal(&run, &declares),
        "the environment cannot satisfy this prompt:\n\
         - missing required Pack: absent\n\
         - needy is unavailable: needy needs tests/session, and this run provides none\n\
         - broken is unavailable: backed needs tests/backend, and this host provides none \
         (at prompt:5)"
    );
}

#[test]
fn refusals_name_each_declared_pack_not_installed_or_inserted_unavailable() {
    let mut registry = PackRegistry::new();
    for (fqn, make) in [
        ("runvil/web", listed as Make),
        ("acme/poop", listed),
        ("acme/broken", backed),
    ] {
        registry
            .insert(info(fqn), factory(make, Value::Null, HostServices::new()))
            .unwrap();
    }
    registry
        .insert_unavailable(
            info("acme/offline"),
            make_tool_error(ErrorCode::ToolOther, "offline takes no configuration"),
        )
        .unwrap();
    let host = Packs::new(registry);
    let declares = prompt(
        "uses:\n  - web\n  - nope\n  - acme/gone\n  - poop\n  - acme/broken\n  - acme/offline\n",
    );
    assert_eq!(
        host.refusals(&declares),
        [
            (name("nope"), "runvil/nope is not installed".to_owned()),
            (name("gone"), "acme/gone is not installed".to_owned()),
            (
                name("poop"),
                "poop means runvil/poop, which is not installed; write acme/poop to use that Pack"
                    .to_owned(),
            ),
            (name("offline"), "offline takes no configuration".to_owned()),
        ],
        "a bare id means runvil/<id>, the installed runvil/web is no refusal, and neither is \
         acme/broken, whose factory only a run calls"
    );
}

#[test]
fn refusals_are_empty_when_every_declared_pack_is_installed() {
    let host = vendors("fails");
    assert!(
        host.refusals(&prompt("uses:\n  - web\n  - acme/poop\n"))
            .is_empty(),
        "a Pack whose ready fails is still installed"
    );
    assert!(host.refusals(&prompt("")).is_empty());
}

/// Whether the ready wait over the Packs a run of `prompt` holds on
/// `host` resolves without waiting, and its failures when it does.
fn waits(host: &Packs, prompt: &Prompt) -> Option<BTreeMap<&'static str, String>> {
    let picked = host.pick(prompt);
    let made = host.make(&picked, &HostServices::new());
    wait_ready(&made, &CancelHandle::new()).now_or_never()
}

#[test]
fn the_ready_wait_covers_only_the_runs_packs() {
    let host = vendors("pends");
    assert_eq!(
        waits(&host, &prompt("uses:\n  - web\n")),
        Some(BTreeMap::new()),
        "a run that does not declare acme/web never waits on it"
    );
    assert!(
        waits(&host, &prompt("")).is_some(),
        "an undeclared third-party Pack is no run's to wait on"
    );
    assert!(
        waits(&host, &prompt("uses:\n  - acme/web\n")).is_none(),
        "a run that declares acme/web waits on it"
    );
}

/// The snapshot of a run of `prompt` on `host` after the ready wait,
/// beside the failures the wait found.
fn waited(host: &Packs, prompt: &Prompt) -> (BTreeMap<&'static str, String>, RunPacks) {
    let picked = host.pick(prompt);
    let made = host.make(&picked, &HostServices::new());
    let failures = wait_ready(&made, &CancelHandle::new())
        .now_or_never()
        .unwrap();
    let run = RunPacks::snapshot(made, failures.clone(), &picked.unresolved);
    (failures, run)
}

#[test]
fn a_ready_failure_is_keyed_by_fqn_and_never_reaches_another_vendors_pack() {
    let host = vendors("fails");

    let (failures, run) = waited(&host, &prompt("uses:\n  - acme/web\n"));
    assert_eq!(
        failures,
        BTreeMap::from([("acme/web", "web will not be ready".to_owned())])
    );
    assert_eq!(
        run.roster().get(&name("web")),
        Some(&unavailable_for("web will not be ready"))
    );

    let (failures, run) = waited(&host, &prompt("uses:\n  - web\n"));
    assert!(failures.is_empty(), "{failures:?}");
    assert_eq!(run.roster().get(&name("web")), Some(&usable_from("runvil")));
    let ids: Vec<String> = run
        .catalog()
        .tools()
        .iter()
        .map(|tool| tool.id.to_string())
        .collect();
    assert_eq!(ids, ["web/runvil"], "runvil/web still serves the run");
}
