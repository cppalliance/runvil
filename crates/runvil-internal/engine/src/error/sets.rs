//! The rules of the merge that builds a section entry's frozen sets, and
//! the message [`Error::ToolSets`](super::Error::ToolSets) renders when
//! the sets break one.

use runvil_lua::OfferMode;
use runvil_types::packs::PackId;
use runvil_types::tools::ToolId;

/// A merge rule a section entry's frozen sets broke.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SetRule {
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
pub(crate) struct SetTool {
    /// The tool's id.
    pub(crate) id: ToolId,
    /// The set the tool sits in.
    pub(crate) set: OfferMode,
}

/// `packs` as a bracketed list, such as `[web, gh]`.
fn pack_list(packs: &[PackId]) -> String {
    let names: Vec<String> = packs.iter().map(ToString::to_string).collect();
    format!("[{}]", names.join(", "))
}

/// The message for `tools` breaking `rule`, written for a model: each tool
/// with its Pack and its set, and what the rule requires.
pub(super) fn sets_message(rule: &SetRule, tools: &[SetTool]) -> String {
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
        SetRule::RenderedPacks { asked, answered } => format!(
            "the render answered the Packs {} where the section offered the Packs {}; a render \
             answers each offered Pack once, in offer order",
            pack_list(answered),
            pack_list(asked)
        ),
        SetRule::UnofferedSet(pack) => format!(
            "the {pack} Pack's render returned {tools}, a set the section did not offer the \
             {pack} Pack into; a render returns tools only for the sets the section offered its \
             Pack into"
        ),
        SetRule::ForeignTool(pack) => format!(
            "the {pack} Pack's render returned {tools}; a Pack renders only tools whose id starts \
             with its own Pack id"
        ),
        SetRule::UnofferedCatalogTool => format!(
            "the render returned {tools}, a run catalog tool the section did not offer in that \
             set; a render returns a catalog tool only into a set the section offered it in"
        ),
        SetRule::UniqueWireName(wire_name) => format!(
            "the tools {tools} share the wire name {wire_name:?}, and a wire name names one tool \
             across a section's Static Set and Dynamic Set; offer only one of them"
        ),
    }
}
