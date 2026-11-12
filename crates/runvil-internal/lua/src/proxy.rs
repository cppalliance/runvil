//! The read-only proxy every seal builds on - `sys`, a frozen `argv` table,
//! a sealed prelude global, and a prelude's `var` view - the key
//! formatter their refusals and field paths share, and [`unseal`], which
//! reads a table through such views without calling into Lua.
//! `tools.stats` and `tools.packs` build on the walkable [`read_only_walk`].
//! Tool and Pack objects share [`registry_object`] and [`add_frozen_guard`].

use std::collections::HashSet;

use mlua::{
    AnyUserData, Function, Lua, MaybeSend, MetaMethod, MultiValue, Table, UserDataMethods, Value,
};

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

/// What a table stands for once sealed views are read through.
pub(crate) enum Unsealed {
    /// The table that holds the data.
    Data(Table),
    /// A view whose data only a function can read, with the view's label
    /// when it has one.
    Unreadable(Option<String>),
    /// A chain of views that leads back to one of its own views.
    Loop,
}

/// Reads `table` through sealed views, never calling into Lua.
///
/// A sealed view is a table with no entries of its own whose metatable has
/// both `__index` and `__metatable`: a table `__index` is its data, read
/// by these same rules, and a function `__index` cannot be read, labelled
/// by a string `__metatable`. Any other table is its own data. `hop` runs
/// once for each view passed, so the caller can bound the work, and the
/// views already passed sit in a set, so a long chain costs time in
/// proportion to its length.
///
/// # Errors
/// Returns the failure of `hop`, or of reading a metatable.
pub(crate) fn unseal(
    table: &Table,
    mut hop: impl FnMut() -> mlua::Result<()>,
) -> mlua::Result<Unsealed> {
    let mut current = table.clone();
    let mut views = HashSet::new();
    loop {
        let Some(metatable) = current.metatable() else {
            return Ok(Unsealed::Data(current));
        };
        let label: Value = metatable.raw_get("__metatable")?;
        if label.is_nil()
            || current
                .pairs::<Value, Value>()
                .next()
                .transpose()?
                .is_some()
        {
            return Ok(Unsealed::Data(current));
        }
        match metatable.raw_get::<Value>("__index")? {
            Value::Table(data) => {
                hop()?;
                views.insert(current.to_pointer());
                if views.contains(&data.to_pointer()) {
                    return Ok(Unsealed::Loop);
                }
                current = data;
            }
            Value::Function(_) => {
                let label = match label {
                    Value::String(label) => Some(label.to_string_lossy()),
                    _ => None,
                };
                return Ok(Unsealed::Unreadable(label));
            }
            _ => return Ok(Unsealed::Data(current)),
        }
    }
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

/// The userdata under `key` in the registry table `registry`, or `None`
/// when the table is absent or the entry is no userdata.
///
/// # Errors
/// Returns an `mlua` error when the registry cannot be read.
pub(crate) fn registry_object(
    lua: &Lua,
    registry: &str,
    key: &str,
) -> mlua::Result<Option<AnyUserData>> {
    let objects: Option<Table> = lua.named_registry_value(registry)?;
    match objects
        .map(|objects| objects.raw_get::<Value>(key))
        .transpose()?
    {
        Some(Value::UserData(object)) => Ok(Some(object)),
        _ => Ok(None),
    }
}

/// Installs the `__newindex` refusal of a frozen userdata: assigning any
/// field raises `{label} are frozen: cannot assign field {key:?}`.
pub(crate) fn add_frozen_guard<T, M: UserDataMethods<T>>(methods: &mut M, label: &'static str) {
    methods.add_meta_method(
        MetaMethod::NewIndex,
        move |_, _, (key, _): (String, Value)| -> mlua::Result<()> {
            Err(mlua::Error::external(format!(
                "{label} are frozen: cannot assign field {key:?}"
            )))
        },
    );
}
