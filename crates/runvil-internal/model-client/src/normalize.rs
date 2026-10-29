//! The neutral reply checks every caller-built completion passes.
//!
//! A tool call must have a nonblank id and name and object arguments, the
//! calls in one turn must have distinct ids, and a turn with no product is
//! an `EmptyReply`-kind failure. The validating constructors in
//! `client/wire-canned.rs` run these checks and refuse the parts with an
//! `InvalidParts`-kind failure, so a completion is built only from parts a
//! wire decoder would accept.

use std::collections::HashSet;

use serde_json::Value;

use crate::Result;
use crate::model::CompletionError;

/// The empty-reply error for a turn with no product.
pub(crate) fn empty_reply_error() -> CompletionError {
    CompletionError::empty_reply(false)
}

/// Refuses a blank tool-call id.
pub(crate) fn check_call_id(id: &str) -> Result<()> {
    if id.trim().is_empty() {
        return Err(CompletionError::invalid_parts("tool call id was blank"));
    }
    Ok(())
}

/// Refuses a blank tool-call name.
pub(crate) fn check_call_name(name: &str) -> Result<()> {
    if name.trim().is_empty() {
        return Err(CompletionError::invalid_parts("tool call name was blank"));
    }
    Ok(())
}

/// Refuses tool-call arguments that are not a JSON object, the shape tools
/// accept, and hands back the object it proved.
///
/// The check cannot `unwrap` the map out of the value, because the
/// workspace denies `unwrap_used` and `expect_used`, so it matches the
/// object arm and returns the map. A caller that needs the map then holds
/// it without a second parse.
pub(crate) fn checked_call_arguments(arguments: Value) -> Result<serde_json::Map<String, Value>> {
    match arguments {
        Value::Object(map) => Ok(map),
        _ => Err(CompletionError::invalid_parts(
            "tool call arguments were not a JSON object",
        )),
    }
}

/// Records `id` in `seen`, refusing an id another call in the turn has.
pub(crate) fn check_unique_call_id<'a>(seen: &mut HashSet<&'a str>, id: &'a str) -> Result<()> {
    if !seen.insert(id) {
        return Err(CompletionError::invalid_parts(format!(
            "duplicate tool call id {id:?} within one turn"
        )));
    }
    Ok(())
}

#[cfg(test)]
#[path = "normalize-tests.rs"]
mod tests;
