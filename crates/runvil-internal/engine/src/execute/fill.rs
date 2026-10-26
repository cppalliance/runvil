//! Prepare's fill function: the trivial model fill.

use runvil_parser::ModelKeyword;
use runvil_parser::detail::keyword_name;

use crate::model::ThinkingMode;
use crate::parser::Prompt;

use super::bindings::ModelBindings;
use super::requirements::{RequirementCheck, Requirements, UnmetRequirement};

/// The trivial model fill: binds every declared role to the run's
/// current model and checks each role's hard keywords and context
/// minimum against its model info, reporting required versus actual into
/// [`Requirements::unmet_models`]. With no current model there is nothing
/// to fill or check.
pub(super) fn fill_model_bindings(
    prompt: &Prompt,
    model: Option<crate::model::ModelInfo>,
    requirements: &mut Requirements,
) -> ModelBindings {
    let mut bindings = ModelBindings::default();
    let Some(model) = model else {
        return bindings;
    };
    for (label, role) in prompt.frontmatter().models() {
        if let Some(minimum) = role.min_context()
            && model.context() < minimum
        {
            requirements.unmet_models.push(UnmetRequirement {
                role: label.to_owned(),
                check: RequirementCheck::ContextMinimum,
                required: minimum.to_string(),
                actual: model.context().to_string(),
            });
        }
        for keyword in role.keywords() {
            // Soft keywords document author intent; only the hard
            // keywords have a model info property to check against.
            let actual = match keyword {
                ModelKeyword::Thinking if model.thinking() == ThinkingMode::Never => Some("Never"),
                ModelKeyword::NoThinking if model.thinking() == ThinkingMode::Always => {
                    Some("Always")
                }
                _ => None,
            };
            if let Some(actual) = actual {
                requirements.unmet_models.push(UnmetRequirement {
                    role: label.to_owned(),
                    check: RequirementCheck::HardKeyword,
                    required: keyword_name(*keyword).to_owned(),
                    actual: actual.to_owned(),
                });
            }
        }
        bindings.bind(label, model.clone());
    }
    bindings
}
