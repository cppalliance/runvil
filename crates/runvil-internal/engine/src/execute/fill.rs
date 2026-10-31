//! Prepare's fill function: the caller's role map becomes the run's model set.

use std::collections::BTreeMap;

use crate::model::{ModelBinding, ModelInfo, ModelInvocation, ModelRole, ModelSet, ThinkingMode};
use crate::parser::{ModelKeyword, Prompt};

use super::requirements::{RequirementCheck, Requirements, UnmetRequirement};

/// Builds the run's model set from the caller's role map and checks what the
/// prompt declares against the models that fill the roles.
///
/// A role's model is its own entry in `models`, else the `default` entry.
/// The set holds one binding for each of the five roles, in
/// [`ModelRole::ALL`] order, for every prompt whether or not it declares the
/// role. Each declared role's context minimum and hard keywords are checked
/// against its model's info, reporting required versus actual into
/// [`Requirements::unmet_models`]. A label that is not one of the five roles
/// is not read.
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
        let (keywords, minimum) = match prompt.frontmatter().models().get(role.as_str()) {
            Some(declared) => (declared.keywords(), declared.min_context()),
            None => (&[][..], None),
        };
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
        check_thinking(role, keywords, &info, requirements);
        let binding = ModelBinding::new(
            role.as_str(),
            info.description(),
            info.id().clone(),
            ModelInvocation::new().with_thinking(thinking_switch(keywords)),
            info.context(),
        )
        .with_catalog_info(info);
        set.push(binding);
    }
    set
}

/// Reports each hard keyword of `role` that `info`'s thinking mode fails.
/// Soft keywords document author intent; only the hard keywords have a model
/// info property to check against.
fn check_thinking(
    role: ModelRole,
    keywords: &[ModelKeyword],
    info: &ModelInfo,
    requirements: &mut Requirements,
) {
    for keyword in keywords {
        let failed = match keyword {
            ModelKeyword::Thinking if info.thinking() == ThinkingMode::Never => {
                Some(("thinking: true", "Never"))
            }
            ModelKeyword::NoThinking if info.thinking() == ThinkingMode::Always => {
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
}

/// The thinking switch the hard keywords of a role set: `Some(true)` for
/// `thinking`, `Some(false)` for `no-thinking`, and the later one when a
/// role names both.
fn thinking_switch(keywords: &[ModelKeyword]) -> Option<bool> {
    keywords.iter().fold(None, |switch, keyword| match keyword {
        ModelKeyword::Thinking => Some(true),
        ModelKeyword::NoThinking => Some(false),
        _ => switch,
    })
}
