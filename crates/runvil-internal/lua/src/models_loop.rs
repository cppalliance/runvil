//! The rules of `models.loop` and `models.compact`, run in Rust behind the
//! loop shim.
//!
//! `models.loop` and a model handle's `loop` are one-line entries over a
//! Lua trampoline in `__impl_coro.lua` over `loop_begin`, which this
//! module builds, and the step closure each call returns, which owns that
//! call's machine. `models.compact` is a third entry over the same
//! trampoline and `compact_begin`, whose step settles one compactor call
//! the way an overflowed round's is settled. A Rust function called from
//! Lua cannot yield, and cannot call a Lua function that may yield, so
//! each step returns to the trampoline with one action, named by its tag:
//! `"yield"` a request and
//! pass every resume value back; call a `"handler"` between
//! `enter_local_handler()` and `leave_local_handler()`, or the
//! `"compactor"`, under the raw `pcall` and pass `ok` and the first
//! result back; `"raise"` a value with `error(value, 0)`; or `"return"`
//! nil. This adapter reads the values the trampoline passes into the
//! machine's typed input, steps the machine, and turns its next step into
//! that action.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use mlua::{AnyUserData, Function, Lua, LuaSerdeExt, MultiValue, Table, Value};
use runvil_types::metrics::ToolCallEvent;

use crate::error::{Error, Result};
use crate::error_value::{ErrorKind, error_table, normalized};
use crate::hardening::InstructionBudget;
use crate::messages::MessageList;
use crate::protocol::ChatResult;

mod machine;

use machine::{Input, Machine, Phase, Then, raised, settle};

/// `models.loop`'s refusal of a model handle in the compactor's place.
const HANDLE_FIRST: &str = "models.loop takes (compactor?); \
                            call handle:loop(compactor?) to run on a model handle";

/// `models.loop`'s refusal past one argument.
const LOOP_ARITY: &str = "models.loop takes (compactor?)";

/// A handle's `loop` called without a model handle as its receiver.
const NO_RECEIVER: &str = "call loop on a model handle with a colon: handle:loop(compactor?)";

/// A handle's `loop` past its receiver and one argument.
const METHOD_ARITY: &str = "handle:loop takes (compactor?)";

/// Either entry called while a loop or a compactor runs in the same
/// section.
const LOOP_RUNNING: &str = "a loop or a compactor is already running in this section; \
                            ask a side question with models.infer(prompt)";

/// `models.compact`'s refusal past two arguments.
const COMPACT_ARITY: &str = "models.compact takes (compactor, reason)";

/// `models.compact` called while a loop or a compactor runs in the same
/// section.
const COMPACT_RUNNING: &str =
    "models.compact cannot start while a loop or a compactor is running in this section";

/// Builds `loop_begin`, the shim chunk's argument before `compact_begin`.
///
/// `loop_begin(entry, ...)` takes `false` for `models.loop` or `true` for
/// a handle's `loop`, then that entry's arguments, and returns the
/// call's step closure followed by the first action's tag and values;
/// or `nil`, `"raise"`, and the argument error. Every call runs over the
/// section's list, read from the `_G` guard's `messages` slot. The step
/// closure, made with `create_function_mut`, owns the call's
/// [`machine::Machine`]; it takes the last yield's resume values, or the
/// last call's `ok` and first result, and returns the next action's tag
/// and values. mlua
/// refuses a recursive call of such a closure, which cannot happen,
/// because the step returns before every yield and every call. Every
/// error the loop raises is a `"raise"` action, so it is raised from the
/// trampoline's Lua frame; only a crate bug or an exhausted Lua heap
/// makes either function fail.
///
/// `max_tool_iterations` is the round cap, `compactors` the table that
/// `compactors.fail` is read from at each call, `loop_running` the VM's
/// flag that is set while one of its loops runs, and `budget` the VM's
/// instruction budget, whose cancel flag the step reads where the shim
/// called `cancel_requested()`.
///
/// # Errors
/// Returns [`Error::Lua`] if the function cannot be created.
pub(crate) fn loop_begin(
    lua: &Lua,
    max_tool_iterations: usize,
    compactors: Table,
    loop_running: &Arc<AtomicBool>,
    budget: &InstructionBudget,
) -> Result<Function> {
    let running = Arc::clone(loop_running);
    let budget = budget.clone();
    lua.create_function(move |lua, args: MultiValue| {
        begin(
            lua,
            args,
            max_tool_iterations,
            &compactors,
            &running,
            &budget,
        )
    })
    .map_err(Error::lua)
}

/// `loop_begin`'s body: the entry flag, then that entry's argument checks
/// in Functional Specification's order, then the refusal while `running`
/// or the list's `compacting` flag is set, then the machine over the
/// section's list and its step closure,
/// whose body is `act(lua, machine.step(read_input(lua, machine.phase(),
/// values, &budget)?))`, then the first action. `running` is set only once
/// nothing here can fail and the first action leaves the machine running;
/// the step clears it when the machine is done, before it builds that
/// action, and whenever it fails. A failed step also clears the list's
/// `compacting` flag, which an overflow sets before the step builds the
/// compactor's action.
fn begin(
    lua: &Lua,
    args: MultiValue,
    max_rounds: usize,
    compactors: &Table,
    running: &Arc<AtomicBool>,
    budget: &InstructionBudget,
) -> mlua::Result<MultiValue> {
    let mut args = args.into_iter();
    let Some(Value::Boolean(method)) = args.next() else {
        return Err(malformed("loop_begin needs its entry flag first"));
    };
    let args: Vec<Value> = args.collect();
    let receiver = args.first().is_some_and(crate::models::is_handle);
    let (handle, mut args) = if method {
        if !receiver {
            return refuse(lua, NO_RECEIVER);
        }
        if args.len() > 2 {
            return refuse(lua, METHOD_ARITY);
        }
        let mut args = args.into_iter();
        (args.next(), args)
    } else {
        if receiver {
            return refuse(lua, HANDLE_FIRST);
        }
        if args.len() > 1 {
            return refuse(lua, LOOP_ARITY);
        }
        (None, args.into_iter())
    };
    let compactor = match args.next().unwrap_or(Value::Nil) {
        Value::Nil => compactors.get::<Value>("fail")?,
        function @ Value::Function(_) => function,
        other => {
            let message = format!(
                "compactor must be a function, got {}",
                lua_type_name(&other)
            );
            return refuse(lua, &message);
        }
    };
    let messages = crate::globals::messages(lua).map_err(mlua::Error::external)?;
    let list = MessageList::clone(&*messages.borrow::<MessageList>()?);
    if running.load(Ordering::Relaxed) || list.compacting() {
        return refuse(lua, LOOP_RUNNING);
    }
    let messages = Value::UserData(messages);
    let flagged = list.clone();
    let (mut machine, first) = Machine::begin(max_rounds, list, messages, handle, compactor);
    let started = !matches!(machine.phase(), Phase::Done);
    let budget = budget.clone();
    let cleared = Arc::clone(running);
    let step = lua.create_function_mut(move |lua, values: MultiValue| {
        let then =
            read_input(lua, machine.phase(), values, &budget).map(|input| machine.step(input));
        if matches!(machine.phase(), Phase::Done) {
            cleared.store(false, Ordering::Relaxed);
        }
        let action = then.and_then(|then| act(lua, then));
        if action.is_err() {
            cleared.store(false, Ordering::Relaxed);
            flagged.set_compacting(false);
        }
        action
    })?;
    let mut action = act(lua, first)?;
    action.push_front(Value::Function(step));
    if started {
        running.store(true, Ordering::Relaxed);
    }
    Ok(action)
}

/// Builds `compact_begin`, the shim chunk's argument after `loop_begin`.
///
/// `compact_begin(compactor, reason)` checks its arity, then the
/// compactor, then the reason, refuses while `loop_running` or the
/// section list's `compacting` flag is set, snapshots the list, and sets
/// that flag. It returns the step closure followed by the `"compactor"`
/// action with the compactor and the reason; or `nil`, `"raise"`, and the
/// refusal. The step takes the compactor's raw `pcall` outcome, settles it
/// as an overflowed round's is settled but under the structural rules
/// only, and returns the `"return"` or `"raise"` action. `budget` is the
/// VM's instruction budget, whose cancel flag the step reads.
///
/// # Errors
/// Returns [`Error::Lua`] if the function cannot be created.
pub(crate) fn compact_begin(
    lua: &Lua,
    loop_running: &Arc<AtomicBool>,
    budget: &InstructionBudget,
) -> Result<Function> {
    let running = Arc::clone(loop_running);
    let budget = budget.clone();
    lua.create_function(move |lua, args: MultiValue| compact(lua, args, &running, &budget))
        .map_err(Error::lua)
}

/// `compact_begin`'s body. The flag is set only once nothing here can
/// fail, and the step clears it before it builds its action.
fn compact(
    lua: &Lua,
    args: MultiValue,
    running: &AtomicBool,
    budget: &InstructionBudget,
) -> mlua::Result<MultiValue> {
    let args: Vec<Value> = args.into_iter().collect();
    if args.len() > 2 {
        return refuse(lua, COMPACT_ARITY);
    }
    let mut args = args.into_iter();
    let compactor = args.next().unwrap_or(Value::Nil);
    let reason = args.next().unwrap_or(Value::Nil);
    for (name, value, wanted) in [
        ("compactor", &compactor, "function"),
        ("reason", &reason, "string"),
    ] {
        if value.type_name() != wanted {
            let message = format!("{name} must be a {wanted}, got {}", lua_type_name(value));
            return refuse(lua, &message);
        }
    }
    let messages = crate::globals::messages(lua).map_err(mlua::Error::external)?;
    let list = MessageList::clone(&*messages.borrow::<MessageList>()?);
    if running.load(Ordering::Relaxed) || list.compacting() {
        return refuse(lua, COMPACT_RUNNING);
    }
    let mut snapshot = Some(list.snapshot());
    let budget = budget.clone();
    let compacted = list.clone();
    let step = lua.create_function_mut(move |lua, (ok, value): (Value, Value)| {
        let Some(snapshot) = snapshot.take() else {
            return Err(malformed(
                "the models.compact step ran after its compaction ended",
            ));
        };
        let outcome = if truthy(&ok) { Ok(value) } else { Err(value) };
        let then = settle(&compacted, snapshot, outcome, budget.is_cancelled(), None)
            .map_or_else(|refusal| refusal, |_| Then::Return);
        act(lua, then)
    })?;
    let tag = Value::String(lua.create_string("compactor")?);
    let action = MultiValue::from_vec(vec![Value::Function(step), tag, compactor, reason]);
    list.set_compacting(true);
    Ok(action)
}

/// `loop_begin`'s answer to a refused argument: no step closure, then the
/// `"raise"` action with a new `lua`-kind error.
fn refuse(lua: &Lua, message: &str) -> mlua::Result<MultiValue> {
    let mut action = act(lua, Then::RaiseNew(raised(ErrorKind::Lua, message)))?;
    action.push_front(Value::Nil);
    Ok(action)
}

/// Reads the values the trampoline passed as the input `phase` awaits.
/// A failed envelope's value goes through [`envelope_failure`]. A `chat`
/// answer's [`ChatResult`] is taken out of the userdata it resumed as, so
/// the machine gets the scheduler's own value. A raw `pcall`'s outcome is
/// read beside the run's cancel flag.
fn read_input(
    lua: &Lua,
    phase: &Phase<Value>,
    values: MultiValue,
    budget: &InstructionBudget,
) -> mlua::Result<Input<Value>> {
    let mut values = values.into_iter();
    let mut next = || values.next().unwrap_or(Value::Nil);
    let ok = truthy(&next());
    let value = next();
    match phase {
        Phase::Compacting(_) | Phase::Handling => Ok(Input::Called {
            outcome: if ok { Ok(value) } else { Err(value) },
            cancelled: budget.is_cancelled(),
        }),
        Phase::Chatting => Ok(Input::Answered(answer(lua, ok, value, |round| {
            lua.unpack::<AnyUserData>(round)?
                .take::<ChatResult>()
                .map(Box::new)
        })?)),
        Phase::Batching => Ok(Input::Batched(answer(lua, ok, value, texts)?)),
        Phase::Calling => {
            let handler = next();
            if ok && !handler.is_nil() {
                return Ok(Input::Local {
                    handler,
                    args: next(),
                });
            }
            Ok(Input::Dispatched(answer(lua, ok, value, text)?))
        }
        Phase::Reporting(_) => Ok(Input::Reported(answer(lua, ok, value, text)?)),
        Phase::Done => Err(malformed("the models.loop step ran after its loop ended")),
    }
}

/// One `(ok, value)` envelope as the machine reads it: `value` read by
/// `read` when `ok`, else the value `fail` raises for it.
fn answer<T>(
    lua: &Lua,
    ok: bool,
    value: Value,
    read: impl FnOnce(Value) -> mlua::Result<T>,
) -> mlua::Result<std::result::Result<T, Value>> {
    if ok {
        read(value).map(Ok)
    } else {
        envelope_failure(lua, value).map(Err)
    }
}

/// Turns the machine's next step into the trampoline's tag and values:
/// each request table built as the shim builds it, a call's arguments
/// converted with `to_value`, the overflow reason as its `tag()`, a
/// normalized failure through `error_value::normalized`, and a new error
/// as `error_table` over `Error::Raised`.
fn act(lua: &Lua, then: Then<Value>) -> mlua::Result<MultiValue> {
    let (tag, values) = match then {
        Then::Chat { messages, handle } => {
            let handle = handle.unwrap_or(Value::Nil);
            let fields = [("messages", messages), ("handle", handle)];
            ("yield", vec![request(lua, "chat", fields)?])
        }
        Then::ToolCalls { calls, turn } => {
            let entries = lua.create_table_with_capacity(calls.len(), 0)?;
            for (position, call) in calls.iter().enumerate() {
                let entry = lua.create_table()?;
                for (name, value) in call_fields(lua, call)? {
                    entry.raw_set(name, value)?;
                }
                entries.raw_set(position + 1, entry)?;
            }
            let fields = [
                ("calls", Value::Table(entries)),
                ("turn", Value::Integer(i64::from(turn))),
            ];
            ("yield", vec![request(lua, "tool_calls", fields)?])
        }
        Then::ToolCall { call, turn } => {
            let [alias, args, call_id] = call_fields(lua, &call)?;
            let fields = [
                alias,
                args,
                call_id,
                ("turn", Value::Integer(i64::from(turn))),
            ];
            ("yield", vec![request(lua, "tool_call", fields)?])
        }
        Then::Compact { compactor, reason } => {
            let reason = Value::String(lua.create_string(reason.tag())?);
            ("compactor", vec![compactor, reason])
        }
        Then::Handle { handler, args } => ("handler", vec![handler, args]),
        Then::Report(returned) => {
            let ok = Value::Boolean(returned.is_some());
            let fields = [("ok", ok), ("value", returned.unwrap_or(Value::Nil))];
            ("yield", vec![request(lua, "local_tool_done", fields)?])
        }
        Then::Return => ("return", Vec::new()),
        Then::Raise(value) => ("raise", vec![value]),
        Then::RaiseNormalized(failure) => ("raise", vec![normalized(lua, failure)?]),
        Then::RaiseNew(raised) => {
            let table = error_table(lua, &Error::Raised(raised))?;
            ("raise", vec![Value::Table(table)])
        }
    };
    let mut action = MultiValue::from_vec(values);
    action.push_front(Value::String(lua.create_string(tag)?));
    Ok(action)
}

/// A requested call's `alias`, `args`, and `call_id` fields, as both the
/// `tool_calls` entries and the `tool_call` request carry them.
fn call_fields(lua: &Lua, call: &ToolCallEvent) -> mlua::Result<[(&'static str, Value); 3]> {
    Ok([
        ("alias", Value::String(lua.create_string(&call.name)?)),
        ("args", lua.to_value(&call.arguments)?),
        ("call_id", Value::String(lua.create_string(&call.id)?)),
    ])
}

/// A request table as the shim built it: `op` and the given fields, each
/// nil field left unset.
fn request<const N: usize>(lua: &Lua, op: &str, fields: [(&str, Value); N]) -> mlua::Result<Value> {
    let table = lua.create_table()?;
    table.raw_set("op", op)?;
    for (name, value) in fields {
        table.raw_set(name, value)?;
    }
    Ok(Value::Table(table))
}

/// The value `fail` raises for an envelope's failure: an error table
/// unchanged; anything else a new `lua`-kind table whose message is what
/// the `tostring` global returns for it. The Engine renders every failure
/// as an error table, so only a hand-built envelope reaches the second
/// branch.
fn envelope_failure(lua: &Lua, value: Value) -> mlua::Result<Value> {
    if let Value::Table(_) = value {
        return Ok(value);
    }
    let tostring: Function = lua.globals().get("tostring")?;
    let message: String = tostring.call(value)?;
    let table = error_table(lua, &Error::Raised(raised(ErrorKind::Lua, &message)))?;
    Ok(Value::Table(table))
}

/// A Lua string's text; any other value is a malformed answer.
fn text(value: Value) -> mlua::Result<String> {
    match value {
        Value::String(text) => Ok(text.to_str()?.to_owned()),
        _ => Err(malformed("a loop answer's text is not a string")),
    }
}

/// A sequence of Lua strings' texts; any other value is a malformed
/// answer.
fn texts(value: Value) -> mlua::Result<Vec<String>> {
    let Value::Table(sequence) = value else {
        return Err(malformed("a batch answer is not a sequence"));
    };
    (1..=sequence.raw_len())
        .map(|index| text(sequence.raw_get(index)?))
        .collect()
}

/// Lua's truth test: everything but nil and false.
fn truthy(value: &Value) -> bool {
    !matches!(value, Value::Nil | Value::Boolean(false))
}

/// The adapter failure for a value the shim chunk or the renderer never
/// produces: a crate bug, raised as an mlua callback error.
fn malformed(what: &'static str) -> mlua::Error {
    mlua::Error::external(Error::Internal(what.into()))
}

/// Lua's `type()` name for `value`, with an integer named `integer`, as
/// the shim's `engine_type` names it. mlua's `type_name` differs for a
/// light userdata (`lightuserdata`) and an mlua error value (`error`),
/// both of which Lua names `userdata`.
fn lua_type_name(value: &Value) -> &'static str {
    match value {
        Value::LightUserData(_) | Value::Error(_) => "userdata",
        other => other.type_name(),
    }
}

#[cfg(test)]
mod tests;
