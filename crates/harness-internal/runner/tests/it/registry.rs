//! The Pack registry and installs by FQN: a new registry holds the
//! built-in `runvil/subagents` and lists every Pack in FQN order, `add`
//! refuses an FQN that is not a vendor/name pair and an FQN the registry
//! holds already, and install refuses a Pack the registry does not
//! hold, keeps the built-in's Pack id to the built-in, and refuses the
//! built-in's second install as a taken Pack id.

use harness_runner::{HostContext, InstallError, PackOrigin, PackRegistry};
use runvil_plugin::{HostServices, PackId, ServiceId, ServiceKey};
use serde_json::Value;

use crate::prepare::echo_pack;
use crate::support::registry;

/// The built-in's FQN.
const SUBAGENTS: &str = "runvil/subagents";

/// The test-only service a listed Pack needs.
const GREETING: ServiceKey<str> = ServiceKey::new("tests/greeting");

const NEEDS: &[ServiceId] = &[GREETING.id()];

const NO_NEEDS: &[ServiceId] = &[];

fn name(text: &str) -> PackId {
    PackId::parse(text).unwrap()
}

#[test]
fn a_new_registry_holds_the_builtin_and_lists_every_pack_in_fqn_order() {
    let mut packs = PackRegistry::new();
    packs
        .add(echo_pack("tests/zeta", None).needs(NEEDS))
        .unwrap();
    packs.add(echo_pack("acme/alpha", None)).unwrap();
    let listed: Vec<(&str, PackOrigin, &[ServiceId])> = packs
        .list()
        .map(|(info, kind)| (info.fqn, kind, info.needs))
        .collect();
    assert_eq!(
        listed,
        [
            ("acme/alpha", PackOrigin::Linked, NO_NEEDS),
            (SUBAGENTS, PackOrigin::Builtin, NO_NEEDS),
            ("tests/zeta", PackOrigin::Linked, NEEDS),
        ],
        "a Pack added with needs lists those needs"
    );
    let (builtin, _kind) = packs.list().nth(1).unwrap();
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
        assert_eq!(
            PackRegistry::new().add(echo_pack(bad, None)),
            Err(InstallError::InvalidFqn { fqn: bad }),
            "{bad}"
        );
    }
}

#[test]
fn add_refuses_an_fqn_the_registry_holds_already_the_builtins_included() {
    let mut packs = registry(&[echo_pack("tests/tools", None)]);
    for taken in ["tests/tools", SUBAGENTS] {
        assert_eq!(
            packs.add(echo_pack(taken, None)),
            Err(InstallError::DuplicatePack { fqn: taken }),
            "{taken}"
        );
    }
    assert!(
        packs
            .list()
            .any(|(info, kind)| info.fqn == SUBAGENTS && kind == PackOrigin::Builtin),
        "the refused add left the built-in in place"
    );
}

#[test]
fn install_refuses_a_pack_the_registry_does_not_hold() {
    let mut host = HostContext::new(
        registry(&[echo_pack("tests/tools", None)]),
        HostServices::new(),
    );
    for missing in ["tests/other", "runvil/tasks"] {
        assert_eq!(
            host.install(missing, None, Value::Null),
            Err(InstallError::UnknownPlugin {
                fqn: missing.to_owned()
            }),
            "{missing}"
        );
    }
    let message = host
        .install("tests/other", None, Value::Null)
        .unwrap_err()
        .to_string();
    assert!(
        message.contains("tests/other") && message.contains("PackRegistry::add"),
        "the refusal names the Pack and how to add it: {message}"
    );
}

#[test]
fn install_keeps_the_builtins_pack_id_to_the_builtin_alone() {
    let mut host = HostContext::new(
        registry(&[
            echo_pack("tests/tools", None),
            echo_pack("acme/subagents", None),
        ]),
        HostServices::new(),
    );
    assert_eq!(
        host.install(SUBAGENTS, Some(name("helpers")), Value::Null),
        Err(InstallError::ReservedName {
            fqn: SUBAGENTS,
            name: name("helpers"),
        }),
        "the built-in keeps its own Pack id"
    );
    assert_eq!(
        host.install("tests/tools", Some(name("subagents")), Value::Null),
        Err(InstallError::ReservedName {
            fqn: "tests/tools",
            name: name("subagents"),
        }),
        "a linked Pack may not pick the built-in's Pack id"
    );
    let refused = host.install("acme/subagents", None, Value::Null);
    assert_eq!(
        refused,
        Err(InstallError::ReservedName {
            fqn: "acme/subagents",
            name: name("subagents"),
        }),
        "a linked Pack may not take the built-in's Pack id by default"
    );
    assert_eq!(
        refused.unwrap_err().to_string(),
        "Pack `acme/subagents` cannot be installed as subagents: \
         a built-in Pack keeps its own Pack id, and no other Pack may take it"
    );
    assert_eq!(
        host.install(SUBAGENTS, Some(name("subagents")), Value::Null),
        Ok(name("subagents")),
        "the built-in installs under its own Pack id"
    );
}

#[test]
fn installing_the_builtin_twice_is_a_taken_pack_id() {
    let mut host = HostContext::new(PackRegistry::new(), HostServices::new());
    assert_eq!(
        host.install(SUBAGENTS, None, Value::Null),
        Ok(name("subagents"))
    );
    assert_eq!(
        host.install(SUBAGENTS, None, Value::Null),
        Err(InstallError::NameTaken {
            name: name("subagents"),
            existing: name("subagents"),
        })
    );
}
