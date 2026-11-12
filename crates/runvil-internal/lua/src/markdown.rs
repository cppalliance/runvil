//! The `markdown` Engine global: Markdown text as plain Lua tables.
//!
//! `markdown.parse(text)` reads text into a list of blocks. A block is a
//! plain record and a list is a marked list, so a parse result passes
//! `json.encode`, can be stored in `var`, and can be a `tasks.fanout`
//! collection. Each block holds its `source`, the lines it was read
//! from, and the sources of a parse are the text. `markdown.section`
//! finds the one section a path names, or one block of it that a selector
//! picks, and raises when it is not there. `markdown.section_or` is the
//! same lookup and returns nil for that miss. `markdown.select` collects
//! every block a selector matches, all of them for `*`, and
//! `markdown.render` writes the sources of blocks back as one text.
//!
//! `runvil-markdown` parses, writes, and holds the path and selector
//! grammars. This module turns its blocks into tables and writes tables
//! back (`blocks`), reads a list of blocks the author passes (`walk`),
//! and runs the lookups over those tables (`lookup`). Every function is
//! pure, and a lookup returns the tables that are already inside the list
//! it was given. The `prompt` reads take the same selector and index,
//! read by [`Selection`], over the blocks of one section of the running
//! prompt.

mod blocks;
mod lookup;
mod walk;

pub(crate) use blocks::block_table;

use mlua::{Lua, LuaSerdeExt, LuaString, MultiValue, Table, Value};
use runvil_markdown::{Path, Pick, Selector, Writer, pick};

use self::lookup::Match;
use self::walk::{Walk, whole};
use crate::error::{self, Result, author_refusal};
use crate::json::text_input;

/// Installs the `markdown` table as a global.
///
/// # Errors
/// Returns a `LuaRuntime` failure if the table, its functions, or the
/// global cannot be created.
pub(crate) fn install(lua: &Lua) -> Result<()> {
    let build = || -> mlua::Result<Table> {
        let markdown = lua.create_table()?;
        markdown.raw_set("parse", lua.create_function(parse)?)?;
        markdown.raw_set(
            "section",
            lua.create_function(|lua, arguments| {
                section(lua, "markdown.section", true, arguments)
            })?,
        )?;
        markdown.raw_set(
            "section_or",
            lua.create_function(|lua, arguments| {
                section(lua, "markdown.section_or", false, arguments)
            })?,
        )?;
        markdown.raw_set("select", lua.create_function(select)?)?;
        markdown.raw_set(
            "render",
            lua.create_function(|lua, blocks: Value| render(lua, &blocks))?,
        )?;
        Ok(markdown)
    };
    let markdown = build().map_err(error::make_lua_error)?;
    lua.globals()
        .raw_set("markdown", markdown)
        .map_err(error::make_lua_error)
}

/// `markdown.parse(text)`: the blocks of `text`, as a marked list.
fn parse(lua: &Lua, text: Value) -> mlua::Result<Table> {
    const FUNCTION: &str = "markdown.parse";
    let bytes = text_input(text, FUNCTION)?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| author_refusal(format!("{FUNCTION}: text is not valid UTF-8")))?;
    blocks::list_table(lua, &runvil_markdown::parse(text))
}

/// The selector of a lookup and the index after it.
pub(crate) struct Selection {
    /// The selector as the author wrote it.
    pub(crate) given: String,
    /// The selector.
    pub(crate) selector: Selector,
    /// The index, when there is one. It is never zero.
    pub(crate) n: Option<i64>,
}

impl Selection {
    /// Reads the `selector` and `n` arguments of `function`: none, a
    /// selector, or a selector and an index.
    ///
    /// # Errors
    /// Returns the refusal of a selector that is not a string or is
    /// malformed, of an index that is not a nonzero whole number, and of
    /// an index with no selector before it.
    pub(crate) fn read(function: &str, selector: &Value, n: &Value) -> mlua::Result<Option<Self>> {
        let refuse = |message: String| author_refusal(format!("{function}: {message}"));
        let given = match selector {
            Value::Nil if n.is_nil() => return Ok(None),
            Value::Nil => return Err(refuse("an index needs a selector before it".to_owned())),
            Value::String(given) => given.to_string_lossy(),
            other => {
                return Err(refuse(format!(
                    "selector must be a string, got {}",
                    other.type_name()
                )));
            }
        };
        let selector = Selector::parse(&given).map_err(refuse)?;
        let n = match (n, whole(n)) {
            (Value::Nil, _) => None,
            (_, Some(n)) if n != 0 => Some(n),
            (Value::Integer(_) | Value::Number(_), Some(_)) => {
                return Err(refuse("index must be a nonzero integer, got 0".to_owned()));
            }
            (Value::Number(number), None) => {
                return Err(refuse(format!(
                    "index must be a nonzero integer, got {number}"
                )));
            }
            (other, _) => {
                return Err(refuse(format!(
                    "index must be a nonzero integer, got {}",
                    other.type_name()
                )));
            }
        };
        Ok(Some(Self { given, selector, n }))
    }

    /// Picks one of `lines`, the lines of the blocks the selector matched
    /// in the section `place` names: the position of the pick, or the
    /// message of a miss.
    ///
    /// # Errors
    /// Returns the message that asks for an index when several matched
    /// and there is none.
    pub(crate) fn pick(
        &self,
        place: &str,
        lines: &[Option<i64>],
    ) -> std::result::Result<Pick, String> {
        pick(&self.given, place, lines, self.n)
    }
}

/// `markdown.section(blocks, path [, selector [, n]])` and its
/// `section_or` twin: the one section `path` names in `blocks`, or the one
/// block of it the selector picks. A section or a block that is not there
/// raises when `strict` and is nil otherwise; every other failure raises
/// from both.
fn section(
    lua: &Lua,
    function: &'static str,
    strict: bool,
    arguments: MultiValue,
) -> mlua::Result<Value> {
    let mut arguments = arguments.into_iter();
    let mut next = || arguments.next().unwrap_or(Value::Nil);
    let (blocks, path, selector, n) = (next(), next(), next(), next());
    let mut walk = Walk::new(lua, function);
    let given = match &path {
        Value::String(path) => path.to_string_lossy(),
        other => {
            return Err(walk.refuse(format!("path must be a string, got {}", other.type_name())));
        }
    };
    let path = Path::parse(&given).map_err(|message| walk.refuse(message))?;
    let selection = Selection::read(function, &selector, &n)?;
    let miss = |walk: &Walk, message: String| {
        if strict {
            Err(walk.refuse(message))
        } else {
            Ok(Value::Nil)
        }
    };
    let found = match lookup::sections(&mut walk, &blocks, &path)? {
        Err(number) => return miss(&walk, path.no_match(&given, number)),
        Ok(found) => found,
    };
    let [section] = found.as_slice() else {
        let lines: Vec<Option<i64>> = found.iter().map(|section| section.line).collect();
        return Err(walk.refuse(Path::ambiguous(&given, &lines)));
    };
    let Some(selection) = selection else {
        return Ok(Value::Table(section.found.clone()));
    };
    let members = lookup::members(&mut walk, &section.blocks, &selection.selector)?;
    let lines: Vec<Option<i64>> = members.iter().map(|block| block.line).collect();
    let place = format!("\"{}\"", given.trim());
    match selection
        .pick(&place, &lines)
        .map_err(|message| walk.refuse(message))?
    {
        Pick::Match(at) => Ok(members
            .into_iter()
            .nth(at)
            .map_or(Value::Nil, |Match { found, .. }| Value::Table(found))),
        Pick::Missing(message) => miss(&walk, message),
    }
}

/// `markdown.select(blocks, selector)`: every block `selector` matches at
/// any depth, in file order, as a new marked list of the tables
/// themselves.
fn select(lua: &Lua, (blocks, selector): (Value, Value)) -> mlua::Result<Table> {
    const FUNCTION: &str = "markdown.select";
    let mut walk = Walk::new(lua, FUNCTION);
    let Some(selection) = Selection::read(FUNCTION, &selector, &Value::Nil)? else {
        return Err(walk.refuse("selector must be a string, got nil"));
    };
    let mut kept = Vec::new();
    lookup::every(&mut walk, &blocks, &selection.selector, &mut kept)?;
    let list = lua.create_table_with_capacity(kept.len(), 0)?;
    list.set_metatable(Some(lua.array_metatable()))?;
    for (index, block) in kept.into_iter().enumerate() {
        list.raw_set(index + 1, block)?;
    }
    Ok(list)
}

/// `markdown.render(blocks)`: the `source` of every block of `blocks`, at
/// any depth and in file order, as one text. The writer parts two blocks
/// that would otherwise run together, and adds nothing between two blocks
/// of one parse.
fn render(lua: &Lua, blocks: &Value) -> mlua::Result<LuaString> {
    let mut walk = Walk::new(lua, "markdown.render");
    let mut writer = Writer::new();
    blocks::write(&mut walk, blocks, &mut writer)?;
    lua.create_string(writer.finish())
}
