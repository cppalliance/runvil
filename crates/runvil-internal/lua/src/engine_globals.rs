//! Engine globals installed into section VMs: `log`, `untrusted`, and the
//! `store` table in every one, and `ui` when the run has an
//! application-state snapshot. Each is a persistent global: installed once
//! per section and valid across every chunk the VM runs.

#[path = "engine_globals-store.rs"]
mod store;

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use mlua::{Lua, LuaSerdeExt, MultiValue, Value};
use runvil_types::emitter::Emitter;
use runvil_types::untrusted::GuardNonce;

use crate::error::{Error, QuotaExceeded, QuotaResource, Result};

pub(crate) use store::{install_store_table, route_store_to_shims};
pub use store::{run_store_op, store_error_message, store_probe_message};

/// Maximum number of Unicode scalar values accepted by `log`.
pub(crate) const LUA_LOG_CHARACTER_LIMIT: usize = 256;

/// Runs the persistent per-section `log(message)` Engine function.
fn log_checkpoint(
    emitter: &Emitter,
    section: &str,
    log_budget: &AtomicU32,
    log_byte_budget: &AtomicUsize,
    arguments: MultiValue,
) -> mlua::Result<()> {
    if arguments.len() != 1 {
        return Err(mlua::Error::external("log expects exactly one argument"));
    }
    // Spend one unit of the per-VM log budget before doing any work; an
    // exhausted budget refuses further checkpoints (lua 002).
    if log_budget
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_sub(1))
        .is_err()
    {
        return Err(mlua::Error::external(QuotaExceeded(
            QuotaResource::LogEvent,
        )));
    }
    let Some(Value::String(message)) = arguments.into_iter().next() else {
        return Err(mlua::Error::external("log message must be a UTF-8 string"));
    };
    let message = message
        .to_str()
        .map_err(|_| mlua::Error::external("log message must be a UTF-8 string"))?;
    if message.chars().count() > LUA_LOG_CHARACTER_LIMIT {
        return Err(mlua::Error::external(format!(
            "log message must be at most {LUA_LOG_CHARACTER_LIMIT} characters"
        )));
    }
    if message.chars().any(is_log_line_break_or_control) {
        return Err(mlua::Error::external(
            "log message must not contain newline or control characters",
        ));
    }
    // Enforce a cumulative byte ceiling in addition to the event count,
    // so many small events cannot emit unbounded total log volume
    // (lua 002).
    if log_byte_budget
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
            remaining.checked_sub(message.len())
        })
        .is_err()
    {
        return Err(mlua::Error::external(QuotaExceeded(QuotaResource::LogByte)));
    }
    emitter.lua(section, &message);
    Ok(())
}

/// Installs `log`. The closure captures owned strings and Arc clones, so it
/// outlives any single chunk.
pub(crate) fn install_log(
    lua: &Lua,
    emitter: &Emitter,
    section: &str,
    log_budget: &Arc<AtomicU32>,
    log_byte_budget: &Arc<AtomicUsize>,
) -> Result<()> {
    let section = section.to_owned();
    let emitter = emitter.clone();
    let log_budget = Arc::clone(log_budget);
    let log_byte_budget = Arc::clone(log_byte_budget);
    let log = lua
        .create_function(move |_, arguments: MultiValue| {
            log_checkpoint(&emitter, &section, &log_budget, &log_byte_budget, arguments)
        })
        .map_err(Error::lua)?;
    lua.globals().raw_set("log", log).map_err(Error::lua)
}

fn is_log_line_break_or_control(character: char) -> bool {
    character.is_control() || matches!(character, '\u{2028}' | '\u{2029}')
}

/// Installs `untrusted(s)`. Every wrap the VM performs shares one owned
/// clone of the run's nonce, and a non-string argument fails through mlua's
/// automatic type error.
pub(crate) fn install_untrusted(lua: &Lua, nonce: &GuardNonce) -> Result<()> {
    let nonce = nonce.clone();
    let untrusted = lua
        .create_function(move |_, s: String| Ok(nonce.wrap(&s)))
        .map_err(Error::lua)?;
    lua.globals()
        .raw_set("untrusted", untrusted)
        .map_err(Error::lua)
}

/// `ui()` snapshot conversion: a JSON null field reads as nil in author
/// code, never as the userdata NULL sentinel the serde bridge defaults
/// to - an unset snapshot field must simply be absent.
const UI_SNAPSHOT_OPTIONS: mlua::serde::SerializeOptions = mlua::serde::SerializeOptions::new()
    .serialize_none_to_null(false)
    .serialize_unit_to_null(false);

/// Installs `ui()`: each call converts the run's `snapshot` afresh into a
/// new table, JSON nulls reading as nil, so author code that mutates one
/// result never sees the mutation on the next call. The snapshot is the
/// application state the caller passed to `RunContext::with_ui` at run start;
/// a later change takes effect on the next run. A run started without a
/// snapshot never installs the global, so `ui` is absent there - not
/// stubbed.
///
/// The snapshot arrives shared: one run installs it into every section VM
/// it starts, and the closure serializes through the `Arc`, so no VM holds
/// its own copy of the JSON tree.
///
/// # Errors
/// Returns [`Error::LuaRuntime`] if the function or the global cannot be created.
pub fn install_ui(lua: &Lua, snapshot: Arc<serde_json::Value>) -> Result<()> {
    let snapshot = lua
        .create_function(move |lua, ()| lua.to_value_with(snapshot.as_ref(), UI_SNAPSHOT_OPTIONS))
        .map_err(Error::lua)?;
    lua.globals().raw_set("ui", snapshot).map_err(Error::lua)
}
