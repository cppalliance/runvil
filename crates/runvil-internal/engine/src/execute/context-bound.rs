//! The run-scoped sets built from the prepared bindings, and the `argv`
//! derivation: the pieces `RunState::new` assembles once per run.

use std::collections::BTreeMap;

use runvil_parser::ModelKeyword;
use runvil_types::packs::PackId;

use crate::lua::ToolSet;
use crate::model::{ModelBinding, ModelInvocation, ModelSet};
use crate::parser::Prompt;

use super::super::config::RunContext;

/// Builds the run's shared tool set: the frontmatter's declared Packs,
/// the `roster` moved out of the context, the run catalog, which shares
/// the context's handle, and each Pack's vendor. Nothing is bound here:
/// each section entry binds the tools it offers when it freezes its sets.
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
        ctx.tools.clone(),
        ctx.vendors.clone(),
    )
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
/// switch as the frozen invocation, the model info's provider id when
/// the catalog names one, and the model info itself, which a section
/// entry's render receives. Unfilled roles produce no binding:
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
        )
        .with_catalog_info(info.clone());
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
