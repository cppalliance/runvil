//! Tests for the Pack registry: a new registry holds the built-in
//! `runvil/subagents` and lists every Pack in FQN order, `insert` refuses an
//! FQN that is not a vendor/name pair, an FQN the registry holds already,
//! a reserved Pack id from any vendor, and a third-party Pack taking the
//! built-in's Pack id, and accepts a Pack id another vendor has; the
//! registry keeps each Pack's config and hands it on with the Pack id, its
//! Debug output never shows a config, and each built-in takes no reserved
//! Pack id and builds a Pack that offers no tools and carries no prelude
//! and no needs.

use runvil_types::error::{Error, ErrorCode, ErrorFact};
use serde_json::{Value, json};

use super::{BUILTIN, PackOrigin, PackRegistry, builtin, pack_id};
use crate::{HostServices, PackId, PackInfo, ServiceId, ServiceKey};

/// Asserts `result` refuses the insert of `fqn` for `reason`, with its
/// message as its prose, and returns the message.
#[track_caller]
fn refused(result: Result<(), Error>, reason: &str, fqn: &str) -> String {
    let error = result.expect_err("the insert must be refused");
    assert_eq!(error.code, ErrorCode::InvalidValue, "{error:?}");
    assert_eq!(
        error.facts.get(&ErrorFact::Reason),
        Some(&Value::from(reason)),
        "{error:?}"
    );
    assert_eq!(
        error.facts.get(&ErrorFact::Name),
        Some(&Value::from(fqn)),
        "{error:?}"
    );
    assert_eq!(error.prose, error.message, "{error:?}");
    error.message
}

/// The built-in's FQN.
const SUBAGENTS: &str = "runvil/subagents";

/// The test-only service a listed Pack needs.
const GREETING: ServiceKey<str> = ServiceKey::new("tests/greeting");

const NEEDS: &[ServiceId] = &[GREETING.id()];

const NO_NEEDS: &[ServiceId] = &[];

/// A Pack with the FQN `fqn` whose object offers nothing.
const fn fixture(fqn: &'static str) -> PackInfo {
    PackInfo::new(fqn, builtin)
}

fn name(text: &str) -> PackId {
    PackId::parse(text).unwrap()
}

#[test]
fn a_new_registry_holds_the_builtin_and_lists_every_pack_in_fqn_order() {
    let mut packs = PackRegistry::new();
    packs
        .insert(fixture("tests/zeta").needs(NEEDS), Value::Null)
        .unwrap();
    packs.insert(fixture("acme/alpha"), Value::Null).unwrap();
    let listed: Vec<(&str, PackOrigin, &[ServiceId])> = packs
        .list()
        .map(|(info, origin)| (info.fqn, origin, info.needs))
        .collect();
    assert_eq!(
        listed,
        [
            ("acme/alpha", PackOrigin::Linked, NO_NEEDS),
            (SUBAGENTS, PackOrigin::Builtin, NO_NEEDS),
            ("tests/zeta", PackOrigin::Linked, NEEDS),
        ],
        "a Pack inserted with needs lists those needs"
    );
    let (builtin, _origin) = packs.list().nth(1).unwrap();
    assert!(
        !builtin.description.is_empty(),
        "a Host listing Packs can say what the built-in does"
    );
}

#[test]
fn add_refuses_an_fqn_that_is_not_a_vendor_name_pair() {
    for bad in [
        "web",
        "acme/web/extra",
        "/web",
        "acme/",
        "Acme/web",
        "acme/Web",
    ] {
        refused(
            PackRegistry::new().insert(fixture(bad), Value::Null),
            "invalid_fqn",
            bad,
        );
    }
}

#[test]
fn add_refuses_an_fqn_the_registry_holds_already_the_builtins_included() {
    let mut packs = PackRegistry::new();
    packs.insert(fixture("tests/tools"), Value::Null).unwrap();
    for taken in ["tests/tools", SUBAGENTS] {
        refused(
            packs.insert(fixture(taken), Value::Null),
            "duplicate_pack",
            taken,
        );
    }
    assert!(
        packs
            .list()
            .any(|(info, origin)| info.fqn == SUBAGENTS && origin == PackOrigin::Builtin),
        "the refused insert left the built-in in place"
    );
}

#[test]
fn add_refuses_a_third_party_pack_taking_the_builtins_pack_id() {
    let mut packs = PackRegistry::new();
    refused(
        packs.insert(fixture("acme/subagents"), Value::Null),
        "reserved_name",
        "acme/subagents",
    );
    assert!(
        !packs.list().any(|(info, _)| info.fqn == "acme/subagents"),
        "the refused Pack is not in the registry"
    );
}

#[test]
fn add_refuses_a_pack_taking_a_reserved_pack_id_from_any_vendor() {
    for (fqn, reserved) in [
        ("acme/adhoc", "adhoc"),
        ("runvil/adhoc", "adhoc"),
        ("acme/dynamic", "dynamic"),
        ("runvil/dynamic", "dynamic"),
    ] {
        let mut packs = PackRegistry::new();
        let message = refused(
            packs.insert(fixture(fqn), Value::Null),
            "reserved_pack_id",
            fqn,
        );
        assert!(
            message.contains(&format!("the Pack id {reserved},")),
            "{message}"
        );
        assert!(
            !packs.list().any(|(info, _)| info.fqn == fqn),
            "the refused {fqn} is not in the registry"
        );
    }
}

#[test]
fn add_accepts_a_pack_whose_id_another_vendor_has() {
    let mut packs = PackRegistry::new();
    packs.insert(fixture("runvil/web"), Value::Null).unwrap();
    assert_eq!(packs.insert(fixture("acme/web"), Value::Null), Ok(()));
    let ids: Vec<(&'static str, String)> = packs
        .into_entries()
        .map(|(info, id, _config)| (info.fqn, id.to_string()))
        .collect();
    assert_eq!(
        ids,
        [
            ("acme/web", "web".to_owned()),
            (SUBAGENTS, "subagents".to_owned()),
            ("runvil/web", "web".to_owned()),
        ],
        "each Pack id is its FQN's second segment, so two vendors share one"
    );
}

#[test]
fn the_registry_keeps_each_packs_config_and_hands_it_on_with_its_pack_id() {
    let mut packs = PackRegistry::new();
    packs
        .insert(fixture("acme/kit"), json!({ "depth": 2 }))
        .unwrap();
    let entries: Vec<(&'static str, PackId, Value)> = packs
        .into_entries()
        .map(|(info, id, config)| (info.fqn, id, config))
        .collect();
    assert_eq!(
        entries,
        [
            ("acme/kit", name("kit"), json!({ "depth": 2 })),
            (SUBAGENTS, name("subagents"), Value::Null),
        ]
    );
}

#[test]
fn the_registrys_debug_output_shows_each_fqn_origin_and_pack_id_and_no_config() {
    let mut packs = PackRegistry::new();
    let secret = "Bearer ghp_secret_token_value_0042";
    packs
        .insert(
            fixture("acme/mcp"),
            json!({ "docs": { "headers": { "Authorization": secret } } }),
        )
        .unwrap();
    let shown = format!("{packs:?}");
    for part in ["acme/mcp", "Linked", SUBAGENTS, "Builtin"] {
        assert!(shown.contains(part), "{part} is shown: {shown}");
    }
    for id in [name("mcp"), name("subagents")] {
        let id = format!("{id:?}");
        assert!(shown.contains(&id), "{id} is shown: {shown}");
    }
    assert!(!shown.contains(secret), "no config is shown: {shown}");
    assert!(
        !shown.contains("Authorization"),
        "no config key is shown: {shown}"
    );
}

#[test]
fn the_registry_refusals_name_the_pack() {
    let mut packs = PackRegistry::new();
    packs.insert(fixture("acme/web"), Value::Null).unwrap();
    let mut insert = |fqn| packs.insert(fixture(fqn), Value::Null);
    let messages = [
        refused(insert("web"), "invalid_fqn", "web"),
        refused(insert("acme/web"), "duplicate_pack", "acme/web"),
        refused(insert("acme/subagents"), "reserved_name", "acme/subagents"),
        refused(insert("acme/adhoc"), "reserved_pack_id", "acme/adhoc"),
        refused(insert("acme/dynamic"), "reserved_pack_id", "acme/dynamic"),
    ];
    assert_eq!(
        messages,
        [
            "Pack fully qualified name `web` is not a vendor/name pair",
            "Pack `acme/web` is already in the registry",
            "Pack `acme/subagents` cannot take the Pack id subagents, \
             which the built-in runvil/subagents reserves",
            "Pack `acme/adhoc` cannot take the Pack id adhoc, which the Engine reserves",
            "Pack `acme/dynamic` cannot take the Pack id dynamic, which the Engine reserves",
        ]
    );
}

#[test]
fn no_builtin_takes_a_reserved_pack_id() {
    for info in BUILTIN {
        let id = pack_id(info.fqn).unwrap();
        assert!(!id.is_reserved(), "{} takes a reserved Pack id", info.fqn);
    }
}

#[test]
fn builtins_offer_no_tools_prelude_or_needs() {
    for info in BUILTIN {
        assert!(info.prelude.is_none(), "{} has a prelude", info.fqn);
        assert!(info.needs.is_empty(), "{} has needs", info.fqn);
        let name = pack_id(info.fqn).unwrap();
        let pack = (info.construct)(&name, Value::Null, &HostServices::new()).unwrap();
        assert!(pack.tools().is_empty(), "{} offers tools", info.fqn);
    }
}
