//! The run's model bindings: [`ModelBindings`].

use std::collections::BTreeMap;

use crate::model::{ModelId, ModelInfo};

/// The models bound for a run: which model each declared role uses, and
/// the model info of every model a role is bound to.
///
/// [`prepare`](super::RunContext::prepare) fills these bindings. It binds
/// every declared role to the current model the caller passes it.
///
/// A lookup goes from a role label to a model identity, and from that
/// identity to the model's info.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelBindings {
    /// The decision: role label to the bound model's identity.
    roles: BTreeMap<String, ModelId>,
    /// The info of every model a role is bound to: identity to model info.
    models: BTreeMap<ModelId, ModelInfo>,
}

impl ModelBindings {
    /// Binds the role `label` to `model`, recording the model info under
    /// its identity.
    pub(super) fn bind(&mut self, label: &str, model: ModelInfo) {
        self.roles.insert(label.to_owned(), model.id().clone());
        self.models.entry(model.id().clone()).or_insert(model);
    }

    /// Returns the identity of the model bound to the role `label`, or
    /// `None` when the role was not filled.
    #[must_use]
    pub fn role_id(&self, label: &str) -> Option<&ModelId> {
        self.roles.get(label)
    }

    /// Returns the info of the model bound to the role `label`, or
    /// `None` when the role was not filled.
    #[must_use]
    pub fn resolve(&self, label: &str) -> Option<&ModelInfo> {
        self.roles.get(label).and_then(|id| self.models.get(id))
    }

    /// Returns the info of the model with the identity `id`, or `None`
    /// when no role is bound to that model.
    ///
    /// `None` does not mean the run may not use the model: a catalog model
    /// that a UI snapshot reaches by raw id is not in this map either,
    /// because only a role binding puts one here.
    #[must_use]
    pub fn model(&self, id: &ModelId) -> Option<&ModelInfo> {
        self.models.get(id)
    }

    /// Returns the number of bound roles.
    #[must_use]
    pub fn len(&self) -> usize {
        self.roles.len()
    }

    /// Returns whether the set of bound roles is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.roles.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::*;
    use crate::model::ThinkingMode;

    fn model_info(name: &str) -> ModelInfo {
        ModelInfo::new(
            ModelId::gateway(name).expect("the test id is valid"),
            "A test model",
            NonZeroU32::new(32_000).expect("the context window is non-zero"),
            ThinkingMode::Switchable,
        )
    }

    #[test]
    fn an_empty_binding_set_resolves_nothing() {
        let bindings = ModelBindings::default();
        assert!(bindings.is_empty());
        assert_eq!(bindings.len(), 0);
        assert!(bindings.role_id("analyst").is_none());
        assert!(bindings.resolve("analyst").is_none());
    }

    #[test]
    fn two_roles_bound_to_one_model_share_one_model_info_entry() {
        // The trivial fill binds every role to the same model, and the
        // model info table holds it once.
        let model = model_info("current");
        let mut bindings = ModelBindings::default();
        bindings.bind("analyst", model.clone());
        bindings.bind("triage", model.clone());
        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings.role_id("analyst"), Some(model.id()));
        assert_eq!(bindings.role_id("triage"), Some(model.id()));
        assert_eq!(bindings.resolve("analyst"), Some(&model));
        assert_eq!(bindings.resolve("triage"), Some(&model));
        assert_eq!(bindings.model(model.id()), Some(&model));
        assert!(
            bindings
                .model(&ModelId::gateway("other").expect("valid"))
                .is_none()
        );
    }
}
