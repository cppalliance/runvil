//! Model value types: validated temperature, bind/invocation options,
//! prompt-local bindings, and completion options.

use std::num::NonZeroU32;

use runvil_types::error::{Error, ErrorCode, FactKey};
use runvil_types::models::{ModelId, ModelInfo, ThinkingMode};
use serde_json::Value;

/// The largest sampling temperature the backend accepts.
const TEMPERATURE_MAX: f64 = 2.0;

/// A validated sampling temperature: finite and within `[0.0, 2.0]`.
///
/// A temperature enters a request only as a `Temperature`, so the backend
/// receives only valid temperatures. Being finite, it compares as a total
/// equivalence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Temperature(f64);

impl Eq for Temperature {}

impl Temperature {
    /// Builds a temperature from a finite `value` within `[0.0, 2.0]`.
    ///
    /// # Errors
    /// Returns an [`InvalidValue`](ErrorCode::InvalidValue) error with the
    /// reason `not_finite` when `value` is `NaN` or an infinity, or
    /// `out_of_range` when it falls outside `[0.0, 2.0]`.
    pub fn new(value: f64) -> std::result::Result<Temperature, Error> {
        if !value.is_finite() {
            return Err(invalid_temperature(
                "not_finite",
                "temperature must be finite".to_owned(),
            ));
        }
        if !(0.0..=TEMPERATURE_MAX).contains(&value) {
            return Err(invalid_temperature(
                "out_of_range",
                format!("temperature {value} is outside the supported range [0.0, 2.0]"),
            ));
        }
        Ok(Temperature(value))
    }

    /// Returns the validated value.
    #[must_use]
    pub fn get(self) -> f64 {
        self.0
    }
}

/// The error for a rejected temperature: an `InvalidValue` that names the
/// `temperature` setting, with the broken rule as its reason.
fn invalid_temperature(reason: &'static str, message: String) -> Error {
    let mut error = Error::new(
        ErrorCode::InvalidValue,
        message,
        "The temperature must be a finite number from 0.0 to 2.0. Use a value in that range.",
    );
    error
        .facts
        .insert(FactKey::Name, Value::from("temperature"));
    error.facts.insert(FactKey::Reason, Value::from(reason));
    error
}

/// The fixed request settings that a model binding applies to every completion.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ModelInvocation {
    /// The sampling temperature, when the binding's builder or a `models.use`
    /// option set one.
    ///
    /// It is a validated [`Temperature`], so the binding and the request carry
    /// only finite values within `[0.0, 2.0]`.
    pub temperature: Option<Temperature>,
    /// The maximum number of tokens to generate, when the binding's builder or
    /// a `models.use` option set a cap.
    ///
    /// The cap is at least one, so it always allows some output.
    /// `models.use` rejects a zero cap when it is called.
    pub max_tokens: Option<NonZeroU32>,
    /// The thinking switch, sent as `chat_template_kwargs.enable_thinking`,
    /// when set.
    pub thinking: Option<bool>,
}

impl ModelInvocation {
    /// Builds settings that set no temperature, token cap, or thinking
    /// switch.
    #[must_use]
    pub fn new() -> ModelInvocation {
        ModelInvocation::default()
    }

    /// Sets the sampling temperature.
    #[must_use]
    pub fn with_temperature(mut self, temperature: Option<Temperature>) -> ModelInvocation {
        self.temperature = temperature;
        self
    }

    /// Sets the maximum number of tokens to generate.
    #[must_use]
    pub fn with_max_tokens(mut self, max_tokens: Option<NonZeroU32>) -> ModelInvocation {
        self.max_tokens = max_tokens;
        self
    }

    /// Sets the thinking switch.
    #[must_use]
    pub fn with_thinking(mut self, thinking: Option<bool>) -> ModelInvocation {
        self.thinking = thinking;
        self
    }
}

/// A prompt-local alias bound to one model and its fixed request settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelBinding {
    alias: String,
    description: String,
    id: ModelId,
    invocation: ModelInvocation,
    context: NonZeroU32,
    /// The model's provider id, when the catalog names one.
    provider: Option<String>,
    /// The catalog's description of the bound model, when the binding was
    /// filled from the catalog.
    catalog: Option<ModelInfo>,
}

impl ModelBinding {
    /// Builds a binding from every part a resolved model requires.
    ///
    /// The `context` window is a required argument of at least one token, so a
    /// binding is always complete once built.
    #[must_use]
    pub fn new(
        alias: impl Into<String>,
        description: impl Into<String>,
        id: ModelId,
        invocation: ModelInvocation,
        context: NonZeroU32,
    ) -> Self {
        Self {
            alias: alias.into(),
            description: description.into(),
            id,
            invocation,
            context,
            provider: None,
            catalog: None,
        }
    }

    /// Sets the provider id for a binding built without catalog info. A held
    /// catalog's provider wins.
    #[must_use]
    pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = Some(provider.into());
        self
    }

    /// Sets the catalog's description of the bound model, which
    /// [`render_info`](Self::render_info) returns.
    #[must_use]
    pub fn with_catalog_info(mut self, info: ModelInfo) -> Self {
        self.catalog = Some(info);
        self
    }

    /// Replaces the binding's request settings.
    #[must_use]
    pub fn with_invocation(mut self, invocation: ModelInvocation) -> Self {
        self.invocation = invocation;
        self
    }

    /// Returns the exact prompt-local alias.
    #[must_use]
    pub fn alias(&self) -> &str {
        &self.alias
    }

    /// Returns the description of the bound model.
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Returns the stable identity of the bound model.
    #[must_use]
    pub fn id(&self) -> &ModelId {
        &self.id
    }

    /// Returns the binding's request settings.
    #[must_use]
    pub fn invocation(&self) -> &ModelInvocation {
        &self.invocation
    }

    /// Returns the catalog context window size in tokens (always at least one).
    #[must_use]
    pub fn context(&self) -> NonZeroU32 {
        self.context
    }

    /// Returns the provider id, or `None` when neither a held catalog nor the
    /// binding's own field names one.
    ///
    /// A held catalog's provider wins, because the catalog is the record the
    /// binding was filled from.
    #[must_use]
    pub fn provider(&self) -> Option<&str> {
        self.catalog
            .as_ref()
            .and_then(ModelInfo::provider)
            .or(self.provider.as_deref())
    }

    /// Returns the model info a section entry's render receives for this
    /// binding: the catalog's, when the binding holds it, or else one built
    /// from the binding's id, description, context window, and provider
    /// that claims [`ThinkingMode::Never`], the claim that asks least of
    /// the model.
    #[must_use]
    pub fn render_info(&self) -> ModelInfo {
        if let Some(info) = &self.catalog {
            return info.clone();
        }
        let info = ModelInfo::new(
            self.id.clone(),
            self.description.clone(),
            self.context,
            ThinkingMode::Never,
        );
        match self.provider() {
            Some(provider) => info.with_provider(provider.to_owned()),
            None => info,
        }
    }

    /// Builds the [`CompletionOptions`] for a completion made under this
    /// binding, from the model's name and the binding's request settings.
    #[must_use]
    pub fn completion_options(&self) -> CompletionOptions {
        CompletionOptions::new(self.id.name()).with_invocation(self.invocation.clone())
    }
}

/// Per-call settings merged into a chat-completions request body.
///
/// Build one with [`CompletionOptions::new`] and its `with_*` setters.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CompletionOptions {
    /// The caller-facing model name sent on the wire.
    model: String,
    /// Every request setting the options apply, held as one value so the
    /// getters and the setters cannot disagree.
    invocation: ModelInvocation,
}

impl CompletionOptions {
    /// Builds options for `model`, with the temperature, token cap, and
    /// thinking switch set to `None`.
    #[must_use]
    pub fn new(model: impl Into<String>) -> CompletionOptions {
        CompletionOptions {
            model: model.into(),
            invocation: ModelInvocation::new(),
        }
    }

    /// Sets the sampling temperature after validating it is finite and within
    /// the backend-supported range `[0.0, 2.0]`.
    ///
    /// # Errors
    /// Returns the [`InvalidValue`](ErrorCode::InvalidValue) error of
    /// [`Temperature::new`] when `temperature` is `NaN` or an infinity, or
    /// falls outside `[0.0, 2.0]`, so only a valid temperature is sent.
    pub fn with_temperature(
        mut self,
        temperature: f64,
    ) -> std::result::Result<CompletionOptions, Error> {
        self.invocation.temperature = Some(Temperature::new(temperature)?);
        Ok(self)
    }

    /// Sets the maximum number of tokens to generate.
    ///
    /// The cap is a [`NonZeroU32`], so it always allows some output.
    #[must_use]
    pub fn with_max_tokens(mut self, max_tokens: NonZeroU32) -> CompletionOptions {
        self.invocation.max_tokens = Some(max_tokens);
        self
    }

    /// Sets the thinking switch, sent as `chat_template_kwargs.enable_thinking`.
    #[must_use]
    pub fn with_thinking(mut self, thinking: bool) -> CompletionOptions {
        self.invocation.thinking = Some(thinking);
        self
    }

    /// Replaces the request settings the options carry.
    ///
    /// A binding passes its own frozen [`ModelInvocation`] here, so the
    /// options apply exactly the settings the binding holds.
    #[must_use]
    pub fn with_invocation(mut self, invocation: ModelInvocation) -> CompletionOptions {
        self.invocation = invocation;
        self
    }

    /// Replaces the model name sent in the request and keeps every other
    /// setting.
    ///
    /// When the caller serves a request with a substitute for the bound model,
    /// it sets the substitute's name here so the request names it.
    #[must_use]
    pub fn with_model(mut self, model: impl Into<String>) -> CompletionOptions {
        self.model = model.into();
        self
    }

    /// Returns the caller-facing model name sent in the request.
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Returns the request settings the options apply.
    #[must_use]
    pub fn invocation(&self) -> &ModelInvocation {
        &self.invocation
    }

    /// Returns the sampling temperature, when one was set.
    #[must_use]
    pub fn temperature(&self) -> Option<Temperature> {
        self.invocation.temperature
    }

    /// Returns the maximum number of tokens to generate, when a cap was set.
    #[must_use]
    pub fn max_tokens(&self) -> Option<NonZeroU32> {
        self.invocation.max_tokens
    }

    /// Returns the `enable_thinking` switch, when it was set.
    #[must_use]
    pub fn thinking(&self) -> Option<bool> {
        self.invocation.thinking
    }
}

/// The run's model set: the five role bindings the run builds once,
/// shared unchanged by every section VM.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelSet {
    /// The bindings in the order they were pushed.
    bindings: Vec<ModelBinding>,
}

impl ModelSet {
    /// Returns bindings in the order they were pushed.
    #[must_use]
    pub fn bindings(&self) -> &[ModelBinding] {
        &self.bindings
    }

    /// Returns the binding for `alias`, if the set holds one.
    #[must_use]
    pub fn binding(&self, alias: &str) -> Option<&ModelBinding> {
        self.bindings.iter().find(|binding| binding.alias == alias)
    }

    /// Appends `binding` after every binding already in the set.
    pub fn push(&mut self, binding: ModelBinding) {
        self.bindings.push(binding);
    }
}
#[cfg(test)]
#[path = "options-tests.rs"]
mod tests;
