//! The `prompt` table: the reads of the running prompt.
//!
//! `prompt.section(path)` returns a section's own raw Markdown and raises
//! when the path names no section. `prompt.section_or(path)` is the same
//! read, and returns nil for that miss. `prompt.list(path)` returns a list
//! section's items. All three raise for a path the caller's reader
//! refuses and for a target that is not a string.
//!
//! The reader looks a path up from a place: the library whose code made
//! the read, or the section that is running when a section's own code
//! did. A VM has one table for its sections' code, and
//! [`library_env`](crate::library_env) makes one more for each library it
//! loads.

use std::sync::Arc;

use mlua::{Lua, Table, Value};

use crate::error::{self, Result};
use crate::section::{LibraryKey, SectionRead, resolve_section_target};

/// Installs the `prompt` global over `read`, and hands the library
/// environment what makes a library's own `prompt`.
///
/// # Errors
/// Returns a `LuaRuntime` failure if the table cannot be built or
/// installed.
pub(crate) fn install<R, E>(lua: &Lua, globals: &Table, read: R) -> Result<()>
where
    R: Fn(String, Option<LibraryKey>) -> std::result::Result<SectionRead, E>
        + Send
        + Sync
        + 'static,
    E: std::error::Error + Send + Sync + 'static,
{
    let read = Arc::new(read);
    let prompt = table(lua, &read, None).map_err(error::make_lua_error)?;
    globals
        .raw_set("prompt", prompt)
        .map_err(error::make_lua_error)?;
    let prompt_from = lua
        .create_function(move |lua, identity: String| {
            table(lua, &read, Some(LibraryKey::new(identity)))
        })
        .map_err(error::make_lua_error)?;
    crate::library_env::register_prompt(lua, prompt_from).map_err(error::make_lua_error)
}

/// Builds one `prompt` table over `read`. Its reads start at the library
/// `from` names, or at the running section when `from` is `None`.
fn table<R, E>(lua: &Lua, read: &Arc<R>, from: Option<LibraryKey>) -> mlua::Result<Table>
where
    R: Fn(String, Option<LibraryKey>) -> std::result::Result<SectionRead, E>
        + Send
        + Sync
        + 'static,
    E: std::error::Error + Send + Sync + 'static,
{
    let look_up = {
        let read = Arc::clone(read);
        move |target: Value| -> mlua::Result<(String, SectionRead)> {
            let path = resolve_section_target(target)?;
            let found = read(path.clone(), from.clone()).map_err(mlua::Error::external)?;
            Ok((path, found))
        }
    };
    let look_up = Arc::new(look_up);
    let strict = Arc::clone(&look_up);
    let section = lua.create_function(move |_, target: Value| match strict(target)?.1 {
        SectionRead::Found(text) => Ok(text.markdown),
        SectionRead::Missing(error) => Err(mlua::Error::external(error)),
    })?;
    let lenient = Arc::clone(&look_up);
    let section_or = lua.create_function(move |_, target: Value| match lenient(target)?.1 {
        SectionRead::Found(text) => Ok(Some(text.markdown)),
        SectionRead::Missing(_) => Ok(None),
    })?;
    let list = lua.create_function(move |lua, target: Value| match look_up(target)? {
        (_, SectionRead::Missing(_)) => Ok(Value::Nil),
        (path, SectionRead::Found(text)) if text.items.is_empty() => Err(mlua::Error::external(
            format!("section `{}` has no pre-parsed items", path.trim()),
        )),
        (_, SectionRead::Found(text)) => pack_sequence(lua, text.items).map(Value::Table),
    })?;
    let prompt = lua.create_table()?;
    prompt.raw_set("section", section)?;
    prompt.raw_set("section_or", section_or)?;
    prompt.raw_set("list", list)?;
    Ok(prompt)
}

/// Collects owned values into a 1-based Lua sequence table.
fn pack_sequence(lua: &Lua, values: Vec<String>) -> mlua::Result<Table> {
    let table = lua.create_table_with_capacity(values.len(), 0)?;
    for (index, value) in values.into_iter().enumerate() {
        table.raw_set(index + 1, value)?;
    }
    Ok(table)
}
