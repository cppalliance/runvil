//! Inspectable Lua userdata returned by `models.use`, `models.get`, and
//! `models.current`.
//!
//! The userdata is a pure value: it exposes a frozen [`ModelBinding`]'s
//! fields to Lua and nothing else. It has no methods, so there is exactly
//! one way to run a model: the `models` namespace functions, which take the
//! handle as their leading argument.

use mlua::{UserData, UserDataFields, Value};

use runvil_model_client::model::ModelBinding;

/// One model handle over a frozen `ModelBinding`. Its getters return raw
/// Lua numbers where the binding keeps validated types (`NonZeroU32`,
/// `Temperature`).
#[derive(Debug, Clone)]
pub(crate) struct LuaModelHandle {
    binding: ModelBinding,
}

impl LuaModelHandle {
    /// Builds a handle that holds the frozen `binding`.
    #[must_use]
    pub(crate) fn from_binding(binding: ModelBinding) -> Self {
        Self { binding }
    }

    /// Returns the frozen binding this handle holds.
    #[must_use]
    pub(crate) fn binding(&self) -> &ModelBinding {
        &self.binding
    }

    /// Returns the role label the handle was obtained by; the `name` and
    /// `label` fields both read it.
    #[must_use]
    fn name(&self) -> &str {
        self.binding.alias()
    }

    /// Returns the name of the bound model, without the server namespace
    /// its id also holds.
    #[must_use]
    fn model_id(&self) -> &str {
        self.binding.id().name()
    }

    /// Returns the description of the bound model.
    #[must_use]
    fn description(&self) -> &str {
        self.binding.description()
    }

    /// Returns the catalog context window size in tokens.
    #[must_use]
    fn context(&self) -> u32 {
        self.binding.context().get()
    }

    /// Returns the model's provider id, when the catalog names one.
    #[must_use]
    fn provider(&self) -> Option<String> {
        self.binding.provider().map(str::to_owned)
    }

    /// Returns the frozen thinking switch, when the role set one.
    #[must_use]
    fn thinking(&self) -> Option<bool> {
        self.binding.invocation().thinking
    }

    /// Returns the frozen sampling temperature, when one was set.
    #[must_use]
    fn temperature(&self) -> Option<f64> {
        self.binding
            .invocation()
            .temperature
            .map(runvil_model_client::model::Temperature::get)
    }

    /// Returns the frozen max generation tokens, when one was set.
    #[must_use]
    fn max_tokens(&self) -> Option<u32> {
        self.binding
            .invocation()
            .max_tokens
            .map(std::num::NonZeroU32::get)
    }
}

impl UserData for LuaModelHandle {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("name", |_, this| Ok(this.name().to_owned()));
        fields.add_field_method_get("label", |_, this| Ok(this.name().to_owned()));
        fields.add_field_method_get("model_id", |_, this| Ok(this.model_id().to_owned()));
        fields.add_field_method_get("description", |_, this| Ok(this.description().to_owned()));
        fields.add_field_method_get("context", |_, this| Ok(this.context()));
        fields.add_field_method_get("provider", |_, this| Ok(this.provider()));
        fields.add_field_method_get("thinking", |_, this| Ok(this.thinking()));
        fields.add_field_method_get("temperature", |_, this| Ok(this.temperature()));
        fields.add_field_method_get("max_tokens", |_, this| Ok(this.max_tokens()));
    }
}

/// Whether `value` is a model handle.
pub(crate) fn is_handle(value: &Value) -> bool {
    matches!(value, Value::UserData(userdata) if userdata.is::<LuaModelHandle>())
}
