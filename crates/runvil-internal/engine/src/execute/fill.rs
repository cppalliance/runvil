//! The run's fill function: the caller's role map becomes the run's model
//! set when the run is built.

use std::collections::BTreeMap;

use crate::model::{ModelBinding, ModelInfo, ModelInvocation, ModelRole, ModelSet, ThinkingMode};
use crate::parser::{Prompt, RoleRequirements};

use super::requirements::{RequirementCheck, Requirements, UnmetRequirement};

/// Builds the run's model set from the caller's role map and checks what the
/// prompt declares against the models that fill the roles.
///
/// A role's model is its own entry in `models`, else the `default` entry.
/// The set holds one binding for each of the five roles, in
/// [`ModelRole::ALL`] order, for every prompt whether or not it declares the
/// role. Each declared role's context minimum and thinking requirement are
/// checked against its model's info, reporting required versus actual into
/// [`Requirements::unmet_models`], and a declared thinking requirement sets
/// the thinking switch on that role's binding.
///
/// An empty map means the caller has no model, so the set stays empty and
/// nothing is checked. A non-empty map with no `default` entry reports one
/// [`RequirementCheck::Unbound`] entry, and the set stays empty.
pub(super) fn fill_model_set(
    prompt: &Prompt,
    mut models: BTreeMap<ModelRole, ModelInfo>,
    requirements: &mut Requirements,
) -> ModelSet {
    let mut set = ModelSet::default();
    if models.is_empty() {
        return set;
    }
    let Some(default) = models.remove(&ModelRole::Default) else {
        requirements.unmet_models.push(UnmetRequirement {
            role: ModelRole::Default,
            check: RequirementCheck::Unbound,
            required: "a model".to_owned(),
            actual: "none".to_owned(),
        });
        return set;
    };
    for role in ModelRole::ALL {
        let info = models.remove(&role).unwrap_or_else(|| default.clone());
        let declared = prompt.frontmatter().role_requirements(role);
        let minimum = declared.and_then(RoleRequirements::min_context);
        let thinking = declared.and_then(RoleRequirements::thinking);
        if let Some(minimum) = minimum
            && info.context() < minimum
        {
            requirements.unmet_models.push(UnmetRequirement {
                role,
                check: RequirementCheck::ContextMinimum,
                required: minimum.to_string(),
                actual: info.context().to_string(),
            });
        }
        check_thinking(role, thinking, &info, requirements);
        let binding = ModelBinding::new(
            role.as_str(),
            info.description(),
            info.id().clone(),
            ModelInvocation::new().with_thinking(thinking),
            info.context(),
        )
        .with_catalog_info(info);
        set.push(binding);
    }
    set
}

/// Reports the thinking requirement of `role` when `info`'s thinking mode
/// fails it: `thinking: true` needs a model that can think, and
/// `thinking: false` needs one that can run without thinking.
fn check_thinking(
    role: ModelRole,
    thinking: Option<bool>,
    info: &ModelInfo,
    requirements: &mut Requirements,
) {
    let failed = match thinking {
        Some(true) if info.thinking() == ThinkingMode::Never => Some(("thinking: true", "Never")),
        Some(false) if info.thinking() == ThinkingMode::Always => {
            Some(("thinking: false", "Always"))
        }
        _ => None,
    };
    if let Some((required, actual)) = failed {
        requirements.unmet_models.push(UnmetRequirement {
            role,
            check: RequirementCheck::Thinking,
            required: required.to_owned(),
            actual: actual.to_owned(),
        });
    }
}
