//! Public model vocabulary: stable identity, catalog, and model info.
//!
//! The caller supplies a [`ModelCatalog`], and catalog entries are named by
//! their validated [`ModelId`]. These types are the shared vocabulary every
//! runvil crate and every caller may name, with no transport, binding,
//! or invocation machinery.

use std::num::NonZeroU32;

use serde::Deserialize;

use crate::names::IdError;

/// The stable identity of one catalogued model.
///
/// An identity pairs a server namespace with a model name. A model the
/// gateway serves uses the namespace `"gateway"` and the name the gateway
/// lists it under. That name is the `name` key of the model's entry in the
/// gateway configuration and the `id` field in the OpenAI-compatible model
/// list.
///
/// Both parts are always non-empty and free of control characters.
/// [`ModelId::new`] and [`ModelId::gateway`] reject any other value.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub struct ModelId {
    server: String,
    name: String,
}

impl ModelId {
    /// The namespace for models the gateway serves.
    pub const GATEWAY: &'static str = "gateway";

    /// Builds an identity from its server namespace and model name.
    ///
    /// # Errors
    /// Returns [`IdError`] if `server` or `name` is empty or contains a
    /// control character, so every `ModelId` is usable.
    pub fn new(
        server: impl Into<String>,
        name: impl Into<String>,
    ) -> std::result::Result<ModelId, IdError> {
        let server = server.into();
        let name = name.into();
        Self::validate("server", &server)?;
        Self::validate("name", &name)?;
        Ok(Self { server, name })
    }

    /// Builds an identity in the `"gateway"` namespace.
    ///
    /// `name` is the name the gateway lists the model under.
    ///
    /// # Errors
    /// Returns [`IdError`] if `name` is empty or contains a control
    /// character.
    pub fn gateway(name: impl Into<String>) -> std::result::Result<ModelId, IdError> {
        Self::new(Self::GATEWAY, name)
    }

    /// Validates one identity component, naming the field in any error.
    ///
    /// Rejection is by Unicode scalar, not raw byte: every control
    /// character is refused, including C1 controls such as U+0085 (NEL) whose
    /// UTF-8 encoding a byte-range scan would miss.
    fn validate(field: &'static str, value: &str) -> std::result::Result<(), IdError> {
        let reason = if value.is_empty() {
            "must not be empty"
        } else if value.chars().any(char::is_control) {
            "must not contain a control character"
        } else {
            return Ok(());
        };
        Err(IdError::new(format!("invalid model id: {field} {reason}")))
    }

    /// Returns the server namespace.
    #[must_use]
    pub(crate) fn server(&self) -> &str {
        &self.server
    }

    /// Returns the model name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Whether a catalogued model can emit thinking tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum ThinkingMode {
    /// The backend always runs with thinking off.
    Never,
    /// The backend always emits thinking tokens.
    Always,
    /// The client may turn thinking on or off per request.
    Switchable,
}

/// One of the five fixed model tiers a prompt can ask for.
///
/// Each role is a tier the caller maps to a model. A role with no model of its
/// own uses the model mapped to [`ModelRole::Default`]. The names are
/// lowercase on the wire, and [`ModelRole::ALL`] lists them in a fixed order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum ModelRole {
    /// The model every other role falls back to.
    Default,
    /// The most capable tier, for hard reasoning.
    Strong,
    /// A compact tier, for light work.
    Small,
    /// The lowest-latency tier.
    Fast,
    /// The tier for open-ended writing.
    Creative,
}

impl ModelRole {
    /// Every role, in declaration order.
    pub const ALL: [ModelRole; 5] = [
        ModelRole::Default,
        ModelRole::Strong,
        ModelRole::Small,
        ModelRole::Fast,
        ModelRole::Creative,
    ];

    /// Returns the role's lowercase name, the text a prompt writes.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ModelRole::Default => "default",
            ModelRole::Strong => "strong",
            ModelRole::Small => "small",
            ModelRole::Fast => "fast",
            ModelRole::Creative => "creative",
        }
    }
}

/// Describes one catalogued model.
///
/// It holds the model's identity, description, context window, thinking
/// mode, and the provider id the catalog names, if any.
///
/// Two gateway types share the name but are different types:
/// `gateway_api_types::ModelInfo`, one entry of the gateway's `/v1/models`
/// list, and `gateway_local::gguf::ModelInfo`, the metadata read from a
/// GGUF file.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ModelInfo {
    id: ModelId,
    description: String,
    context: NonZeroU32,
    thinking: ThinkingMode,
    provider: Option<String>,
}

impl ModelInfo {
    /// Builds a `ModelInfo` from its identity and catalog fields.
    ///
    /// The context window is a [`NonZeroU32`], so it always holds at least
    /// one token.
    #[must_use]
    pub fn new(
        id: ModelId,
        description: impl Into<String>,
        context: NonZeroU32,
        thinking: ThinkingMode,
    ) -> Self {
        Self {
            id,
            description: description.into(),
            context,
            thinking,
            provider: None,
        }
    }

    /// Sets the provider id the catalog names for this model.
    #[must_use]
    pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = Some(provider.into());
        self
    }

    /// Returns the stable identity.
    #[must_use]
    pub fn id(&self) -> &ModelId {
        &self.id
    }

    /// Returns the prose that describes the model.
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Returns the context window size in tokens (always non-zero).
    #[must_use]
    pub fn context(&self) -> NonZeroU32 {
        self.context
    }

    /// Returns the thinking capability.
    #[must_use]
    pub fn thinking(&self) -> ThinkingMode {
        self.thinking
    }

    /// Returns the provider id, or `None` when the catalog names none.
    #[must_use]
    pub fn provider(&self) -> Option<&str> {
        self.provider.as_deref()
    }
}

/// A list of available models, each with a distinct identity.
///
/// [`ModelCatalog::new`] rejects a repeated `ModelId`, and
/// [`ModelCatalog::empty`] returns an empty catalog.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ModelCatalog {
    models: Vec<ModelInfo>,
}

impl ModelCatalog {
    /// Builds a catalog from `ModelInfo` values, keeping the order they are
    /// given in.
    ///
    /// # Errors
    /// Returns [`IdError`] when two `ModelInfo` values share one stable
    /// [`ModelId`], so each identity in the catalog names exactly one model
    /// info.
    pub fn new(
        models: impl IntoIterator<Item = ModelInfo>,
    ) -> std::result::Result<ModelCatalog, IdError> {
        let models: Vec<ModelInfo> = models.into_iter().collect();
        for (index, model) in models.iter().enumerate() {
            if models[..index].iter().any(|prior| prior.id() == model.id()) {
                return Err(IdError::new(format!(
                    "duplicate model identity in catalog: {}/{}",
                    model.id().server(),
                    model.id().name()
                )));
            }
        }
        Ok(Self { models })
    }

    /// Returns an empty catalog.
    #[must_use]
    pub fn empty() -> Self {
        Self { models: Vec::new() }
    }

    /// Returns every model info.
    #[must_use]
    pub fn models(&self) -> &[ModelInfo] {
        &self.models
    }

    /// Looks up a model info by stable identity.
    #[must_use]
    pub fn get(&self, id: &ModelId) -> Option<&ModelInfo> {
        self.models.iter().find(|model| model.id() == id)
    }

    /// Returns whether the catalog contains a model info with `id`.
    #[must_use]
    pub fn contains(&self, id: &ModelId) -> bool {
        self.get(id).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_c0_c1_and_other_control_scalars() {
        // The C0 record separator (U+001E) must never survive into an id.
        assert!(ModelId::new(ModelId::GATEWAY, "a\u{001e}b").is_err());
        // A C1 control (NEL, U+0085) whose UTF-8 bytes (0xC2 0x85) a byte-range
        // scan would miss but a scalar `is_control` scan rejects.
        assert!(ModelId::new(ModelId::GATEWAY, "a\u{0085}b").is_err());
        // DEL (U+007F) and NUL are refused too.
        assert!(ModelId::new(ModelId::GATEWAY, "a\u{007f}b").is_err());
        assert!(ModelId::new("srv\u{0000}", "name").is_err());
        // A benign multi-byte non-ASCII name is still accepted.
        assert!(ModelId::new(ModelId::GATEWAY, "café-模型").is_ok());
    }

    #[test]
    fn model_id_rejects_empty_and_control_characters() {
        let message = |result: std::result::Result<ModelId, IdError>| {
            result
                .expect_err("the model id must be rejected")
                .to_string()
        };
        assert_eq!(
            message(ModelId::gateway("")),
            "invalid model id: name must not be empty"
        );
        assert_eq!(
            message(ModelId::new("", "name")),
            "invalid model id: server must not be empty"
        );
        assert_eq!(
            message(ModelId::new("server", "")),
            "invalid model id: name must not be empty"
        );
        assert_eq!(
            message(ModelId::new("server", "na\nme")),
            "invalid model id: name must not contain a control character"
        );
        assert!(ModelId::gateway("valid-alias").is_ok());
    }

    #[test]
    fn a_model_id_exposes_its_server_and_name() {
        let id = ModelId::new(ModelId::GATEWAY, "claude-sonnet-4-6").expect("a valid model id");
        assert_eq!(id.server(), "gateway");
        assert_eq!(id.name(), "claude-sonnet-4-6");
    }

    #[test]
    fn a_thinking_mode_deserializes_from_its_lowercase_wire_form() {
        let mode: ThinkingMode =
            serde_json::from_str("\"switchable\"").expect("a thinking mode deserializes");
        assert_eq!(mode, ThinkingMode::Switchable);
    }

    #[test]
    fn a_model_info_keeps_its_context_window_and_thinking_mode() {
        let context = NonZeroU32::new(131_072).expect("test context window is non-zero");
        let model = ModelInfo::new(
            ModelId::gateway("analyst").expect("test model alias is valid"),
            "A careful analysis model",
            context,
            ThinkingMode::Switchable,
        );
        assert_eq!(model.context(), context);
        assert_eq!(model.thinking(), ThinkingMode::Switchable);
    }

    #[test]
    fn a_model_catalog_contains_each_model_info_it_was_built_from() {
        let ctx = NonZeroU32::new(8_192).expect("test context window is non-zero");
        let id = ModelId::gateway("small").expect("test model alias is valid");
        let catalog = ModelCatalog::new([ModelInfo::new(
            id.clone(),
            "A tiny model",
            ctx,
            ThinkingMode::Never,
        )])
        .expect("a catalog with one model info builds");
        assert!(catalog.contains(&id));
        assert_eq!(catalog.models().len(), 1);
    }

    #[test]
    fn model_catalog_rejects_duplicate_ids() {
        let ctx = NonZeroU32::new(8_192).expect("test context window is non-zero");
        let info = |name: &str| {
            ModelInfo::new(
                ModelId::gateway(name).expect("test model alias is valid"),
                "d",
                ctx,
                ThinkingMode::Never,
            )
        };
        let err = ModelCatalog::new([info("dup"), info("dup")])
            .expect_err("a catalog with duplicate ids must be rejected");
        assert_eq!(
            err.to_string(),
            "duplicate model identity in catalog: gateway/dup"
        );
        assert!(ModelCatalog::new([info("a"), info("b")]).is_ok());
    }

    #[test]
    fn each_model_role_name_deserializes_to_its_variant() {
        let cases = [
            ("default", ModelRole::Default),
            ("strong", ModelRole::Strong),
            ("small", ModelRole::Small),
            ("fast", ModelRole::Fast),
            ("creative", ModelRole::Creative),
        ];
        for (name, expected) in cases {
            let role: ModelRole = serde_json::from_str(&format!("\"{name}\""))
                .expect("a model role name deserializes");
            assert_eq!(role, expected, "model_role name {name}");
        }
    }

    #[test]
    fn model_role_all_lists_the_five_roles_in_order_and_round_trips_through_as_str() {
        assert_eq!(
            ModelRole::ALL,
            [
                ModelRole::Default,
                ModelRole::Strong,
                ModelRole::Small,
                ModelRole::Fast,
                ModelRole::Creative,
            ]
        );
        for role in ModelRole::ALL {
            let back: ModelRole = serde_json::from_str(&format!("\"{}\"", role.as_str()))
                .expect("a model_role as_str text deserializes");
            assert_eq!(back, role);
        }
    }

    #[test]
    fn an_unknown_model_role_name_fails_and_the_error_names_all_five_roles() {
        let err = serde_json::from_str::<ModelRole>("\"reviewer\"")
            .expect_err("an unknown model_role name must be rejected")
            .to_string();
        for name in ["default", "strong", "small", "fast", "creative"] {
            assert!(err.contains(name), "model_role error {err:?} omits {name}");
        }
    }

    #[test]
    fn a_model_role_works_as_a_btree_map_key_ordered_like_all() {
        let map: std::collections::BTreeMap<ModelRole, usize> = ModelRole::ALL
            .iter()
            .rev()
            .enumerate()
            .map(|(index, role)| (*role, index))
            .collect();
        let keys: Vec<ModelRole> = map.keys().copied().collect();
        assert_eq!(keys, ModelRole::ALL, "model_role keys follow the ALL order");
    }
}
