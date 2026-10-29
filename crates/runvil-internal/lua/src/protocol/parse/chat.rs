//! The chat request parser: the handle a handle's `loop` names and the
//! section's list.

use mlua::Value;

use crate::messages::MessageList;
use crate::protocol::Request;

use super::{FieldFailure, call_handle};

/// Parses a `chat` request: the optional `handle`, which a handle's
/// `loop` sets from its receiver, and the `messages` list, which the loop
/// sets to the section's list.
///
/// The list validated each record as it added it, so the parse checks only
/// that `messages` is a list and clones its handle; the driver projects the
/// records without re-checking them. Only the Engine builds a chat
/// request, so a `messages` field that is not a list is a malformed yield.
/// The handle is checked first.
pub(super) fn parse_chat(table: &mlua::Table) -> std::result::Result<Request, FieldFailure> {
    let binding = call_handle(table, "models.loop")?;
    let list = match table.raw_get::<Value>("messages") {
        Ok(Value::UserData(userdata)) => userdata
            .borrow::<MessageList>()
            .map(|list| MessageList::clone(&list))
            .map_err(|_| FieldFailure::Malformed)?,
        Ok(_) | Err(_) => return Err(FieldFailure::Malformed),
    };
    Ok(Request::Chat { list, binding })
}
