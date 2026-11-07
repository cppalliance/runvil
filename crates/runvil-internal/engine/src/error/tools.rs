//! The failures of a tool's binding and of a refused tool call: a schema
//! that did not build, frozen sets that broke a rule of the merge that
//! builds them, and a name the model or a script may not call.

use runvil_types::error::{ErrorCode, ErrorFact};
use runvil_types::tools::ToolId;

use crate::lua::{DiscoveryTool, OfferMode};

use super::{Error, failure, with_fact};

/// A failure with `code` whose text, `text`, is written for the model, so
/// it is both the message and the prose, about the tool name `name`.
fn refusal(code: ErrorCode, text: String, name: &str) -> Error {
    with_fact(Error::new(code, text.clone(), text), ErrorFact::Name, name)
}

/// A model-facing schema that did not build for the bound tool `alias`:
/// the `Name` fact is the alias, the `Reason` fact is the schema refusal's
/// own, and the message ends with the refusal's text. Constructed only by
/// the tool-scope preparation.
pub(crate) fn bind_schema(alias: &str, cause: &Error) -> Error {
    let mut error = with_fact(
        failure(
            ErrorCode::ToolSchemaInvalid,
            format!("model-facing schema build failure for tool alias {alias:?}"),
        ),
        ErrorFact::Name,
        alias,
    );
    if let Some(reason) = cause.facts.get(&ErrorFact::Reason) {
        error.facts.insert(ErrorFact::Reason, reason.clone());
    }
    error.with_cause(cause)
}

/// A section entry's frozen sets, or the render answer they are merged
/// from, that broke a rule of the merge that builds them, which fails the
/// entry's first loop round. `message` names what broke the rule and what
/// the rule requires.
pub(crate) fn tool_sets(message: String) -> Error {
    failure(ErrorCode::ToolSetsInvalid, message)
}

/// The tool `id` in `set`, as a broken merge rule's message names it, such
/// as `web/fetch of the web Pack in the Static Set`.
pub(crate) fn tool_in_set(id: &ToolId, set: OfferMode) -> String {
    format!("{id} of the {} Pack in the {set}", id.pack())
}

/// The model called `name`, outside both of the section entry's frozen
/// sets. The text names the Static Set's wire names, `static_aliases`, as
/// what they are; the offered-but-unscoped hint when `global_exists` says
/// the name is a catalog tool's wire name; and where a Dynamic Set tool is
/// reached from when `has_dynamic_set` says the section's Dynamic Set holds
/// a tool, which the caller reads from the frozen sets rather than from
/// `static_aliases`, since a catalog tool may take a discovery tool's wire
/// name while no Dynamic Set holds a tool.
///
/// This is the model tool loop's refusal alone: a script `tools.call`
/// resolves against the run's whole catalog and fails with
/// [`unbound_tool`] instead.
pub(crate) fn out_of_scope_tool(
    name: &str,
    global_exists: bool,
    static_aliases: &[String],
    has_dynamic_set: bool,
) -> Error {
    let hint = if global_exists {
        " (a catalog tool that was not offered in this section)"
    } else {
        ""
    };
    // A section whose Dynamic Set holds nothing freezes no discovery tool,
    // so naming one would point the model at a tool it cannot call.
    let dynamic_set = if has_dynamic_set {
        format!(
            "; a Dynamic Set tool is reached through {} and {}",
            DiscoveryTool::Search.wire_name(),
            DiscoveryTool::Call.wire_name()
        )
    } else {
        String::new()
    };
    refusal(
        ErrorCode::OutOfScopeTool,
        format!(
            "tool {name:?} is not in this section's scope; aliases this section offers: \
             {static_aliases:?}{hint}{dynamic_set}"
        ),
        name,
    )
}

/// A `tools.call` that named `name`, which is no adhoc tool and no tool in
/// the run's catalog, whose tool ids, in id order, are `ids`.
///
/// Script-initiated dispatch resolves a canonical id against the run's
/// whole catalog, not the section's frozen sets - the sets shape what the
/// model is offered, and the author's own code is not the model - so this
/// means the name is no catalog tool's id at all; a wire name is the
/// model's, never a script's.
pub(crate) fn unbound_tool(name: &str, ids: &[String]) -> Error {
    refusal(
        ErrorCode::UnboundTool,
        format!("tool {name:?} is not a tool in this run; catalog tools: {ids:?}"),
        name,
    )
}

/// A `tools.call` that named `name`, the catalog tool `id`, which only a
/// model may call: a Pack marks a tool model-only when a render gives its
/// arguments a script cannot know.
pub(crate) fn model_only_tool(name: &str, id: &str) -> Error {
    refusal(
        ErrorCode::ModelOnlyTool,
        format!(
            "tool {name:?} ({id}) is model-only: a render shapes its arguments for the model \
             that receives them, so a script cannot call it; offer it with tools.offer and let \
             the model call it"
        ),
        name,
    )
}
