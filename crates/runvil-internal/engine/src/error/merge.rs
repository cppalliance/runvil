//! The rules of the merge that builds a section entry's frozen sets, and
//! the message [`Error::ToolMerge`](super::Error::ToolMerge) renders when
//! the sets break one.

use std::fmt;

use runvil_types::tools::ToolId;

/// One of the two sets a section entry freezes at its first model call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrozenSet {
    /// The Static Set, which every round of the entry advertises.
    Static,
    /// The Dynamic Set, which no round advertises.
    Dynamic,
}

impl fmt::Display for FrozenSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            FrozenSet::Static => "Static Set",
            FrozenSet::Dynamic => "Dynamic Set",
        })
    }
}

/// A merge rule a section entry's frozen sets broke.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MergeRule {
    /// A wire name names one tool across both sets; this one names two.
    UniqueWireName(String),
}

/// One tool that broke a merge rule: its id, whose first segment names its
/// Pack, and the set it sits in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MergeTool {
    /// The tool's id.
    pub(crate) id: ToolId,
    /// The set the tool sits in.
    pub(crate) set: FrozenSet,
}

/// The message for `tools` breaking `rule`, written for a model: each tool
/// with its Pack and its set, and what the rule requires.
pub(super) fn merge_message(rule: &MergeRule, tools: &[MergeTool]) -> String {
    let tools = tools
        .iter()
        .map(|tool| {
            format!(
                "{} of the {} Pack in the {}",
                tool.id,
                tool.id.pack(),
                tool.set
            )
        })
        .collect::<Vec<_>>()
        .join(" and ");
    match rule {
        MergeRule::UniqueWireName(wire_name) => format!(
            "the tools {tools} share the wire name {wire_name:?}, and a wire name names one tool \
             across a section's Static Set and Dynamic Set; offer only one of them"
        ),
    }
}
