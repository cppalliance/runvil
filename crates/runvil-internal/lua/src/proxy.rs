//! The read-only proxy every seal builds on - `sys`, a frozen `argv` table,
//! a sealed prelude global, and a prelude's `var` view - and the key
//! formatter their refusals and field paths share.
//! `tools.stats` and `tools.packs` build on the walkable [`read_only_walk`].

use mlua::{Function, Lua, MaybeSend, MultiValue, Table, Value};

/// Renders a table key for a refusal: a string as its text, an integer as
/// its digits, and any other key by its type name.
pub(crate) fn key_text(key: &Value) -> String {
    match key {
        Value::String(name) => name.to_string_lossy(),
        Value::Integer(index) => index.to_string(),
        other => other.type_name().to_owned(),
    }
}

/// Renders the path of the field `key` under `path`: `path.name` for a
/// string key, `path[key]` for any other, its key rendered by [`key_text`].
pub(crate) fn key_path(path: &str, key: &Value) -> String {
    match key {
        Value::String(name) => format!("{path}.{}", name.to_string_lossy()),
        other => format!("{path}[{}]", key_text(other)),
    }
}

/// Builds an empty proxy whose `__index` is `index`, whose `__newindex`
/// raises the text `refusal` builds from the refused key, and whose
/// `__metatable` is `label`, so `pairs` over it sees nothing and
/// `getmetatable` returns only the label.
pub(crate) fn read_only_proxy(
    lua: &Lua,
    index: Value,
    refusal: impl Fn(&Value) -> String + MaybeSend + 'static,
    label: &str,
) -> mlua::Result<Table> {
    let newindex = lua.create_function(
        move |_lua, (_proxy, key, _value): (Value, Value, Value)| -> mlua::Result<()> {
            Err(mlua::Error::runtime(refusal(&key)))
        },
    )?;
    let metatable = lua.create_table()?;
    metatable.raw_set("__index", index)?;
    metatable.raw_set("__newindex", newindex)?;
    metatable.raw_set("__metatable", label)?;
    let proxy = lua.create_table()?;
    proxy.set_metatable(Some(metatable))?;
    Ok(proxy)
}

/// An empty proxy whose metatable holds `index`, a `__pairs` that walks
/// with `step`, a `__newindex` that raises `<path> is read-only`, and that
/// text as its `__metatable`, which protects it.
pub(crate) fn read_only_walk(
    lua: &Lua,
    index: Function,
    step: Function,
    path: &str,
) -> mlua::Result<Table> {
    let label = format!("{path} is read-only");
    let pairs =
        lua.create_function(move |_, proxy: Table| Ok((step.clone(), proxy, Value::Nil)))?;
    let refusal = label.clone();
    let newindex = lua.create_function(move |_, _: MultiValue| -> mlua::Result<()> {
        Err(mlua::Error::external(refusal.clone()))
    })?;
    let meta = lua.create_table()?;
    meta.raw_set("__index", index)?;
    meta.raw_set("__pairs", pairs)?;
    meta.raw_set("__newindex", newindex)?;
    meta.raw_set("__metatable", label)?;
    let proxy = lua.create_table()?;
    proxy.set_metatable(Some(meta))?;
    Ok(proxy)
}
