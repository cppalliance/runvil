//! The block guard's failure stash: the capture that records a guarded
//! block's raised value and raise-point traceback, and the take that reads
//! them back when the failure reaches the Engine.

use mlua::{Function, Lua, Table, Value};

use crate::error::{self, Error, Result, TRACEBACK_HEADING};
use crate::error_value::{raised_from, retained_error};

/// The registry key of the last value a guarded block raised, written by
/// the guard's `stash_failure` capture from the message handler at the
/// raise point and taken by [`take_failure`] when the failure reaches the
/// Engine.
const FAILURE_REGISTRY: &str = "runvil.impl_coro.failure";

/// The registry key of the traceback recorded beside the stashed failure:
/// the coroutine's stack at the raise point, before the guard's `xpcall`
/// unwinds the block's frames. The guard's re-raise happens after that
/// unwinding, so the traceback mlua appends to the re-raised error shows
/// only the guard's own frame; this one holds the author's.
const FAILURE_TRACEBACK_REGISTRY: &str = "runvil.impl_coro.failure_traceback";

/// Builds the shim chunk's `stash_failure(failure)` capture: records the
/// block's raised value and its raise-point traceback for [`take_failure`].
pub(super) fn stash_failure_capture(lua: &Lua) -> Result<Function> {
    lua.create_function(|lua, failure: Value| {
        // Called from the guard's message handler, so the failing
        // frames are still on this coroutine's stack: level 1 starts
        // the traceback at the handler, above this capture's own frame.
        let traceback = lua.traceback(None, 1)?;
        lua.set_named_registry_value(FAILURE_TRACEBACK_REGISTRY, traceback)?;
        lua.set_named_registry_value(FAILURE_REGISTRY, failure)
    })
    .map_err(error::lua)
}

/// What the guard stashed for the last failure of a guarded block.
#[derive(Debug, Default)]
pub(crate) struct StashedFailure {
    /// The raised value read back as the universal error it stands for
    /// when it is a structured error table built on this VM: the error a
    /// table was rendered from, or the one its fields hold; `None` when it
    /// was a plain string, an author's own table, or a Rust callback's
    /// wrapped failure.
    pub(crate) raised: Option<Error>,
    /// The raised value itself when it was a table, which identifies the
    /// raise of one particular table.
    table: Option<Table>,
    /// The coroutine's traceback at the raise point, `stack traceback:`
    /// heading included, when the handler recorded one.
    traceback: Option<String>,
}

impl StashedFailure {
    /// Whether the block raised `table` itself, not merely a table with
    /// the same contents.
    pub(crate) fn raised_table_is(&self, table: &Table) -> bool {
        self.table
            .as_ref()
            .is_some_and(|raised| raised.to_pointer() == table.to_pointer())
    }

    /// Restores the raise as the line mapper should read it onto a
    /// Lua-raised failure.
    ///
    /// mlua appends the coroutine's traceback to a runtime error when the
    /// coroutine dies, but the guard's re-raise is what kills it, so that
    /// traceback shows the guard's frame and nothing of the block's. When
    /// this stash recorded the real one, the appended tail is replaced with
    /// it, so the line mapper sees the author's frames. A raised error
    /// table renders as its prose, so the text before the tail becomes the
    /// message of the error the table stands for. A Rust callback's
    /// wrapped failure has its own traceback and is left untouched.
    pub(crate) fn restore_traceback<'e>(
        &self,
        error: &'e mlua::Error,
    ) -> std::borrow::Cow<'e, mlua::Error> {
        let mlua::Error::RuntimeError(message) = error else {
            return std::borrow::Cow::Borrowed(error);
        };
        if self.traceback.is_none() && self.raised.is_none() {
            return std::borrow::Cow::Borrowed(error);
        }
        let (head, tail) = message
            .rfind(TRACEBACK_HEADING)
            .map_or((message.as_str(), ""), |at| message.split_at(at));
        let head = self
            .raised
            .as_ref()
            .map_or(head, |raised| raised.message.as_str());
        let restored = match &self.traceback {
            Some(traceback) => format!("{head}\n{traceback}"),
            None => format!("{head}{tail}"),
        };
        std::borrow::Cow::Owned(mlua::Error::RuntimeError(restored))
    }
}

/// Takes what the guard stashed for the last failure of a guarded block.
/// When a failure was stashed, both slots are cleared, so a later failure
/// never sees a stale value; when none was, the result is the default
/// (nothing raised, no traceback).
///
/// # Errors
/// Returns a `LuaRuntime` failure if a registry slot cannot be read or
/// cleared.
pub(crate) fn take_failure(lua: &Lua) -> Result<StashedFailure> {
    let failure: Value = lua
        .named_registry_value(FAILURE_REGISTRY)
        .map_err(error::lua)?;
    let traceback: Value = lua
        .named_registry_value(FAILURE_TRACEBACK_REGISTRY)
        .map_err(error::lua)?;
    if matches!(failure, Value::Nil) {
        return Ok(StashedFailure::default());
    }
    lua.set_named_registry_value(FAILURE_REGISTRY, Value::Nil)
        .map_err(error::lua)?;
    lua.set_named_registry_value(FAILURE_TRACEBACK_REGISTRY, Value::Nil)
        .map_err(error::lua)?;
    let traceback = match traceback {
        Value::String(text) => Some(text.to_str().map_err(error::lua)?.to_owned()),
        _ => None,
    };
    let raised = match retained_error(lua, &failure).map_err(error::lua)? {
        Some(retained) => Some(retained),
        None => raised_from(lua, &failure).map_err(error::lua)?,
    };
    let table = match failure {
        Value::Table(table) => Some(table),
        _ => None,
    };
    Ok(StashedFailure {
        raised,
        table,
        traceback,
    })
}
