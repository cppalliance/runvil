//! Lookups over a list of blocks: the sections a path names, the blocks of
//! one section a selector matches, and every block a selector matches.
//!
//! A lookup returns the tables that are already inside the list it was
//! given, so it copies nothing. It reads only `kind`, `title`, `blocks`,
//! `lang`, and `line`, and a block whose `kind` it does not know matches
//! no selector but `*`, which matches every block.

use std::collections::HashSet;
use std::ffi::c_void;

use mlua::{Table, Value};
use runvil_markdown::{Path, Selector, Step, position};

use super::walk::{Entry, Walk};

/// One block a lookup kept.
pub(super) struct Match {
    /// The block's table, as it sits in the list.
    pub(super) found: Table,
    /// The block's `line`, when it holds one.
    pub(super) line: Option<i64>,
    /// A section's `blocks` field, or nil for any other block.
    pub(super) blocks: Value,
    /// The data table of a section's `blocks`, which the next step of a
    /// path starts from.
    list: Option<*const c_void>,
}

/// One step of a path applied to the whole tree.
struct Search<'a> {
    /// The step.
    step: &'a Step,
    /// The lists the step starts from, or `None` for the first step,
    /// which starts from the list the lookup was given.
    starts: Option<&'a HashSet<*const c_void>>,
    /// The sections kept so far, so one that two parents share counts
    /// once.
    seen: HashSet<*const c_void>,
    /// The sections the step kept, in file order.
    kept: Vec<Match>,
}

/// Finds the sections `path` names in the list `root`: every one its last
/// step kept, in file order, or the number of the first step, counted
/// from 1, that kept none.
///
/// Each step is one walk of the whole tree. `/title` keeps the sections
/// with that title that are direct members of a starting list, and
/// `//title` keeps those at any depth under one. `[n]` then keeps only
/// the `n`th of them.
///
/// # Errors
/// Returns the refusal of a list or a block that cannot be read.
pub(super) fn sections(
    walk: &mut Walk,
    root: &Value,
    path: &Path,
) -> mlua::Result<Result<Vec<Match>, usize>> {
    let mut starts: Option<HashSet<*const c_void>> = None;
    let mut kept = Vec::new();
    for (number, step) in path.steps().iter().enumerate() {
        let mut search = Search {
            step,
            starts: starts.as_ref(),
            seen: HashSet::new(),
            kept: Vec::new(),
        };
        search_list(walk, root, false, true, &mut search)?;
        kept = search.kept;
        if let Some(n) = step.index {
            kept = position(kept.len(), n)
                .map(|at| kept.swap_remove(at))
                .into_iter()
                .collect();
        }
        if kept.is_empty() {
            return Ok(Err(number + 1));
        }
        starts = Some(kept.iter().filter_map(|section| section.list).collect());
    }
    Ok(Ok(kept))
}

/// Applies one step to the list `list` and everything under it, and
/// returns the list's data table. `inside` is whether the list sits under
/// a starting list, and `root` whether it is the list the lookup was
/// given.
fn search_list(
    walk: &mut Walk,
    list: &Value,
    inside: bool,
    root: bool,
    search: &mut Search<'_>,
) -> mlua::Result<*const c_void> {
    let data = walk.open(list)?;
    let pointer = data.to_pointer();
    let here = search
        .starts
        .map_or(root, |starts| starts.contains(&pointer));
    walk.each(&data, &mut |walk, entry| {
        if !entry.is("kind", SECTION)? {
            return Ok(());
        }
        let reachable = here || (search.step.deep && inside);
        let kept = reachable
            && entry.is("title", &search.step.title)?
            && search.seen.insert(entry.found.to_pointer());
        let slot = if kept {
            search.kept.push(Match {
                found: entry.found.clone(),
                line: entry.line()?,
                blocks: Value::Nil,
                list: None,
            });
            Some(search.kept.len() - 1)
        } else {
            None
        };
        let blocks = entry.field("blocks")?;
        if !blocks.is_nil() {
            let list = search_list(walk, &blocks, inside || here, false, search)?;
            if let Some(section) = slot.and_then(|slot| search.kept.get_mut(slot)) {
                section.list = Some(list);
                section.blocks = blocks;
            }
        }
        Ok(())
    })?;
    walk.close();
    Ok(pointer)
}

/// The `kind` of a section.
const SECTION: &str = "section";

/// Whether `selector` matches the block `entry`, and whether the block
/// is a section. The block's `kind` is the empty text when it has none,
/// which only `*` matches, and its `lang` is read only for a selector
/// that names a language.
fn selects(selector: &Selector, entry: &Entry) -> mlua::Result<(bool, bool)> {
    let kind = entry.string("kind")?;
    let kind = kind.as_ref().and_then(|kind| kind.to_str().ok());
    let kind = kind.as_deref().unwrap_or_default();
    let lang = match selector {
        Selector::Any | Selector::Kind(_) => None,
        Selector::Fence(_) => entry.string("lang")?,
    };
    let lang = lang.as_ref().and_then(|lang| lang.to_str().ok());
    Ok((selector.matches(kind, lang.as_deref()), kind == SECTION))
}

/// Finds the blocks `selector` matches among the direct members of the
/// list `blocks`, in order. A nil `blocks` is a section that holds
/// nothing.
///
/// # Errors
/// Returns the refusal of a list or a block that cannot be read.
pub(super) fn members(
    walk: &mut Walk,
    blocks: &Value,
    selector: &Selector,
) -> mlua::Result<Vec<Match>> {
    let mut kept = Vec::new();
    if blocks.is_nil() {
        return Ok(kept);
    }
    let data = walk.open(blocks)?;
    walk.each(&data, &mut |_, entry| {
        let (selected, _) = selects(selector, entry)?;
        if selected {
            kept.push(Match {
                found: entry.found.clone(),
                line: entry.line()?,
                blocks: Value::Nil,
                list: None,
            });
        }
        Ok(())
    })?;
    walk.close();
    Ok(kept)
}

/// Adds to `kept` every block `selector` matches in the list `list` and
/// under it, in file order, a section before its members.
///
/// # Errors
/// Returns the refusal of a list or a block that cannot be read.
pub(super) fn every(
    walk: &mut Walk,
    list: &Value,
    selector: &Selector,
    kept: &mut Vec<Table>,
) -> mlua::Result<()> {
    let data = walk.open(list)?;
    walk.each(&data, &mut |walk, entry| {
        let (selected, section) = selects(selector, entry)?;
        if selected {
            kept.push(entry.found.clone());
        }
        if section {
            let blocks = entry.field("blocks")?;
            if !blocks.is_nil() {
                every(walk, &blocks, selector, kept)?;
            }
        }
        Ok(())
    })?;
    walk.close();
    Ok(())
}
