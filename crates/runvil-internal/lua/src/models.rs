//! The Lua `models` Engine global: `use` / `get` / `current`.
//!
//! The five roles arrive filled, when the run is built, in the shared
//! [`ModelSet`], and the table selects among them by role name. `models.use` takes a role
//! name or a model handle and stores the binding it selects, with its
//! optional sampling options applied, as the section's current model.
//! `models.get` inspects a role without selecting it, and `models.current`
//! returns the handle for the current model. The suspending `models.infer`,
//! `models.loop`, and `models.compact` are coroutine shims added to this
//! table afterward.
use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::Mutex;

use mlua::{Lua, MultiValue, Table, Value};

use runvil_model_client::model::{ModelBinding, ModelId, ModelInvocation, ModelSet, Temperature};
use runvil_types::models::ModelRole;

use crate::error::{Error, Result};

#[path = "models-userdata.rs"]
mod userdata;

pub(crate) use userdata::{LuaModelHandle, is_handle};

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

/// Builds the raw-model-id fallback's binding: a `models.get` label outside
/// the five roles, resolved as a raw gateway catalog model id with no
/// invocation overrides and the fallback context window.
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

/// The error every selection read or write raises when its lock is poisoned.
pub(crate) fn selection_poisoned() -> mlua::Error {
    mlua::Error::external("model selection lock was poisoned")
}

/// The Lua type name of `value`, as `type(value)` reports it: an integer is a
/// `number`.
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
pub(crate) struct UseOptions {
    temperature: Option<Temperature>,
    max_tokens: Option<NonZeroU32>,
}

impl UseOptions {
    /// Overrides the binding's invocation with the fields these options set.
    pub(crate) fn apply(self, binding: ModelBinding) -> ModelBinding {
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

/// Installs the `models` table into one section VM (H1 included: there is
/// one install path for every section).
///
/// `models.use(role_or_handle, options?)` takes a role name, which it looks up
/// in the shared [`ModelSet`], or a model handle, whose binding it uses as it
/// is. It applies the optional `temperature` / `max_tokens` table on top of
/// that binding, stores the result in `selection` as the section's current
/// model, and returns the handle. `models.get(label)` returns the handle for
/// a role without selecting it, and `models.current()` returns the handle
/// for the current model. A label outside the five roles is an error, and a
/// role the caller gave no model fails saying so.
///
/// `raw_ids` is the raw-model-id fallback, resolved whenever the caller
/// passes an application-state snapshot (`RunConfig::with_ui`): then
/// `models.get` resolves a label outside the five roles as a raw gateway
/// catalog model id, so the built-in chat prompt can run
/// `models.get(ui().selected_model)`. Refused, such a label is the usual
/// error.
///
/// The coroutine shim layer installs the suspending `models.infer`,
/// `models.loop`, and `models.compact`, because yield cannot cross the
/// Rust callback boundary.
///
/// # Errors
/// Returns [`Error::LuaRuntime`] if a Lua table or callback cannot be created or
/// installed.
pub(crate) fn install_models(
    lua: &Lua,
    globals: &Table,
    set: &Arc<ModelSet>,
    selection: &Arc<Mutex<Option<ModelBinding>>>,
    raw_ids: RawModelIds,
) -> Result<()> {
    let models = lua.create_table().map_err(Error::lua)?;

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
        .map_err(Error::lua)?;
    models.set("use", use_fn).map_err(Error::lua)?;

    let frozen = Arc::clone(set);
    let get_fn = lua
        .create_function(move |_, label: String| -> mlua::Result<LuaModelHandle> {
            // The raw-model-id fallback: with raw ids resolved, a label
            // outside the five roles resolves as a raw gateway catalog model
            // id under the fallback context window. Gateway ids include `/`,
            // `.`, and `:`, so the id's own validation is the only gate.
            let binding = match (find_role(&label), raw_ids) {
                (Some(role), _) => role_binding(&frozen, role)?,
                (None, RawModelIds::Resolved) => raw_gateway_binding(&label)?,
                (None, RawModelIds::Refused) => return Err(not_a_role("models.get", &label)),
            };
            Ok(LuaModelHandle::from_binding(binding))
        })
        .map_err(Error::lua)?;
    models.set("get", get_fn).map_err(Error::lua)?;

    let current = Arc::clone(selection);
    let current_fn = lua
        .create_function(move |_, ()| -> mlua::Result<LuaModelHandle> {
            let binding = current.lock().map_err(|_| selection_poisoned())?.clone();
            binding
                .map(LuaModelHandle::from_binding)
                .ok_or_else(|| mlua::Error::external("the caller supplied no model"))
        })
        .map_err(Error::lua)?;
    models.set("current", current_fn).map_err(Error::lua)?;

    globals.raw_set("models", models).map_err(Error::lua)
}
#[cfg(test)]
#[path = "models-tests.rs"]
mod tests;
