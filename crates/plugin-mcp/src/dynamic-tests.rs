//! Tests for the Pack's two static tools.

use std::collections::BTreeMap;

use runvil_plugin::{Pack, PackId};
use tokio::runtime::Handle;

use super::Mcp;

#[tokio::test]
async fn only_the_listing_tool_describes_tools() {
    let name = PackId::parse("mcp").expect("a valid Pack id");
    let mcp = Mcp::new(&name, BTreeMap::new(), Handle::current())
        .expect("the Pack names its tools under a valid Pack id");
    let flags: Vec<(String, bool)> = mcp
        .tools()
        .iter()
        .map(|tool| (tool.id.to_string(), tool.describes_tools))
        .collect();
    assert_eq!(
        flags,
        [
            ("mcp/get_dynamic_tools".to_owned(), true),
            ("mcp/call_dynamic_tool".to_owned(), false),
        ]
    );
}
