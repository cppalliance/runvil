//! A section entry's frozen sets: the merge that builds them at the
//! entry's first model call, from each catalog Pack's tools per set and
//! the section's adhoc tools, plus the Engine's two discovery tools beside
//! a Dynamic Set, inside the scope validation boundary pair, and the
//! schema a round advertises each Static Set tool under.

use std::collections::BTreeMap;

use runvil_model_client::detail;
use runvil_types::emitter::Emitter;
use runvil_types::event::lifecycle;
use runvil_types::packs::PackId;
use runvil_types::tools::{ToolCatalog, ToolId, ToolInfo};
use serde_json::{Value, json};

use crate::error::{FrozenSet, MergeRule, MergeTool};
use crate::lua::{DiscoveryTool, OfferedTool, RenderedTools, ToolBinding, wire_name_of};
use crate::model::ToolSchema;
use crate::{Error, Result};

/// The most Dynamic Set wire names the search tool's description lists.
const LISTED_NAMES: usize = 100;

/// What the search tool does, before the Dynamic Set's names.
const SEARCH_USAGE: &str = "Lists and searches the dynamic tools, which CallDynamicTool calls: \
                            pass toolName for one tool's description and input schema, pattern \
                            for a regex search of their names, or neither to list them all";

/// What the call tool does.
const CALL_USAGE: &str = "Calls a dynamic tool: pass its name from GetDynamicTools as toolName \
                          and its arguments as arguments.";

/// One catalog Pack's tools for each set of a section entry, in the shape
/// a render answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PackSets {
    /// The Pack.
    pub(super) pack: PackId,
    /// Its tools for the Static Set, in order.
    pub(super) static_set: Vec<ToolInfo>,
    /// Its tools for the Dynamic Set, in order.
    pub(super) dynamic_set: Vec<ToolInfo>,
}

impl PackSets {
    /// The Pack's tools for `set`.
    fn set_mut(&mut self, set: FrozenSet) -> &mut Vec<ToolInfo> {
        match set {
            FrozenSet::Static => &mut self.static_set,
            FrozenSet::Dynamic => &mut self.dynamic_set,
        }
    }
}

/// Each catalog Pack the offers name, once, in the order the Static Set's
/// offers first name it and then the Dynamic Set's, with the catalog tools
/// offered from it into each set, in offer order.
pub(super) fn offered_packs(
    static_tools: &[OfferedTool],
    dynamic_tools: &[OfferedTool],
) -> Vec<PackSets> {
    let mut packs: Vec<PackSets> = Vec::new();
    for (set, tools) in [
        (FrozenSet::Static, static_tools),
        (FrozenSet::Dynamic, dynamic_tools),
    ] {
        for tool in tools {
            let OfferedTool::Catalog(info) = tool else {
                continue;
            };
            let pack = info.id.pack();
            let index = if let Some(index) = packs.iter().position(|held| held.pack == pack) {
                index
            } else {
                packs.push(PackSets {
                    pack,
                    static_set: Vec::new(),
                    dynamic_set: Vec::new(),
                });
                packs.len() - 1
            };
            packs[index].set_mut(set).push(info.clone());
        }
    }
    packs
}

/// Builds a section entry's frozen sets with [`merge_tool_sets`] inside
/// the validation boundary pair, reported through `emitter` under
/// `section`.
///
/// # Errors
/// Returns the merge's error.
pub(super) fn prepare_effective_scope(
    static_tools: &[OfferedTool],
    dynamic_tools: &[OfferedTool],
    rendered: Vec<PackSets>,
    catalog: &ToolCatalog,
    emitter: &Emitter,
    section: &str,
) -> Result<RenderedTools> {
    emitter.report(section, lifecycle::TOOL_SCOPE_VALIDATION_STARTED);
    let result = merge_tool_sets(static_tools, dynamic_tools, rendered, catalog);
    emitter.report(
        section,
        if result.is_ok() {
            lifecycle::TOOL_SCOPE_VALIDATION_SUCCEEDED
        } else {
            lifecycle::TOOL_SCOPE_VALIDATION_FAILED
        },
    );
    result
}

/// One tool laid into a set before the rules run: its binding, and whether
/// it is an adhoc tool.
struct Laid {
    binding: ToolBinding,
    adhoc: bool,
}

/// Merges a section entry's offers into its frozen sets.
///
/// `static_tools` and `dynamic_tools` are each set's offers in offer
/// order, and `rendered` is each catalog Pack's tools per set, as a render
/// answers them. Each set keeps offer order: a Pack's rendered tools sit
/// as one block where that Pack's first tool was offered into the set, in
/// the order they arrive, and an adhoc tool keeps its own place. Then the
/// rules run, in this order:
///
/// - Each tool binds under its wire name, and a catalog id keeps the
///   catalog's output kind and listing flag.
/// - Within a set, a Pack tool whose wire name an adhoc tool of that set
///   takes is dropped.
/// - A Dynamic Set tool whose id the Static Set holds is dropped.
/// - When the Dynamic Set still holds a tool, the two discovery tools
///   join the end of the Static Set.
/// - A wire name used twice across both sets fails the merge.
/// - Every tool must have a valid wire name and an object parameter
///   schema.
///
/// # Errors
/// Returns [`Error::ToolMerge`] naming both tools of a wire-name clash, or
/// [`Error::BindSchema`] for a tool no round could advertise.
pub(super) fn merge_tool_sets(
    static_tools: &[OfferedTool],
    dynamic_tools: &[OfferedTool],
    mut rendered: Vec<PackSets>,
    catalog: &ToolCatalog,
) -> Result<RenderedTools> {
    let mut static_set = lay_out(static_tools, FrozenSet::Static, &mut rendered, catalog);
    let mut dynamic_set = lay_out(dynamic_tools, FrozenSet::Dynamic, &mut rendered, catalog);
    drop_shadowed(&mut static_set);
    drop_shadowed(&mut dynamic_set);
    dynamic_set.retain(|tool| {
        !static_set
            .iter()
            .any(|held| held.binding.id() == tool.binding.id())
    });
    if !dynamic_set.is_empty() {
        static_set.extend(discovery_tools(&dynamic_set)?);
    }
    refuse_clash(&static_set, &dynamic_set)?;
    for tool in static_set.iter().chain(&dynamic_set) {
        tool_schema(&tool.binding)?;
    }
    let bindings = |set: Vec<Laid>| set.into_iter().map(|tool| tool.binding).collect();
    Ok(RenderedTools {
        static_set: bindings(static_set),
        dynamic_set: bindings(dynamic_set),
    })
}

/// Lays `offers` out as `set`: each adhoc tool at its place, and each
/// Pack's rendered tools for `set` where that Pack's first tool sits.
fn lay_out(
    offers: &[OfferedTool],
    set: FrozenSet,
    rendered: &mut [PackSets],
    catalog: &ToolCatalog,
) -> Vec<Laid> {
    let mut laid = Vec::new();
    for offer in offers {
        match offer {
            OfferedTool::Adhoc(binding) => laid.push(Laid {
                binding: binding.clone(),
                adhoc: true,
            }),
            OfferedTool::Catalog(info) => {
                let pack = info.id.pack();
                if let Some(answer) = rendered.iter_mut().find(|answer| answer.pack == pack) {
                    let block = std::mem::take(answer.set_mut(set));
                    laid.extend(block.into_iter().map(|tool| Laid {
                        binding: bind(tool, catalog),
                        adhoc: false,
                    }));
                }
            }
        }
    }
    laid
}

/// Binds a rendered tool under its wire name, with the catalog's output
/// kind and listing flag when its id is a catalog tool's.
fn bind(mut tool: ToolInfo, catalog: &ToolCatalog) -> ToolBinding {
    if let Some(known) = catalog.get(&tool.id) {
        tool.structured_output = known.structured_output;
        tool.describes_tools = known.describes_tools;
    }
    ToolBinding::from_info(&wire_name_of(&tool), &tool)
}

/// Binds both discovery tools, in the order they join the Static Set, for
/// a section entry whose Dynamic Set is `dynamic_set`: the search tool's
/// description ends with the Dynamic Set's wire names, sorted, the first
/// [`LISTED_NAMES`] of them and then how many more there are.
///
/// # Errors
/// Returns [`Error::Internal`] should a discovery tool's id not parse.
fn discovery_tools(dynamic_set: &[Laid]) -> Result<Vec<Laid>> {
    let mut names: Vec<&str> = dynamic_set
        .iter()
        .map(|tool| tool.binding.alias())
        .collect();
    names.sort_unstable();
    let shown = names[..names.len().min(LISTED_NAMES)].join(", ");
    let listed = if names.len() > LISTED_NAMES {
        let more = names.len() - LISTED_NAMES;
        format!("{shown}, and {more} more; call GetDynamicTools to search")
    } else {
        shown
    };
    DiscoveryTool::ALL
        .into_iter()
        .map(|tool| {
            let (description, parameters) = match tool {
                DiscoveryTool::Search => {
                    (format!("{SEARCH_USAGE} ({listed})"), search_parameters())
                }
                DiscoveryTool::Call => (CALL_USAGE.to_owned(), call_parameters()),
            };
            let id = ToolId::parse(tool.id())
                .map_err(|_| Error::internal("a discovery tool's id is a tool id"))?;
            let info = ToolInfo::new(id, description, parameters);
            Ok(Laid {
                binding: ToolBinding::from_info(tool.wire_name(), &info),
                adhoc: false,
            })
        })
        .collect()
}

/// The search tool's parameters, both optional.
fn search_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "toolName": {
                "type": "string",
                "description": "A dynamic tool's name, for its description and input schema."
            },
            "pattern": {
                "type": "string",
                "description": "A regex of at most 256 characters matched against dynamic tool \
                                names; (?i) makes it case-insensitive."
            }
        }
    })
}

/// The call tool's parameters: the target's name, and its arguments.
fn call_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "toolName": {
                "type": "string",
                "description": "The dynamic tool's name, from GetDynamicTools."
            },
            "arguments": {
                "type": "object",
                "description": "The tool's arguments, as its input schema describes them."
            }
        },
        "required": ["toolName"]
    })
}

/// Drops each Pack tool of `set` whose wire name an adhoc tool of `set`
/// takes.
fn drop_shadowed(set: &mut Vec<Laid>) {
    let adhoc: Vec<String> = set
        .iter()
        .filter(|tool| tool.adhoc)
        .map(|tool| tool.binding.alias().to_owned())
        .collect();
    set.retain(|tool| tool.adhoc || !adhoc.iter().any(|name| name == tool.binding.alias()));
}

/// Refuses the first wire name a later tool takes again, across the
/// Static Set and then the Dynamic Set.
fn refuse_clash(static_set: &[Laid], dynamic_set: &[Laid]) -> Result<()> {
    let sited = static_set
        .iter()
        .map(|tool| (tool, FrozenSet::Static))
        .chain(dynamic_set.iter().map(|tool| (tool, FrozenSet::Dynamic)));
    let mut taken: BTreeMap<&str, MergeTool> = BTreeMap::new();
    for (tool, set) in sited {
        let wire_name = tool.binding.alias();
        let site = MergeTool {
            id: tool.binding.id().clone(),
            set,
        };
        if let Some(first) = taken.get(wire_name) {
            return Err(Error::ToolMerge {
                rule: MergeRule::UniqueWireName(wire_name.to_owned()),
                tools: vec![first.clone(), site],
            });
        }
        taken.insert(wire_name, site);
    }
    Ok(())
}

/// The schema a round advertises `binding` under, built through the
/// validated constructor, so a wire name outside
/// `[A-Za-z][A-Za-z0-9_-]{0,63}` or a non-object parameter schema is
/// refused rather than sent to the model.
///
/// # Errors
/// Returns [`Error::BindSchema`] holding the refusal.
pub(super) fn tool_schema(binding: &ToolBinding) -> Result<ToolSchema> {
    detail::tool_schema_new(
        binding.alias().to_owned(),
        binding.description().to_owned(),
        binding.schema().clone(),
    )
    .map_err(|error| Error::BindSchema {
        alias: binding.alias().to_owned(),
        source: Box::new(error),
    })
}

#[cfg(test)]
#[path = "scope-tests.rs"]
mod tests;
