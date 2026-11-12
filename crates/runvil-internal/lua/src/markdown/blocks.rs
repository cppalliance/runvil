//! Blocks as Lua tables, in both directions.
//!
//! [`list_table`] and [`block_table`] build the plain tables a parse hands
//! to Lua: a block is a record with its `kind`, its `line`, and the fields
//! of its kind, and every list is marked, so an empty one encodes as `[]`.
//! [`read`] goes the other way for `markdown.render`, and it is where a
//! block that cannot be written is refused, by the name of its field.

use mlua::{Lua, LuaSerdeExt, Table, Value};
use runvil_markdown::{Block, Kind};

use super::walk::{Entry, Walk, whole};
use crate::json::to_json;

/// Builds the marked list of tables for `blocks`.
///
/// # Errors
/// Returns the Lua failure of building a table, the memory limit's among
/// them.
pub(super) fn list_table(lua: &Lua, blocks: &[Block]) -> mlua::Result<Table> {
    let list = lua.create_table_with_capacity(blocks.len(), 0)?;
    list.set_metatable(Some(lua.array_metatable()))?;
    for (index, block) in blocks.iter().enumerate() {
        list.raw_set(index + 1, block_table(lua, block)?)?;
    }
    Ok(list)
}

/// Builds the table for one block.
///
/// # Errors
/// Returns the Lua failure of building a table, the memory limit's among
/// them.
pub(crate) fn block_table(lua: &Lua, block: &Block) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    table.raw_set("kind", block.kind().name())?;
    table.raw_set("line", block.line())?;
    match block {
        Block::Frontmatter { data, .. } => table.raw_set("data", lua.to_value(data)?)?,
        Block::Section {
            level,
            title,
            blocks,
            ..
        } => {
            table.raw_set("level", *level)?;
            table.raw_set("title", title.as_str())?;
            table.raw_set("blocks", list_table(lua, blocks)?)?;
        }
        Block::Prose { text, .. } => table.raw_set("text", text.as_str())?,
        Block::Fence {
            lang, info, text, ..
        } => {
            table.raw_set("lang", lang.as_str())?;
            table.raw_set("info", info.as_str())?;
            table.raw_set("text", text.as_str())?;
        }
        Block::List { ordered, items, .. } => {
            let list = lua.create_table_with_capacity(items.len(), 0)?;
            list.set_metatable(Some(lua.array_metatable()))?;
            for (index, item) in items.iter().enumerate() {
                list.raw_set(index + 1, item.as_str())?;
            }
            table.raw_set("ordered", *ordered)?;
            table.raw_set("items", list)?;
        }
        Block::Rule { .. } => {}
    }
    Ok(table)
}

/// Reads the list `value` as the blocks `markdown.render` writes, checking
/// every field of every block. `opens_text` is whether the list is the
/// one the function was given, whose first block may be frontmatter. The
/// blocks' lines are not read, since a writer has no use for them.
///
/// # Errors
/// Returns the refusal of a list or a block that cannot be read, and of a
/// block that cannot be written: an unknown `kind`, a field that is
/// missing or of the wrong type, a `level` outside 1 to 6, a line break in
/// a `title`, `info`, or `lang`, a list with no items, and frontmatter
/// that is not the first block or whose `data` is not a record.
pub(super) fn read(
    lua: &Lua,
    walk: &mut Walk,
    value: &Value,
    opens_text: bool,
) -> mlua::Result<Vec<Block>> {
    let list = walk.open(value)?;
    let mut blocks = Vec::new();
    walk.each(&list, &mut |walk, entry| {
        let first = opens_text && blocks.is_empty();
        blocks.push(read_block(lua, walk, entry, first)?);
        Ok(())
    })?;
    walk.close();
    Ok(blocks)
}

/// Reads one block. `first` is whether it is the first block of the text.
fn read_block(lua: &Lua, walk: &mut Walk, entry: &Entry, first: bool) -> mlua::Result<Block> {
    let name = walk.text(&entry.field("kind")?, "kind")?;
    let Some(kind) = Kind::from_name(&name) else {
        return Err(walk.refuse(format!(
            "{}.kind must be one of {}, got \"{name}\"",
            walk.block_name(),
            Kind::ALL.map(Kind::name).join(", ")
        )));
    };
    Ok(match kind {
        Kind::Frontmatter => {
            if !first {
                return Err(walk.refuse(format!(
                    "{} is frontmatter, which must be the first block",
                    walk.block_name()
                )));
            }
            let data = entry.field("data")?;
            let json = match &data {
                Value::Table(_) => to_json(lua, walk.function(), false, &data)?,
                other => return Err(not_a_record(walk, other.type_name())),
            };
            if !json.is_object() {
                return Err(not_a_record(walk, "a list"));
            }
            Block::Frontmatter {
                line: 0,
                data: json,
            }
        }
        Kind::Section => Block::Section {
            line: 0,
            level: level(walk, &entry.field("level")?)?,
            title: one_line(walk, entry, "title")?,
            blocks: read(lua, walk, &entry.field("blocks")?, false)?,
        },
        Kind::Prose => Block::Prose {
            line: 0,
            text: walk.text(&entry.field("text")?, "text")?,
        },
        Kind::Fence => Block::Fence {
            line: 0,
            lang: one_line(walk, entry, "lang")?,
            info: one_line(walk, entry, "info")?,
            text: walk.text(&entry.field("text")?, "text")?,
        },
        Kind::List => {
            let ordered = match entry.field("ordered")? {
                Value::Boolean(ordered) => ordered,
                other => {
                    return Err(walk.refuse(format!(
                        "{}.ordered must be a boolean, got {}",
                        walk.block_name(),
                        other.type_name()
                    )));
                }
            };
            let items = walk.strings(&entry.field("items")?, "items")?;
            if items.is_empty() {
                return Err(walk.refuse(format!(
                    "{}.items must hold at least one item",
                    walk.block_name()
                )));
            }
            Block::List {
                line: 0,
                ordered,
                items,
            }
        }
        Kind::Rule => Block::Rule { line: 0 },
    })
}

/// The refusal of frontmatter `data` that is `got` and not a record.
fn not_a_record(walk: &Walk, got: &str) -> mlua::Error {
    walk.refuse(format!(
        "{}.data must be a record, got {got}",
        walk.block_name()
    ))
}

/// Reads a section's `level`: a whole number from 1 to 6.
fn level(walk: &Walk, value: &Value) -> mlua::Result<u8> {
    if let Some(level @ 1..=6) = whole(value).and_then(|level| u8::try_from(level).ok()) {
        return Ok(level);
    }
    let got = match value {
        Value::Integer(integer) => integer.to_string(),
        Value::Number(number) => number.to_string(),
        other => other.type_name().to_owned(),
    };
    Err(walk.refuse(format!(
        "{}.level must be an integer from 1 to 6, got {got}",
        walk.block_name()
    )))
}

/// Reads the field `field` as text that sits on one line of the output.
fn one_line(walk: &mut Walk, entry: &Entry, field: &str) -> mlua::Result<String> {
    let text = walk.text(&entry.field(field)?, field)?;
    if text.contains(['\n', '\r']) {
        return Err(walk.refuse(format!(
            "{}.{field} must not hold a line break",
            walk.block_name()
        )));
    }
    Ok(text)
}
