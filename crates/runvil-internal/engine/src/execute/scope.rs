//! A section entry's frozen sets: the section's offers, what the entry's
//! render is asked of them, the merge that builds the sets at the entry's
//! first loop round from each catalog Pack's tools per set, as the render
//! answered them, and the section's adhoc tools, plus the Engine's two
//! discovery tools beside a Dynamic Set, inside the scope validation
//! boundary pair, and the schema a round advertises each Static Set tool
//! under.

use std::collections::BTreeMap;

use runvil_types::emitter::Emitter;
use runvil_types::event::lifecycle;
use runvil_types::packs::PackId;
use runvil_types::tools::{ToolCatalog, ToolId, ToolInfo};
use serde_json::{Value, json};

use crate::error::{SetEvidence, SetRule, SetTool};
use crate::execute::PackToolSets;
use crate::lua::{
    DiscoveryTool, NonEmpty, OfferMode, OfferedTool, RenderedTools, SectionVm, ToolBinding,
    ToolSet, current_catalog_packs, current_dynamic_set, current_tool_bindings, wire_name_of,
};
use crate::model::{ToolSchema, detail};
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

/// One Pack's tools for `set`.
fn set_of(sets: &PackToolSets<ToolInfo>, set: OfferMode) -> &[ToolInfo] {
    match set {
        OfferMode::Static => &sets.static_set,
        OfferMode::Dynamic => &sets.dynamic_set,
    }
}

/// One Pack's tools for `set`, to change.
fn set_mut(sets: &mut PackToolSets<ToolInfo>, set: OfferMode) -> &mut Vec<ToolInfo> {
    match set {
        OfferMode::Static => &mut sets.static_set,
        OfferMode::Dynamic => &mut sets.dynamic_set,
    }
}

/// A section's offers, as its render and its merge read them: the tools
/// each set's offers name, in offer order, and each catalog Pack those
/// tools come from, once, in the order the section first offered one of
/// its tools, into either set.
#[derive(Debug)]
pub(super) struct SectionOffers {
    static_tools: Vec<OfferedTool>,
    dynamic_tools: Vec<OfferedTool>,
    /// Each catalog Pack the offers name, once, in the order the section
    /// first offered one of its tools, into either set. This is the only
    /// copy of that order: [`offered_packs`] takes each Pack's slot from
    /// it, so nothing else may produce or reorder the sequence.
    packs: Vec<PackId>,
}

impl SectionOffers {
    /// Reads the offers `vm`'s section has made, expanded against `set`.
    ///
    /// # Errors
    /// Returns the expansion's error.
    pub(super) fn read(set: &ToolSet, vm: &SectionVm) -> Result<Self> {
        let (runtime, adhoc) = (vm.tool_runtime(), vm.adhoc_tools());
        let static_tools = current_tool_bindings(set, runtime, adhoc)?;
        let dynamic_tools = current_dynamic_set(set, runtime, adhoc, &static_tools)?;
        let packs = current_catalog_packs(set, runtime, adhoc)?;
        Ok(Self {
            static_tools,
            dynamic_tools,
            packs,
        })
    }
}

#[cfg(test)]
impl SectionOffers {
    /// The offers of a section that made every offer of `static_tools`
    /// before any offer of `dynamic_tools`.
    pub(super) fn static_first(
        static_tools: &[OfferedTool],
        dynamic_tools: &[OfferedTool],
    ) -> Self {
        let mut packs: Vec<PackId> = Vec::new();
        for tool in static_tools.iter().chain(dynamic_tools) {
            if let OfferedTool::Catalog(info) = tool
                && !packs.contains(&info.id.pack())
            {
                packs.push(info.id.pack());
            }
        }
        Self {
            static_tools: static_tools.to_vec(),
            dynamic_tools: dynamic_tools.to_vec(),
            packs,
        }
    }
}

/// Each catalog Pack `offers` names in `SectionOffers::packs` order, with
/// the catalog tools offered from it into each set, in offer order: what a
/// render is asked.
pub(super) fn offered_packs(offers: &SectionOffers) -> Vec<PackToolSets<ToolInfo>> {
    let mut packs: Vec<PackToolSets<ToolInfo>> = offers
        .packs
        .iter()
        .map(|pack| PackToolSets::new(pack.clone(), Vec::new(), Vec::new()))
        .collect();
    for (set, tools) in [
        (OfferMode::Static, &offers.static_tools),
        (OfferMode::Dynamic, &offers.dynamic_tools),
    ] {
        for tool in tools {
            let OfferedTool::Catalog(info) = tool else {
                continue;
            };
            if let Some(slot) = packs.iter_mut().find(|held| held.pack == info.id.pack()) {
                set_mut(slot, set).push(info.clone());
            }
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
    offers: &SectionOffers,
    rendered: Vec<PackToolSets<ToolInfo>>,
    catalog: &ToolCatalog,
    emitter: &Emitter,
    section: &str,
) -> Result<RenderedTools> {
    emitter.report(section, lifecycle::TOOL_SCOPE_VALIDATION_STARTED);
    let result = merge_tool_sets(offers, rendered, catalog);
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
/// `offers` holds each set's offers in offer order, and `rendered` is
/// each catalog Pack's tools per set, as a render answers them. Each set
/// keeps offer order: a Pack's rendered tools sit as one block where that
/// Pack's first tool was offered into the set, in the order they arrive,
/// and an adhoc tool keeps its own place. The rules run in this order:
///
/// - The render answered what [`offered_packs`] asks of these offers: the
///   same Packs in the same order, with no tool for a set a Pack was not
///   offered into. Every rendered tool's id starts with its Pack's id, a
///   rendered run catalog tool is one the section offered in that set, and
///   a tool repeated within a set keeps its first copy. Each tool binds
///   under its wire name, and a catalog id keeps the catalog's output
///   kind, listing flag, and stop flag, so a render changes only a catalog
///   tool's wire name, description, and schema, while a model-only tool
///   binds with the flags its render gave it.
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
/// Returns [`Error::ToolSets`] naming the rule and the Packs or tools
/// that broke it, or [`Error::BindSchema`] for a tool no round could
/// advertise.
pub(super) fn merge_tool_sets(
    offers: &SectionOffers,
    rendered: Vec<PackToolSets<ToolInfo>>,
    catalog: &ToolCatalog,
) -> Result<RenderedTools> {
    let asked = offered_packs(offers);
    let mut rendered = check_render(&asked, rendered, catalog)?;
    let (static_tools, dynamic_tools) = (&offers.static_tools, &offers.dynamic_tools);
    let mut static_set = lay_out(static_tools, OfferMode::Static, &mut rendered, catalog);
    let mut dynamic_set = lay_out(dynamic_tools, OfferMode::Dynamic, &mut rendered, catalog);
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

/// Checks a render's answer against what it was `asked`, and returns it
/// with each tool a set repeats kept at its first copy.
///
/// # Errors
/// Returns [`Error::ToolSets`] for an answer whose Packs differ from the
/// ones asked, in order, or naming the first rendered tool a Pack's
/// answer may not hold.
fn check_render(
    asked: &[PackToolSets<ToolInfo>],
    answered: Vec<PackToolSets<ToolInfo>>,
    catalog: &ToolCatalog,
) -> Result<Vec<PackToolSets<ToolInfo>>> {
    let packs = |all: &[PackToolSets<ToolInfo>]| -> Vec<PackId> {
        all.iter().map(|sets| sets.pack.clone()).collect()
    };
    if packs(asked) != packs(&answered) {
        return Err(Error::ToolSets {
            evidence: SetEvidence::RenderedPacks {
                asked: packs(asked),
                answered: packs(&answered),
            },
        });
    }
    answered
        .into_iter()
        .zip(asked)
        .map(|(mut sets, offered)| {
            for set in [OfferMode::Static, OfferMode::Dynamic] {
                let tools = std::mem::take(set_mut(&mut sets, set));
                let kept = check_set(&sets.pack, set, set_of(offered, set), tools, catalog)?;
                *set_mut(&mut sets, set) = kept;
            }
            Ok(sets)
        })
        .collect()
}

/// Checks the tools `pack`'s render returned for `set`, which the section
/// offered `offered` from it into, and returns them with each repeat
/// dropped.
///
/// # Errors
/// Returns [`Error::ToolSets`] naming the first tool that sits in a set
/// the Pack was not offered into, under another Pack's id, or is a run
/// catalog tool the section did not offer in `set`.
fn check_set(
    pack: &PackId,
    set: OfferMode,
    offered: &[ToolInfo],
    tools: Vec<ToolInfo>,
    catalog: &ToolCatalog,
) -> Result<Vec<ToolInfo>> {
    let refuse = |rule: SetRule, tool: &ToolInfo| Error::ToolSets {
        evidence: SetEvidence::Tools {
            rule,
            tools: NonEmpty::new(
                SetTool {
                    id: tool.id.clone(),
                    set,
                },
                Vec::new(),
            ),
        },
    };
    let mut kept: Vec<ToolInfo> = Vec::with_capacity(tools.len());
    for tool in tools {
        if offered.is_empty() {
            return Err(refuse(SetRule::UnofferedSet(pack.clone()), &tool));
        }
        if !pack.contains(&tool.id) {
            return Err(refuse(SetRule::ForeignTool(pack.clone()), &tool));
        }
        let unoffered = !offered.iter().any(|held| held.id == tool.id);
        if unoffered && catalog.get(&tool.id).is_some() {
            return Err(refuse(SetRule::UnofferedCatalogTool, &tool));
        }
        if !kept.iter().any(|held| held.id == tool.id) {
            kept.push(tool);
        }
    }
    Ok(kept)
}

/// Lays `offers` out as `set`: each adhoc tool at its place, and each
/// Pack's rendered tools for `set` where that Pack's first tool sits.
fn lay_out(
    offers: &[OfferedTool],
    set: OfferMode,
    rendered: &mut [PackToolSets<ToolInfo>],
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
                    let block = std::mem::take(set_mut(answer, set));
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
/// kind, listing flag, and stop flag when its id is a catalog tool's.
fn bind(mut tool: ToolInfo, catalog: &ToolCatalog) -> ToolBinding {
    if let Some(known) = catalog.get(&tool.id) {
        tool.structured_output = known.structured_output;
        tool.describes_tools = known.describes_tools;
        tool.survives_stop = known.survives_stop;
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
        .map(|tool| (tool, OfferMode::Static))
        .chain(dynamic_set.iter().map(|tool| (tool, OfferMode::Dynamic)));
    let mut taken: BTreeMap<&str, SetTool> = BTreeMap::new();
    for (tool, set) in sited {
        let wire_name = tool.binding.alias();
        let site = SetTool {
            id: tool.binding.id().clone(),
            set,
        };
        if let Some(first) = taken.get(wire_name) {
            return Err(Error::ToolSets {
                evidence: SetEvidence::Tools {
                    rule: SetRule::UniqueWireName(wire_name.to_owned()),
                    tools: NonEmpty::new(first.clone(), vec![site]),
                },
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
        cause: Box::new(error),
    })
}

#[cfg(test)]
#[path = "scope-tests.rs"]
mod tests;
