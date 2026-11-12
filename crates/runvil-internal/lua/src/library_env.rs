//! The environment a library's code runs in.
//!
//! `require` loads each block of a library with an environment table of
//! its own, so the block and every function it defines name their globals
//! through it. The table holds the three names that take a relative
//! section path, made for the library: `call`, `prompt`, and `tasks`, whose
//! `spawn` and `fanout` are the library's own. A relative path given to
//! them starts at the library's section, while the library loads and when
//! its functions run later, whichever section is using it.
//!
//! Every other global read and every global write goes on to the VM's
//! globals table, and so through the `_G` guard, as it does from a
//! section's own block. So `jump` and every other global a library names
//! belong to the section that is running, and so do `_G.prompt`,
//! `_G.call`, and `_G.tasks`.
//!
//! The parts come from two installs that run before `require` is
//! installed: the yield shims hand over what makes `call` and `tasks`
//! through [`register_shims`], and the control globals hand over what
//! makes `prompt` through [`register_prompt`]. [`build`] makes one
//! library's table from whatever was registered.

use mlua::{Function, Lua, Table, Value};

use crate::section::LibraryKey;

/// The registry key of what makes a library's `call` and `tasks`: a table
/// holding the `call_from`, `spawn_from`, and `fanout_from` functions and
/// the Engine's `tasks` table.
const SHIMS_REGISTRY: &str = "runvil.library_env.shims";

/// The registry key of the function that makes a library's `prompt` table
/// from the library's identity.
const PROMPT_REGISTRY: &str = "runvil.library_env.prompt";

/// Keeps what makes a library's `call` and `tasks`. Each of the three
/// functions takes a library's identity and returns the function for that
/// library, and `tasks` is the Engine's table, whose other fields a
/// library's `tasks` shares.
///
/// # Errors
/// Returns the Lua failure of building or storing the registry value.
pub(crate) fn register_shims(
    lua: &Lua,
    call_from: Function,
    spawn_from: Function,
    fanout_from: Function,
    tasks: &Table,
) -> mlua::Result<()> {
    let shims = lua.create_table()?;
    shims.raw_set("call_from", call_from)?;
    shims.raw_set("spawn_from", spawn_from)?;
    shims.raw_set("fanout_from", fanout_from)?;
    shims.raw_set("tasks", tasks)?;
    lua.set_named_registry_value(SHIMS_REGISTRY, shims)
}

/// Keeps `prompt_from`, which takes a library's identity and returns the
/// `prompt` table whose reads start at that library.
///
/// # Errors
/// Returns the Lua failure of storing the registry value.
pub(crate) fn register_prompt(lua: &Lua, prompt_from: Function) -> mlua::Result<()> {
    lua.set_named_registry_value(PROMPT_REGISTRY, prompt_from)
}

/// Builds the environment the blocks of the library under `key` load
/// with.
///
/// The table holds `call`, `tasks`, and `prompt` made for the library. A
/// VM that never installed the yield shims has no `call` or `tasks` to
/// make, and one that never installed the control globals has no
/// `prompt`; the library then reads the VM's globals for those names, as
/// it does for every other. The metatable is protected, so a library's
/// code cannot read or replace it.
///
/// # Errors
/// Returns the Lua failure of building the table or of a registered
/// function.
pub(crate) fn build(lua: &Lua, key: &LibraryKey) -> mlua::Result<Table> {
    let identity = key.identity();
    let env = lua.create_table()?;
    if let Value::Table(shims) = lua.named_registry_value::<Value>(SHIMS_REGISTRY)? {
        let make = |name: &str| -> mlua::Result<Function> {
            shims.raw_get::<Function>(name)?.call(identity)
        };
        env.raw_set("call", make("call_from")?)?;
        // The Engine's `tasks` copied field by field, so setting the two
        // fields below changes the library's table alone.
        let tasks = lua.create_table()?;
        for pair in shims.raw_get::<Table>("tasks")?.pairs::<Value, Value>() {
            let (name, value) = pair?;
            tasks.raw_set(name, value)?;
        }
        tasks.raw_set("spawn", make("spawn_from")?)?;
        tasks.raw_set("fanout", make("fanout_from")?)?;
        env.raw_set("tasks", tasks)?;
    }
    if let Value::Function(prompt_from) = lua.named_registry_value::<Value>(PROMPT_REGISTRY)? {
        env.raw_set("prompt", prompt_from.call::<Table>(identity)?)?;
    }
    let globals = lua.globals();
    let meta = lua.create_table()?;
    meta.raw_set("__index", &globals)?;
    meta.raw_set("__newindex", &globals)?;
    meta.raw_set("__metatable", "a library's environment is guarded")?;
    env.set_metatable(Some(meta))?;
    Ok(env)
}
