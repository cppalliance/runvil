//! The Lua `models` Engine global: `use` / `get` / `current`.
//!
//! The run builds the shared [`ModelSet`] once, holding all five roles or,
//! when the caller supplied no model, none, and the table selects among them
//! by role name; a role read from the empty set fails saying so. `models.use`
//! takes a role name or a model handle and stores the binding it selects,
//! with its optional sampling options applied, as the section's current
//! model. `models.get` inspects a role without selecting it, and
//! `models.current` returns the handle for the current model. A handle has
//! no methods, so the `models` namespace functions, which take it as their
//! leading argument, are the one way to run a model. The suspending
//! `models.infer`, `models.loop`, and `models.compact` are coroutine shims
//! added to this table afterward, because yield cannot cross the Rust
//! callback boundary.
use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::Mutex;

use mlua::{Lua, MultiValue, Table, UserData, UserDataFields, Value};

use runvil_model_client::model::{ModelBinding, ModelId, ModelInvocation, ModelSet, Temperature};
use runvil_types::models::ModelRole;

use crate::error::{self, Result};

/// One model handle over a frozen `ModelBinding`, the inspectable value
/// `models.use`, `models.get`, and `models.current` return: it exposes the
/// binding's fields to Lua and nothing else. Its getters return raw
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

/// Whether `models.get` resolves a label outside the five roles as a raw
/// gateway catalog model id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RawModelIds {
    /// A label outside the five roles is an error.
    Refused,
    /// A label outside the five roles resolves as a raw gateway catalog model id.
    Resolved,
}

/// The context window a raw-model-id binding records: the fallback has no
/// catalog metadata, so a conservative default keeps the compactor
/// precheck safe rather than refusing the model.
const RAW_ID_CONTEXT: NonZeroU32 = match NonZeroU32::new(8192) {
    Some(value) => value,
    None => unreachable!(),
};

/// Builds the [`RawModelIds::Resolved`] binding for `alias`: no invocation
/// overrides and [`RAW_ID_CONTEXT`].
fn raw_gateway_binding(alias: &str) -> mlua::Result<ModelBinding> {
    let id = ModelId::gateway(alias).map_err(|error| {
        mlua::Error::external(format!("models.get model id {alias:?} is invalid: {error}"))
    })?;
    Ok(ModelBinding::new(
        alias,
        alias,
        id,
        ModelInvocation::new(),
        RAW_ID_CONTEXT,
    ))
}

/// The failure of a poisoned model selection lock.
pub(crate) const SELECTION_POISONED: &str = "model selection lock was poisoned";

/// The error a selection read or write inside a Lua callback raises when
/// its lock is poisoned.
#[track_caller]
pub(crate) fn selection_poisoned() -> mlua::Error {
    mlua::Error::external(error::make_internal_error(SELECTION_POISONED))
}

/// The type name the `models.use` target and `set_selection` refusals give
/// `value`: mlua's type name, except that an integer is a `number`, so
/// `json.null` reads `lightuserdata`.
pub(crate) fn lua_type_name(value: &Value) -> &'static str {
    match value {
        Value::Integer(_) | Value::Number(_) => "number",
        other => other.type_name(),
    }
}

/// The five role names as a sentence tail: `default, strong, small, fast or
/// creative`.
fn role_list() -> String {
    let names: Vec<&str> = ModelRole::ALL.iter().map(|role| role.as_str()).collect();
    match names.split_last() {
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
        None => String::new(),
    }
}

/// The role `label` names, if it names one.
fn find_role(label: &str) -> Option<ModelRole> {
    ModelRole::ALL
        .into_iter()
        .find(|role| role.as_str() == label)
}

/// The `function` rejection of a label that is not a role, listing the five.
fn not_a_role(function: &str, label: &str) -> mlua::Error {
    mlua::Error::external(format!(
        "{function} label {label:?} is not a model role: expected {}",
        role_list()
    ))
}

/// The binding that fills `role`, or the error saying the caller supplied
/// no model for it.
fn role_binding(set: &ModelSet, role: ModelRole) -> mlua::Result<ModelBinding> {
    set.binding(role.as_str()).cloned().ok_or_else(|| {
        mlua::Error::external(format!(
            "the caller supplied no model for the {} role",
            role.as_str()
        ))
    })
}
/// The sampling options a `models.use` call sets on its selection. An
/// omitted field keeps the role's own value.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct UseOptions {
    temperature: Option<Temperature>,
    max_tokens: Option<NonZeroU32>,
}

impl UseOptions {
    /// Overrides the binding's invocation with the fields these options set.
    fn apply(self, binding: ModelBinding) -> ModelBinding {
        let mut invocation = binding.invocation().clone();
        invocation.temperature = self.temperature.or(invocation.temperature);
        invocation.max_tokens = self.max_tokens.or(invocation.max_tokens);
        binding.with_invocation(invocation)
    }
}

/// A `models.use` option rejection stating required versus actual.
fn invalid_option(option: &str, required: &str, actual: impl std::fmt::Display) -> mlua::Error {
    mlua::Error::external(format!(
        "models.use option {option} must be {required}, got {actual}"
    ))
}

/// Decodes `temperature`: a Lua integer or number, bounded only by
/// [`Temperature::new`].
fn decode_temperature(value: &Value) -> mlua::Result<Temperature> {
    let number = match value {
        Value::Number(number) => Ok(*number),
        Value::Integer(number) =>
        {
            #[expect(
                clippy::cast_precision_loss,
                reason = "any magnitude that loses precision fails the [0.0, 2.0] check"
            )]
            Ok(*number as f64)
        }
        other => Err(invalid_option("temperature", "a number", other.type_name())),
    }?;
    Temperature::new(number).map_err(|error| {
        if number.is_finite() {
            mlua::Error::external(format!("models.use option {error}"))
        } else {
            invalid_option("temperature", "finite", number)
        }
    })
}

/// Decodes `max_tokens`: a positive integer that fits a [`NonZeroU32`],
/// given as a Lua integer or an integral float.
fn decode_max_tokens(value: &Value) -> mlua::Result<NonZeroU32> {
    let count = match value {
        Value::Integer(number) => u32::try_from(*number).ok(),
        Value::Number(number)
            if number.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(number) =>
        {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "integral and range checked against u32"
            )]
            Some(*number as u32)
        }
        _ => None,
    };
    count.and_then(NonZeroU32::new).ok_or_else(|| {
        let actual = match value {
            Value::Integer(number) => number.to_string(),
            Value::Number(number) => number.to_string(),
            other => other.type_name().to_owned(),
        };
        invalid_option("max_tokens", "an integer in [1, 4294967295]", actual)
    })
}

/// Where `models.use` checks one option entry: non-string keys first,
/// grouped by type because their rejection names only the type, then names
/// bytewise. `pairs` walks in the state's hash-seed order, so checking in
/// this order makes the first rejection a function of the table's contents.
fn option_check_order(key: &Value) -> (bool, Vec<u8>) {
    match key {
        Value::String(name) => (true, name.as_bytes().to_vec()),
        other => (false, other.type_name().as_bytes().to_vec()),
    }
}

/// Decodes the `models.use` arguments after the label: an optional options
/// table and nothing more.
fn parse_use_options(options: Value, rest: &MultiValue) -> mlua::Result<UseOptions> {
    if !rest.is_empty() {
        return Err(mlua::Error::external(format!(
            "models.use takes at most 2 arguments, got {}",
            rest.len() + 2
        )));
    }
    let table = match options {
        Value::Nil => return Ok(UseOptions::default()),
        Value::Table(table) => table,
        other => {
            return Err(mlua::Error::external(format!(
                "models.use options must be a table, got {}",
                other.type_name()
            )));
        }
    };
    let mut entries = table
        .pairs::<Value, Value>()
        .collect::<mlua::Result<Vec<_>>>()?;
    entries.sort_by_cached_key(|(key, _)| option_check_order(key));
    let mut parsed = UseOptions::default();
    for (key, value) in entries {
        let Value::String(key) = key else {
            return Err(mlua::Error::external(format!(
                "models.use option names must be strings, got {}",
                key.type_name()
            )));
        };
        match key.to_string_lossy().as_str() {
            "temperature" => parsed.temperature = Some(decode_temperature(&value)?),
            "max_tokens" => parsed.max_tokens = Some(decode_max_tokens(&value)?),
            other => {
                return Err(mlua::Error::external(format!(
                    "models.use option {other:?} is unknown: expected temperature or max_tokens"
                )));
            }
        }
    }
    Ok(parsed)
}

/// Installs the `models` table the module doc describes into one section VM;
/// every section, H1 included, installs it through this one path.
/// `models.use` stores its selection in `selection`, and `raw_ids` is the
/// [`RawModelIds`] fallback `models.get` applies to a label outside the five
/// roles.
///
/// # Errors
/// Returns a `LuaRuntime` failure if a Lua table or callback cannot be created or
/// installed.
pub(crate) fn install_models(
    lua: &Lua,
    globals: &Table,
    set: &Arc<ModelSet>,
    selection: &Arc<Mutex<Option<ModelBinding>>>,
    raw_ids: RawModelIds,
) -> Result<()> {
    let models = lua.create_table().map_err(error::make_lua_error)?;

    let frozen = Arc::clone(set);
    let current = Arc::clone(selection);
    let use_fn = lua
        .create_function(
            move |_,
                  (target, options, rest): (Value, Value, MultiValue)|
                  -> mlua::Result<LuaModelHandle> {
                let binding = match &target {
                    Value::String(text) => {
                        let label = text.to_string_lossy();
                        let role =
                            find_role(&label).ok_or_else(|| not_a_role("models.use", &label))?;
                        role_binding(&frozen, role)?
                    }
                    Value::UserData(userdata) if userdata.is::<LuaModelHandle>() => {
                        userdata.borrow::<LuaModelHandle>()?.binding().clone()
                    }
                    other => {
                        return Err(mlua::Error::external(format!(
                            "models.use takes a model role name or a model handle, got {}",
                            lua_type_name(other)
                        )));
                    }
                };
                let options = parse_use_options(options, &rest)?;
                let binding = options.apply(binding);
                *current.lock().map_err(|_| selection_poisoned())? = Some(binding.clone());
                Ok(LuaModelHandle::from_binding(binding))
            },
        )
        .map_err(error::make_lua_error)?;
    models.set("use", use_fn).map_err(error::make_lua_error)?;

    let frozen = Arc::clone(set);
    let get_fn = lua
        .create_function(move |_, label: String| -> mlua::Result<LuaModelHandle> {
            // Gateway ids include `/`, `.`, and `:`, so the id's own
            // validation is the only gate.
            let binding = match (find_role(&label), raw_ids) {
                (Some(role), _) => role_binding(&frozen, role)?,
                (None, RawModelIds::Resolved) => raw_gateway_binding(&label)?,
                (None, RawModelIds::Refused) => return Err(not_a_role("models.get", &label)),
            };
            Ok(LuaModelHandle::from_binding(binding))
        })
        .map_err(error::make_lua_error)?;
    models.set("get", get_fn).map_err(error::make_lua_error)?;

    let current = Arc::clone(selection);
    let current_fn = lua
        .create_function(move |_, ()| -> mlua::Result<LuaModelHandle> {
            let binding = current.lock().map_err(|_| selection_poisoned())?.clone();
            binding
                .map(LuaModelHandle::from_binding)
                .ok_or_else(|| mlua::Error::external("the caller supplied no model"))
        })
        .map_err(error::make_lua_error)?;
    models
        .set("current", current_fn)
        .map_err(error::make_lua_error)?;

    globals
        .raw_set("models", models)
        .map_err(error::make_lua_error)
}
#[cfg(test)]
#[path = "models-tests.rs"]
mod tests;
