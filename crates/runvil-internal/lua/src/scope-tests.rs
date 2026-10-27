//! Tests for the scope types: the discovery tools' ids stay under a reserved
//! Pack id, so no vendor Pack can take the discovery namespace.

use runvil_types::tools::ToolId;

use super::DiscoveryTool;

#[test]
fn every_discovery_tool_id_sits_under_a_reserved_pack() {
    for tool in DiscoveryTool::ALL {
        let id = ToolId::parse(tool.id()).expect("a discovery tool id is a valid tool id");
        assert!(
            id.pack().is_reserved(),
            "{} is not under a reserved Pack id",
            tool.id()
        );
    }
}
