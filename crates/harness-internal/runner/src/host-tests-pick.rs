//! The pick of each run's Packs: a bare entry means the runvil Pack of
//! its id, a declared third-party Pack stands in for the runvil Pack of
//! its id, an undeclared third-party Pack is in no run, what the pick
//! cannot resolve is reported with its reason, and the ready wait covers
//! only the run's Packs, keying each failure by FQN.

use futures_util::FutureExt as _;
use runvil::cancel::CancelHandle;

use super::*;

/// A Host holding `runvil/web`, `acme/web`, and `acme/poop`, each
/// offering one tool named after its vendor: `web/runvil`, `web/acme`,
/// and `poop/acme`. `acme/web`'s `ready` answers as `acme_ready` says.
fn vendors(acme_ready: &str) -> HostContext {
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
    let (_run, requirements) = begin(&host, HostServices::new(), BTreeMap::new(), &declares);
    assert!(requirements.is_satisfied(), "{requirements:?}");
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
fn a_bare_entry_that_only_another_vendor_has_is_refused_naming_each_fqn() {
    let one = vendors("ready");
    let (_run, requirements) = begin(
        &one,
        HostServices::new(),
        BTreeMap::new(),
        &prompt("uses:\n  - poop\n"),
    );
    assert_eq!(
        requirements.unavailable,
        [UnavailablePack::new(
            name("poop"),
            "poop means runvil/poop, which is not installed; write acme/poop to use that Pack"
        )]
    );
    assert!(requirements.missing_required.is_empty());

    let both = host(&[
        (info("zed/poop"), Value::Null),
        (info("acme/poop"), Value::Null),
    ]);
    let (_run, requirements) = begin(
        &both,
        HostServices::new(),
        BTreeMap::new(),
        &prompt("uses:\n  - poop\n"),
    );
    assert_eq!(
        requirements.unavailable,
        [UnavailablePack::new(
            name("poop"),
            "poop means runvil/poop, which is not installed; \
             write acme/poop or zed/poop to use that Pack"
        )]
    );
}

#[test]
fn an_entry_no_vendor_has_is_missing_and_an_absent_vendor_name_is_unavailable() {
    let host = vendors("ready");
    let (_run, requirements) = begin(
        &host,
        HostServices::new(),
        BTreeMap::new(),
        &prompt("uses:\n  - nope\n  - acme/gone\n"),
    );
    assert_eq!(requirements.missing_required, [name("nope")]);
    assert_eq!(
        requirements.unavailable,
        [UnavailablePack::new(
            name("gone"),
            "acme/gone is not installed"
        )]
    );
    let notice = requirements.notice();
    assert!(notice.contains("- missing required Pack: nope"), "{notice}");
    assert!(
        notice.contains("- gone is unavailable: acme/gone is not installed"),
        "{notice}"
    );
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
    let (_run, requirements) =
        host.begin_run(&packs, HostServices::new(), failures, &declares_acme);
    assert_eq!(
        requirements.unavailable,
        [UnavailablePack::new(name("web"), "web will not be ready")]
    );

    let declares_runvil = prompt("uses:\n  - web\n");
    let packs = host.pick(&declares_runvil);
    let failures = host.wait_ready(&packs, &cancel).now_or_never().unwrap();
    assert!(failures.is_empty(), "{failures:?}");
    let (run, requirements) =
        host.begin_run(&packs, HostServices::new(), failures, &declares_runvil);
    assert!(requirements.is_satisfied(), "{requirements:?}");
    let ids: Vec<String> = run
        .catalog()
        .tools()
        .iter()
        .map(|tool| tool.id.to_string())
        .collect();
    assert_eq!(ids, ["web/runvil"], "runvil/web still serves the run");
}
