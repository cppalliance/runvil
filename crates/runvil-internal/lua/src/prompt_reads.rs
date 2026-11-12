//! The `prompt` table: the reads of the running prompt.
//!
//! `prompt.section(path)` returns a section's own raw Markdown, and
//! `prompt.section(path, selector [, n])` returns one block of it as a
//! table built for the call: the block the selector picks once the
//! Markdown is parsed by the rules `markdown.parse` follows, with `line`
//! counting lines of the prompt file. Both forms raise when the section
//! or the block is not there. `prompt.section_or` is the same read, and
//! returns nil for that miss. Both raise for a path the caller's reader
//! refuses, for a target that is not a string, and for a malformed
//! selector or index.
//!
//! The reader looks a path up from a place: the library whose code made
//! the read, or the section that is running when a section's own code
//! did. A VM has one table for its sections' code, and
//! [`library_env`](crate::library_env) makes one more for each library it
//! loads.

use std::sync::Arc;

use mlua::{Function, Lua, Table, Value};
use runvil_markdown::{Block, Kind, Pick, Selector, parse_body};

use crate::error::{self, Result, author_refusal};
use crate::markdown::{Selection, block_table};
use crate::section::{LibraryKey, SectionRead, SectionText, resolve_section_target};

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
    let section = reader(lua, read, from.clone(), "prompt.section", true)?;
    let section_or = reader(lua, read, from, "prompt.section_or", false)?;
    let prompt = lua.create_table()?;
    prompt.raw_set("section", section)?;
    prompt.raw_set("section_or", section_or)?;
    Ok(prompt)
}

/// Builds one of the two reads, named `function`: `(path)` returns the
/// section's Markdown, and `(path, selector [, n])` one block of it. A
/// section or a block that is not there raises when `strict`, and is nil
/// otherwise. The selector and the index are checked before the path is
/// looked up, so a malformed one raises whether or not the section is
/// there.
fn reader<R, E>(
    lua: &Lua,
    read: &Arc<R>,
    from: Option<LibraryKey>,
    function: &'static str,
    strict: bool,
) -> mlua::Result<Function>
where
    R: Fn(String, Option<LibraryKey>) -> std::result::Result<SectionRead, E>
        + Send
        + Sync
        + 'static,
    E: std::error::Error + Send + Sync + 'static,
{
    let read = Arc::clone(read);
    lua.create_function(move |lua, (target, selector, n): (Value, Value, Value)| {
        let path = resolve_section_target(target)?;
        let selection = own_selection(function, &selector, &n)?;
        let text = match read(path.clone(), from.clone()).map_err(mlua::Error::external)? {
            SectionRead::Found(text) => text,
            SectionRead::Missing(error) if strict => return Err(mlua::Error::external(error)),
            SectionRead::Missing(_) => return Ok(Value::Nil),
        };
        let Some(selection) = selection else {
            return lua.create_string(text.markdown()).map(Value::String);
        };
        let refuse = |message: String| author_refusal(format!("{function}: {message}"));
        let blocks = own_blocks(&text);
        let matches: Vec<&Block> = blocks
            .iter()
            .filter(|block| selection.selector.selects(block))
            .collect();
        let lines: Vec<Option<i64>> = matches
            .iter()
            .map(|block| i64::try_from(block.line()).ok())
            .collect();
        let place = format!("section `{}`", path.trim());
        match selection.pick(&place, &lines).map_err(refuse)? {
            Pick::Match(at) => match matches.get(at) {
                Some(block) => block_table(lua, block).map(Value::Table),
                None => Ok(Value::Nil),
            },
            Pick::Missing(message) if strict => Err(refuse(message)),
            Pick::Missing(_) => Ok(Value::Nil),
        }
    })
}

/// Reads the selector and index of a read named `function`. A section's
/// own content holds no subsection and no frontmatter, so those two kinds
/// are refused.
fn own_selection(function: &str, selector: &Value, n: &Value) -> mlua::Result<Option<Selection>> {
    let selection = Selection::read(function, selector, n)?;
    let refusal = match selection.as_ref().map(|selection| &selection.selector) {
        Some(Selector::Kind(Kind::Section)) => {
            "a section's own content has no subsections; name a subsection by its own path"
        }
        Some(Selector::Kind(Kind::Frontmatter)) => "a section's own content has no frontmatter",
        _ => return Ok(selection),
    };
    Err(author_refusal(format!("{function}: {refusal}")))
}

/// Parses a section's own Markdown into blocks whose lines count lines of
/// the prompt file. No frontmatter is looked for, because a prompt's is at
/// the top of the file and never in a section.
fn own_blocks(text: &SectionText) -> Vec<Block> {
    let first_line = usize::try_from(text.line().get()).unwrap_or(usize::MAX);
    parse_body(text.markdown(), first_line)
}
