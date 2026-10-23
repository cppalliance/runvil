//! The run-scoped sets built from the prepared bindings, and the `argv`
//! derivation: the pieces `RunState::new` assembles once per run.

use std::collections::{BTreeMap, BTreeSet};

use runvil_model_client::detail;
use runvil_parser::ModelKeyword;
use runvil_types::packs::PackId;
use runvil_types::tools::ToolInfo;

use crate::lua::{ToolBinding, ToolSet};
use crate::model::{ModelBinding, ModelInvocation, ModelSet};
use crate::parser::Prompt;

use super::super::config::RunContext;

/// Builds the run's shared tool set: the frontmatter's declared Packs,
/// the `roster` moved out of the context, and every catalog tool the run
/// can offer bound with the tool's info (its schema, description, output
/// kind, and listing flag) and its Pack's vendor, so run-time execution
/// never consults the catalog again and never holds an implementation.
pub(super) fn bound_tool_set(
    prompt: &Prompt,
    ctx: &RunContext,
    roster: BTreeMap<PackId, Option<String>>,
) -> ToolSet {
    ToolSet::new(
        prompt
            .frontmatter()
            .uses()
            .iter()
            .map(|entry| entry.id().clone())
            .collect(),
        roster,
        offered_bindings(ctx),
    )
}

/// Binds the offering: every catalog tool, in tool id order, under its
/// wire name, its id with `/` and `.` replaced by `_` (`web/fetch` becomes
/// `web_fetch`). A tool whose name can't be offered is left out with a log
/// line; a script still reaches it by id through the catalog bindings.
fn offered_bindings(ctx: &RunContext) -> Vec<ToolBinding> {
    let mut tools: Vec<&ToolInfo> = ctx.tools.tools().iter().collect();
    tools.sort_unstable_by(|left, right| left.id.cmp(&right.id));
    let mut taken = BTreeSet::new();
    let mut offered = Vec::new();
    for tool in tools {
        let name = tool.id.to_string().replace(['/', '.'], "_");
        if let Some(reason) = offer_refusal(&name, tool, &taken) {
            tracing::warn!(tool = %tool.id, %name, %reason, "a tool is left out of the offering");
            continue;
        }
        offered.push(catalog_binding(ctx, &name, tool));
        taken.insert(name);
    }
    offered
}

/// Binds `tool` under `alias`, stamped with the vendor the caller
/// reported for its Pack, if any.
fn catalog_binding(ctx: &RunContext, alias: &str, tool: &ToolInfo) -> ToolBinding {
    ToolBinding::from_info(alias, tool).with_vendor(ctx.vendors.get(&tool.id.pack()).cloned())
}

/// Why `tool` can't be offered under `name`: no model round could
/// advertise it, or an earlier offered tool has it. `None` when it can be
/// offered.
fn offer_refusal(name: &str, tool: &ToolInfo, taken: &BTreeSet<String>) -> Option<String> {
    if let Err(error) = detail::tool_schema_new(
        name,
        tool.description.as_str(),
        tool.parameters_schema.clone(),
    ) {
        return Some(error.to_string());
    }
    if taken.contains(name) {
        return Some("an earlier offered tool has the same name".to_owned());
    }
    None
}

/// Binds every tool in the prepared catalog under its full id, keyed by
/// that id: what a script `tools.call` falls back to when the id names no
/// offered tool. These bindings stay out of the tool set, so they never
/// enter a section's scope and are never advertised.
pub(super) fn catalog_bindings(ctx: &RunContext) -> BTreeMap<String, ToolBinding> {
    ctx.tools
        .tools()
        .iter()
        .map(|tool| {
            let id = tool.id.to_string();
            let binding = catalog_binding(ctx, &id, tool);
            (id, binding)
        })
        .collect()
}

/// The keyword's stable kebab-case spelling, journaled onto the binding as
/// the role's capability set.
fn keyword_name(keyword: ModelKeyword) -> &'static str {
    match keyword {
        ModelKeyword::Thinking => "thinking",
        ModelKeyword::NoThinking => "no-thinking",
        ModelKeyword::Frontier => "frontier",
        ModelKeyword::Fast => "fast",
        ModelKeyword::Small => "small",
        ModelKeyword::Creative => "creative",
        ModelKeyword::Chat => "chat",
        // `ModelKeyword` is `#[non_exhaustive]`; an unlisted keyword
        // reports as unknown rather than breaking the fill.
        _ => "unknown",
    }
}

/// Builds the run's shared model set from the prepared bindings: every
/// filled role becomes a binding under its label, holding the role's
/// keyword set (the handle's `capabilities`), the hard-keyword thinking
/// switch as the frozen invocation, and the model info's provider id when
/// the catalog names one. Unfilled roles produce no binding:
/// `models.use` on the label fails at run time.
pub(super) fn bound_model_set(prompt: &Prompt, ctx: &RunContext) -> ModelSet {
    let mut set = ModelSet::default();
    for (label, role) in prompt.frontmatter().models() {
        let Some(info) = ctx.model_bindings.resolve(label) else {
            continue;
        };
        let mut thinking = None;
        for keyword in role.keywords() {
            match keyword {
                ModelKeyword::Thinking => thinking = Some(true),
                ModelKeyword::NoThinking => thinking = Some(false),
                _ => {}
            }
        }
        let mut binding = ModelBinding::new(
            label,
            role.description().unwrap_or_else(|| info.description()),
            info.id().clone(),
            ModelInvocation::new().with_thinking(thinking),
            info.context(),
        )
        .with_capabilities(
            role.keywords()
                .iter()
                .map(|keyword| keyword_name(*keyword))
                .map(str::to_owned)
                .collect(),
        );
        if let Some(provider) = info.provider() {
            binding = binding.with_provider(provider);
        }
        set.push(binding);
    }
    set
}

/// Derives a section's `argv` from the args string under the prompt's
/// declaration: a default-declared prompt wraps the interface prose into
/// the default shape (`argv.prose`, with the empty string present, not
/// absent); a structured declaration parses the string as JSON, and a parse
/// failure or a JSON `null` reads as nil (`if argv then` is the malformed
/// check). The executor never hard-errors on shape.
pub(super) fn derive_argv(prompt: &Prompt, args: &str) -> Option<serde_json::Value> {
    if prompt.frontmatter().args().is_default() {
        return Some(serde_json::json!({ "prose": args }));
    }
    match serde_json::from_str(args) {
        Ok(serde_json::Value::Null) | Err(_) => None,
        Ok(value) => Some(value),
    }
}
