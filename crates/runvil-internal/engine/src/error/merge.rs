//! The rules of the merge that builds a section entry's frozen sets, and
//! the message [`Error::ToolMerge`](super::Error::ToolMerge) renders when
//! the sets break one.

use std::fmt;

use runvil_types::packs::PackId;
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
    /// A render answers each Pack it was asked about once, in the order
    /// asked; this one answered other Packs.
    RenderedPacks {
        /// The Packs the render was asked about, in order.
        asked: Vec<PackId>,
        /// The Packs it answered, in order.
        answered: Vec<PackId>,
    },
    /// A render returns tools only for the sets the section offered a Pack
    /// into; this Pack's render returned some for another set.
    UnofferedSet(PackId),
    /// A Pack renders only tools under its own id; this Pack rendered
    /// another Pack's tool.
    ForeignTool(PackId),
    /// A render returns a run catalog tool only into a set the section
    /// offered it in; this one returned it into another set.
    UnofferedCatalogTool,
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

/// `packs` as a bracketed list, such as `[web, gh]`.
fn pack_list(packs: &[PackId]) -> String {
    let names: Vec<String> = packs.iter().map(ToString::to_string).collect();
    format!("[{}]", names.join(", "))
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
        MergeRule::RenderedPacks { asked, answered } => format!(
            "the render answered the Packs {} where the section offered the Packs {}; a render \
             answers each offered Pack once, in offer order",
            pack_list(answered),
            pack_list(asked)
        ),
        MergeRule::UnofferedSet(pack) => format!(
            "the {pack} Pack's render returned {tools}, a set the section did not offer the \
             {pack} Pack into; a render returns tools only for the sets the section offered its \
             Pack into"
        ),
        MergeRule::ForeignTool(pack) => format!(
            "the {pack} Pack's render returned {tools}; a Pack renders only tools whose id starts \
             with its own Pack id"
        ),
        MergeRule::UnofferedCatalogTool => format!(
            "the render returned {tools}, a run catalog tool the section did not offer in that \
             set; a render returns a catalog tool only into a set the section offered it in"
        ),
        MergeRule::UniqueWireName(wire_name) => format!(
            "the tools {tools} share the wire name {wire_name:?}, and a wire name names one tool \
             across a section's Static Set and Dynamic Set; offer only one of them"
        ),
    }
}
