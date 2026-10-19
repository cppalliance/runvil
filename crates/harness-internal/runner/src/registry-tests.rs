//! Tests for the built-ins every registry holds: each builds a Plugin that
//! offers no tools, and carries no prelude and no needs.

use runvil_plugin::HostServices;
use serde_json::Value;

use super::{BUILTIN, default_id};

#[test]
fn builtins_offer_no_tools_prelude_or_needs() {
    for info in BUILTIN {
        assert!(info.prelude.is_none(), "{} has a prelude", info.fqn);
        assert!(info.needs.is_empty(), "{} has needs", info.fqn);
        let name = default_id(info.fqn).unwrap();
        let plugin = (info.construct)(&name, Value::Null, &HostServices::new()).unwrap();
        assert!(plugin.tools().is_empty(), "{} offers tools", info.fqn);
    }
}
