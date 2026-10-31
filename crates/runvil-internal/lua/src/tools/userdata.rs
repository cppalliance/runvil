//! The tool object: one tool as Lua sees it.
//!
//! Presentation only: the userdata exposes a tool's `id`, its
//! `description`, its `parameters` schema, its `pack`, the Pack object
//! `tools.packs.get` returns, and whether only a model may call it, and
//! stands for its id wherever
//! `tools.offer` and `tools.call` take a tool. The object is frozen and
//! methodless, so `description` is always the text the Pack or the
//! `tools.adhoc` attrs declared, and each `parameters` read is a fresh
//! table of the schema as declared. Neither is ever in the untrusted
//! envelope. Unlike a model handle, which carries `infer` and `loop`, it
//! is invoked only through `tools.call(tool, arguments)`.

use mlua::{LuaSerdeExt, MetaMethod, UserData, UserDataFields, UserDataMethods, Value};
use runvil_types::tools::{ToolId, ToolInfo};
use serde_json::Value as Json;

use crate::bindings::ToolBinding;
use crate::packs::pack_object;

/// One tool as Lua sees it: `id`, `description`, `parameters`, `pack`, and
/// `model_only_call`, all read-only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LuaToolHandle {
    id: ToolId,
    description: String,
    parameters: Json,
    model_only_call: bool,
}

impl LuaToolHandle {
    /// Builds the object for an adhoc tool from its binding: the id, the
    /// declared description, the parameter schema, and whether only a
    /// model may call it.
    #[must_use]
    pub(crate) fn from_binding(binding: &ToolBinding) -> Self {
        Self {
            id: binding.id().clone(),
            description: binding.description().to_owned(),
            parameters: binding.schema().clone(),
            model_only_call: binding.model_only_call(),
        }
    }

    /// Builds the object for a catalog tool from its info: the id, the
    /// declared description, the parameter schema, and whether only a
    /// model may call it.
    #[must_use]
    pub(crate) fn from_info(info: &ToolInfo) -> Self {
        Self {
            id: info.id.clone(),
            description: info.description.clone(),
            parameters: info.parameters_schema.clone(),
            model_only_call: info.model_only_call,
        }
    }

    /// Returns the tool's canonical id, which the object stands for.
    #[must_use]
    pub(crate) fn id(&self) -> &ToolId {
        &self.id
    }
}

impl UserData for LuaToolHandle {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("id", |_, this| Ok(this.id.to_string()));
        fields.add_field_method_get("description", |_, this| Ok(this.description.clone()));
        // The conversion `json.decode` makes: lists carry the array
        // marker, so `json.encode` gives back `[]`, and nulls read
        // `json.null`.
        fields.add_field_method_get("parameters", |lua, this| lua.to_value(&this.parameters));
        // A script reads this before offering a tool whose call it cannot
        // make: the model's arguments, not the script's, fit the schema a
        // render gave it.
        fields.add_field_method_get("model_only_call", |_, this| Ok(this.model_only_call));
        fields.add_field_method_get("pack", |lua, this| {
            pack_object(lua, &this.id.pack().to_string())
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(
            MetaMethod::NewIndex,
            |_, _, (key, _): (String, Value)| -> mlua::Result<()> {
                Err(mlua::Error::external(format!(
                    "Tool objects are frozen: cannot assign field {key:?}"
                )))
            },
        );
    }
}
