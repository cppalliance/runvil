//! Tests for the scope types: the discovery tools' ids stay under a reserved
//! Pack id, so no vendor Pack can take the discovery namespace, and a count
//! increment seeds the id it was handed.

use runvil_types::tools::ToolId;

use super::{DiscoveryTool, ToolCallCounts};

#[test]
fn increment_seeds_a_missing_id_and_counts_it() {
    let counts = ToolCallCounts::default();
    counts
        .increment("std/read")
        .expect("increment seeds an id it was never given");
    let snapshot = counts.snapshot().expect("the counts mutex is not poisoned");
    assert_eq!(snapshot.get("std/read"), Some(&1));
}

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
