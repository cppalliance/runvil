//! ListDir's arguments: what the call asks for, and what is refused before
//! any path is looked at.

use runvil_plugin::Error;
use serde_json::Value;

use crate::check::{invalid_pattern, refuse_blank, refuse_unknown};
use crate::matcher::Matcher;
use crate::spec::Definition;

/// What a call asks ListDir for.
#[derive(Debug)]
pub(super) struct Request<'a> {
    /// The directory to list, as the model wrote it.
    pub(super) target: &'a str,
    /// The `ignore_globs`, compiled, in the order the call gave them.
    pub(super) ignore: Vec<Matcher>,
}

/// Reads the call's arguments after the shared schema check.
///
/// The order is fixed: an undeclared key, then a blank target, then each
/// `ignore_globs` pattern in turn, so a call that breaks two rules always
/// hears about the same one.
///
/// # Errors
/// Returns an invalid-arguments [`Error`] naming the parameter. A
/// refused pattern is echoed with the rule it broke.
pub(super) fn parse<'a>(definition: &Definition, args: &'a Value) -> Result<Request<'a>, Error> {
    let tool = definition.wire_name.as_str();
    refuse_unknown(tool, &definition.parameters, args)?;
    let target = args
        .get("target_directory")
        .and_then(Value::as_str)
        .unwrap_or_default();
    refuse_blank(tool, "target_directory", target)?;
    let ignore = args
        .get("ignore_globs")
        .and_then(Value::as_array)
        .map(|patterns| patterns.iter().filter_map(Value::as_str))
        .into_iter()
        .flatten()
        .map(|pattern| {
            Matcher::new(pattern)
                .map_err(|error| invalid_pattern(tool, "ignore_globs", pattern, error))
        })
        .collect::<Result<Vec<_>, Error>>()?;
    Ok(Request { target, ignore })
}
