//! The pick of each run's Packs: a bare entry means the runvil Pack of
//! its id, a declared third-party Pack stands in for the runvil Pack of
//! its id, an undeclared third-party Pack is in no run, what the pick
//! cannot resolve is on the roster with its reason or, with none, left
//! off it, and the ready wait covers only the run's Packs, keying each
//! failure by FQN.

use futures_util::FutureExt as _;
use runvil::cancel::CancelHandle;

use super::*;

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
        catalog_ids(&host, HostServices::new(), &declares),
        ["web/runvil"]
    );
    let run = begin(&host, HostServices::new(), BTreeMap::new(), &declares);
    assert_eq!(run.roster().get(&name("web")), Some(&usable_from("runvil")));
}

#[test]
fn a_declared_third_party_pack_replaces_the_runvil_pack_of_its_id() {
    let host = vendors("ready");
    assert_eq!(
        catalog_ids(&host, HostServices::new(), &prompt("uses:\n  - acme/web\n")),
        ["web/acme"]
    );
    assert_eq!(
        catalog_ids(
            &host,
            HostServices::new(),
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
        catalog_ids(&host, HostServices::new(), &prompt("")),
        ["web/runvil"]
    );
    assert_eq!(usable(&host, &prompt("")), ["subagents", "web"]);
}

#[test]
fn a_bare_entry_that_only_another_vendor_has_is_unavailable_naming_each_fqn() {
    let one = vendors("ready");
    let declares = prompt("uses:\n  - poop\n");
    let run = begin(&one, HostServices::new(), BTreeMap::new(), &declares);
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
    let run = begin(&both, HostServices::new(), BTreeMap::new(), &declares);
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
    let run = begin(&host, HostServices::new(), BTreeMap::new(), &declares);
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
    let needy = info("acme/needy").needs(NEEDS_SESSION);
    let broken = PackInfo::new("acme/broken", backed);
    let host = host(&[
        (needy, Value::Null),
        (broken, Value::Null),
        (info("acme/fine"), Value::Null),
    ]);
    let declares = prompt("uses:\n  - acme/needy\n  - absent\n  - acme/fine\n  - acme/broken\n");
    let run = begin(&host, HostServices::new(), BTreeMap::new(), &declares);
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
                unavailable_for("the run lacks the services it needs: tests/session"),
            ),
            (name("subagents"), usable_from("runvil")),
        ]),
        "the absent Pack, which no vendor has, stays off the roster"
    );
    assert_eq!(
        refusal(&run, &declares),
        "the environment cannot satisfy this prompt:\n\
         - missing required Pack: absent\n\
         - needy is unavailable: the run lacks the services it needs: tests/session\n\
         - broken is unavailable: backed needs tests/backend, and this host provides none \
         (at prompt:5)"
    );
}

#[test]
fn refusals_name_each_declared_pack_not_installed_or_whose_construct_failed() {
    let broken = PackInfo::new("acme/broken", backed);
    let host = host(&[
        (info("runvil/web"), Value::Null),
        (info("acme/poop"), Value::Null),
        (broken, Value::Null),
    ]);
    let declares = prompt("uses:\n  - web\n  - nope\n  - acme/gone\n  - poop\n  - acme/broken\n");
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
            (
                name("broken"),
                "backed needs tests/backend, and this host provides none".to_owned(),
            ),
        ],
        "a bare id means runvil/<id>, and the built runvil/web is no refusal"
    );
}

#[test]
fn refusals_are_empty_when_every_declared_pack_built() {
    let host = vendors("fails");
    assert!(
        host.refusals(&prompt("uses:\n  - web\n  - acme/poop\n"))
            .is_empty(),
        "a Pack whose ready fails still built"
    );
    assert!(host.refusals(&prompt("")).is_empty());
}

#[test]
fn the_ready_wait_covers_only_the_runs_packs() {
    let host = vendors("pends");
    let cancel = CancelHandle::new();
    let packs = host.pick(&prompt("uses:\n  - web\n"));
    let failures = host.wait_ready(&packs, &cancel).now_or_never();
    assert_eq!(
        failures,
        Some(BTreeMap::new()),
        "a run that does not declare acme/web never waits on it"
    );
    let packs = host.pick(&prompt(""));
    assert!(
        host.wait_ready(&packs, &cancel).now_or_never().is_some(),
        "an undeclared third-party Pack is no run's to wait on"
    );
    let packs = host.pick(&prompt("uses:\n  - acme/web\n"));
    assert!(
        host.wait_ready(&packs, &cancel).now_or_never().is_none(),
        "a run that declares acme/web waits on it"
    );
}

#[test]
fn a_ready_failure_is_keyed_by_fqn_and_never_reaches_another_vendors_pack() {
    let host = vendors("fails");
    let cancel = CancelHandle::new();

    let declares_acme = prompt("uses:\n  - acme/web\n");
    let packs = host.pick(&declares_acme);
    let failures = host.wait_ready(&packs, &cancel).now_or_never().unwrap();
    assert_eq!(
        failures,
        BTreeMap::from([("acme/web", "web will not be ready".to_owned())])
    );
    let run = host.begin_run(&packs, HostServices::new(), failures);
    assert_eq!(
        run.roster().get(&name("web")),
        Some(&unavailable_for("web will not be ready"))
    );

    let declares_runvil = prompt("uses:\n  - web\n");
    let packs = host.pick(&declares_runvil);
    let failures = host.wait_ready(&packs, &cancel).now_or_never().unwrap();
    assert!(failures.is_empty(), "{failures:?}");
    let run = host.begin_run(&packs, HostServices::new(), failures);
    assert_eq!(run.roster().get(&name("web")), Some(&usable_from("runvil")));
    let ids: Vec<String> = run
        .catalog()
        .tools()
        .iter()
        .map(|tool| tool.id.to_string())
        .collect();
    assert_eq!(ids, ["web/runvil"], "runvil/web still serves the run");
}
