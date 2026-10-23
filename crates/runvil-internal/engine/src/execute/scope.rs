//! Tool-scope schema/dispatch preparation for the model-visible tool set.

use std::collections::BTreeMap;

use runvil_model_client::detail;
use runvil_types::emitter::Emitter;
use runvil_types::event::lifecycle;

use crate::lua::{Advertised, RenderedTools, ToolBinding};
use crate::model::ToolSchema;
use crate::{Error, Result};

/// Builds the section's rendered tool list under the validation boundary
/// pair, reported through the chain's `emitter` under `section`.
///
/// # Errors
/// Returns the schema-construction error of [`prepare_scoped_tools`].
pub(super) fn prepare_effective_scope(
    bindings: &[ToolBinding],
    adhoc_schemas: &[ToolSchema],
    emitter: &Emitter,
    section: &str,
) -> Result<RenderedTools> {
    emitter.report(section, lifecycle::TOOL_SCOPE_VALIDATION_STARTED);
    let result = prepare_scoped_tools(bindings, adhoc_schemas);
    emitter.report(
        section,
        if result.is_ok() {
            lifecycle::TOOL_SCOPE_VALIDATION_SUCCEEDED
        } else {
            lifecycle::TOOL_SCOPE_VALIDATION_FAILED
        },
    );
    result
}

pub(super) fn prepare_scoped_tools(
    bindings: &[ToolBinding],
    adhoc_schemas: &[ToolSchema],
) -> Result<RenderedTools> {
    let mut schemas = Vec::with_capacity(bindings.len() + adhoc_schemas.len());
    let mut dispatch = BTreeMap::new();
    for binding in bindings {
        // An adhoc alias may equal an offered tool's wire name; the adhoc
        // tool wins.
        if adhoc_schemas
            .iter()
            .any(|schema| schema.name() == binding.alias())
        {
            continue;
        }
        // Build every advertised schema through the validated constructor,
        // so an unusable wire name or a non-object JSON Schema is refused here
        // rather than sent to the model.
        let schema = detail::tool_schema_new(
            binding.alias().to_owned(),
            binding.description().to_owned(),
            binding.schema().clone(),
        )
        .map_err(|error| Error::BindSchema {
            alias: binding.alias().to_owned(),
            source: Box::new(error),
        })?;
        schemas.push(schema);
        dispatch.insert(
            binding.alias().to_owned(),
            Some(Advertised {
                id: binding.id().clone(),
                vendor: binding.vendor().map(str::to_owned),
            }),
        );
    }
    // Adhoc tools are prompt-author Lua functions with no live implementation;
    // the `tool_call` arm answers their calls on the section VM. The alias
    // was validated at `tools.offer_local` registration.
    for schema in adhoc_schemas {
        dispatch.insert(schema.name().to_owned(), None);
        schemas.push(schema.clone());
    }
    Ok(RenderedTools { schemas, dispatch })
}
