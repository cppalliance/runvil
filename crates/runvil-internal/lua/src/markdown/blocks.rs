//! Blocks as Lua tables, and the tables written back as text.
//!
//! [`list_table`] and [`block_table`] build the plain tables a parse hands
//! to Lua: a block is a record with its `kind`, its `line`, its `source`,
//! and the fields of its kind, and every list is marked, so an empty one
//! encodes as `[]`. [`write`](fn@write) goes the other way for
//! `markdown.render`: it writes each block's `source`, and reads no other
//! field but the `kind` and `blocks` that say what comes next.

use mlua::{Lua, LuaSerdeExt, Table, Value};
use runvil_markdown::{Block, Kind, Writer};

use super::walk::Walk;

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
    table.raw_set("source", block.source())?;
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

/// Writes the `source` of every block of the list `value` to `writer`, in
/// order, a section's before those of the blocks under it.
///
/// A block's `kind` is read for two things: the writer parts two prose
/// blocks, and only a section's `blocks` are written. A section with no
/// `blocks` field holds nothing, as it does for a lookup.
///
/// # Errors
/// Returns the refusal of a list or a block that cannot be read, and of a
/// `source` that is not a string of valid UTF-8.
pub(super) fn write(walk: &mut Walk, value: &Value, writer: &mut Writer) -> mlua::Result<()> {
    let list = walk.open(value)?;
    walk.each(&list, &mut |walk, entry| {
        let source = walk.text(&entry.field("source")?, "source")?;
        let kind = entry.string("kind")?;
        let kind = kind.as_ref().and_then(|kind| kind.to_str().ok());
        let kind = kind.as_deref().unwrap_or_default();
        writer.write(kind, &source);
        if kind == Kind::Section.name() {
            let blocks = entry.field("blocks")?;
            if !blocks.is_nil() {
                write(walk, &blocks, writer)?;
            }
        }
        Ok(())
    })?;
    walk.close();
    Ok(())
}
