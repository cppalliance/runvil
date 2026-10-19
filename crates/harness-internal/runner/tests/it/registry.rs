//! The Plugin registry and installs by FQN: a new registry holds the
//! built-in `runvil/subagents` and lists every Plugin in FQN order, `add`
//! refuses an FQN that is not a vendor/name pair and an FQN the registry
//! holds already, and install refuses a Plugin the registry does not
//! hold, keeps the built-in's Plugin id to the built-in, and refuses the
//! built-in's second install as a taken Plugin id.

use harness_runner::{HostContext, InstallError, PluginKind, PluginRegistry};
use runvil_plugin::{HostServices, PluginId, ServiceId, ServiceKey};
use serde_json::Value;

use crate::prepare::echo_plugin;
use crate::support::registry;

/// The built-in's FQN.
const SUBAGENTS: &str = "runvil/subagents";

/// The test-only service a listed Plugin needs.
const GREETING: ServiceKey<str> = ServiceKey::new("tests/greeting");

const NEEDS: &[ServiceId] = &[GREETING.id()];

const NO_NEEDS: &[ServiceId] = &[];

fn name(text: &str) -> PluginId {
    PluginId::parse(text).unwrap()
}

#[test]
fn a_new_registry_holds_the_builtin_and_lists_every_plugin_in_fqn_order() {
    let mut plugins = PluginRegistry::new();
    plugins
        .add(echo_plugin("tests/zeta", None).needs(NEEDS))
        .unwrap();
    plugins.add(echo_plugin("acme/alpha", None)).unwrap();
    let listed: Vec<(&str, PluginKind, &[ServiceId])> = plugins
        .list()
        .map(|(info, kind)| (info.fqn, kind, info.needs))
        .collect();
    assert_eq!(
        listed,
        [
            ("acme/alpha", PluginKind::Linked, NO_NEEDS),
            (SUBAGENTS, PluginKind::Builtin, NO_NEEDS),
            ("tests/zeta", PluginKind::Linked, NEEDS),
        ],
        "a Plugin added with needs lists those needs"
    );
    let (builtin, _kind) = plugins.list().nth(1).unwrap();
    assert!(
        !builtin.description.is_empty(),
        "a Host listing Plugins can say what the built-in does"
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
            PluginRegistry::new().add(echo_plugin(bad, None)),
            Err(InstallError::InvalidFqn { fqn: bad }),
            "{bad}"
        );
    }
}

#[test]
fn add_refuses_an_fqn_the_registry_holds_already_the_builtins_included() {
    let mut plugins = registry(&[echo_plugin("tests/tools", None)]);
    for taken in ["tests/tools", SUBAGENTS] {
        assert_eq!(
            plugins.add(echo_plugin(taken, None)),
            Err(InstallError::DuplicatePlugin { fqn: taken }),
            "{taken}"
        );
    }
    assert!(
        plugins
            .list()
            .any(|(info, kind)| info.fqn == SUBAGENTS && kind == PluginKind::Builtin),
        "the refused add left the built-in in place"
    );
}

#[test]
fn install_refuses_a_plugin_the_registry_does_not_hold() {
    let mut host = HostContext::new(
        registry(&[echo_plugin("tests/tools", None)]),
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
        message.contains("tests/other") && message.contains("PluginRegistry::add"),
        "the refusal names the Plugin and how to add it: {message}"
    );
}

#[test]
fn install_keeps_the_builtins_plugin_id_to_the_builtin_alone() {
    let mut host = HostContext::new(
        registry(&[
            echo_plugin("tests/tools", None),
            echo_plugin("acme/subagents", None),
        ]),
        HostServices::new(),
    );
    assert_eq!(
        host.install(SUBAGENTS, Some(name("helpers")), Value::Null),
        Err(InstallError::ReservedName {
            fqn: SUBAGENTS,
            name: name("helpers"),
        }),
        "the built-in keeps its own Plugin id"
    );
    assert_eq!(
        host.install("tests/tools", Some(name("subagents")), Value::Null),
        Err(InstallError::ReservedName {
            fqn: "tests/tools",
            name: name("subagents"),
        }),
        "a linked Plugin may not pick the built-in's Plugin id"
    );
    let refused = host.install("acme/subagents", None, Value::Null);
    assert_eq!(
        refused,
        Err(InstallError::ReservedName {
            fqn: "acme/subagents",
            name: name("subagents"),
        }),
        "a linked Plugin may not take the built-in's Plugin id by default"
    );
    assert_eq!(
        refused.unwrap_err().to_string(),
        "Plugin `acme/subagents` cannot be installed as subagents: \
         a built-in Plugin keeps its own Plugin id, and no other Plugin may take it"
    );
    assert_eq!(
        host.install(SUBAGENTS, Some(name("subagents")), Value::Null),
        Ok(name("subagents")),
        "the built-in installs under its own Plugin id"
    );
}

#[test]
fn installing_the_builtin_twice_is_a_taken_plugin_id() {
    let mut host = HostContext::new(PluginRegistry::new(), HostServices::new());
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
