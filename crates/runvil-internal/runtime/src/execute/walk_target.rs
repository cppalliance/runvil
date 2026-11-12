//! Walk-target resolution: where a control transfer lands, what a prompt
//! read sees, and the library a `require` loads.
//!
//! Each of them names a section by a path and resolves it through
//! [`section_path`]. A control transfer takes a relative path, so it
//! reaches only a neighbor of the section it starts from: a sibling or a
//! direct child. The parent, the parent's siblings, a sibling's children,
//! and grandchildren have no relative form. The scheduler's chains resolve
//! jumps and `call` and `tasks.spawn` targets, fanout workers included,
//! through [`resolve_jump_target`]. The `prompt.section` and `prompt.list`
//! reads take a path from the top as well, which names any section, and
//! [`read_section`] resolves them. `require` takes only a path from the
//! top, and [`resolve_library`] turns the section it names into a
//! [`Library`] keyed by the section's position.

use crate::lua::{Library, LibraryBlock, LibraryKey, SectionText};
use crate::prompt::{Block, Prompt, Section};
use crate::section_path::{self, Origin};
use crate::section_pos::SectionPos;
use crate::{Result, error};

/// What a prompt read sees of the section `path` names from `origin`: its
/// own raw Markdown and its pre-parsed list items, or `None` when the path
/// names no section.
///
/// # Errors
/// Returns a `LuaRuntime` failure when the path is malformed (see
/// [`section_path::find`]).
pub(super) fn read_section(
    path: &str,
    prompt: &Prompt,
    origin: Origin<'_>,
) -> Result<Option<SectionText>> {
    let Some(pos) = section_path::find(path, prompt, origin)?.found() else {
        return Ok(None);
    };
    let section = found_section(prompt, &pos)?;
    Ok(Some(SectionText {
        markdown: section.markdown().to_owned(),
        items: section
            .items()
            .iter()
            .map(|item| item.as_str().to_owned())
            .collect(),
    }))
}

/// The section a lookup found at `pos`.
///
/// # Errors
/// Returns an `Internal` failure when `pos` names no section, which a
/// position a lookup returned never does.
fn found_section<'p>(prompt: &'p Prompt, pos: &SectionPos) -> Result<&'p Section> {
    pos.section(prompt).ok_or(error::make_internal_error(
        "a resolved section path names no section",
    ))
}

/// Where a jump, a `call`, or a spawn lands: the target's position, and
/// whether reaching it descends into the caller's child slice.
#[derive(Debug)]
pub(super) struct JumpTarget {
    /// The target's position.
    pub(super) pos: SectionPos,
    /// Whether the target is a direct child of a walked caller, so a jump
    /// to it starts a child-level walk.
    pub(super) descends: bool,
}

/// Resolves the relative `path` from `origin` and classifies the target:
/// a direct child of a walked section starts a child-level walk, and
/// anything else is a section of the caller's own slice. The H1's
/// children are the top-level sections, where the walk already runs, so
/// its targets never descend.
///
/// # Errors
/// Returns a `LuaRuntime` failure when the path is malformed, is not one
/// of the two relative forms, or names no section (see
/// [`section_path::resolve_neighbor`]).
pub(super) fn resolve_jump_target(
    path: &str,
    prompt: &Prompt,
    origin: Origin<'_>,
) -> Result<JumpTarget> {
    let pos = section_path::resolve_neighbor(path, prompt, origin)?;
    let descends = matches!(origin, Origin::Section(caller) if caller.slice != pos.slice);
    Ok(JumpTarget { pos, descends })
}

/// The key naming the section at `pos`: every index along its path, then
/// its own, dot-joined, so each position has its own.
fn library_key(pos: &SectionPos) -> LibraryKey {
    let indices: Vec<String> = pos
        .slice
        .indices()
        .iter()
        .copied()
        .chain([pos.index.get()])
        .map(|index| index.to_string())
        .collect();
    LibraryKey::new(indices.join("."))
}

/// Resolves `path` for a `require` and builds the library it names. The
/// path starts at the top of the prompt, so it names the same library
/// wherever the `require` is written.
///
/// # Errors
/// Returns a `LuaRuntime` failure when the path is malformed, does not
/// start with `/`, or names no section, and an `Internal` failure for a
/// block kind the Engine cannot run.
pub(super) fn resolve_library(prompt: &Prompt, path: &str) -> Result<Library> {
    let pos = section_path::resolve_from_top(path, prompt)?;
    build_library(
        found_section(prompt, &pos)?,
        library_key(&pos),
        section_path::path_from_top(prompt, &pos),
    )
}

/// The library `section` makes under `key`, reported by `path`: each Lua
/// block in order, paired with the prose block right before it and the
/// line that prose starts on, or with an empty template and no line when
/// no prose block comes right before it. Prose after the last Lua block
/// pairs with nothing.
///
/// # Errors
/// Returns an `Internal` failure for a block kind the Engine cannot run.
fn build_library(section: &Section, key: LibraryKey, path: String) -> Result<Library> {
    let mut blocks = Vec::new();
    let mut pending = None;
    for block in section.blocks() {
        match block {
            Block::Prose { text, line, .. } => pending = Some((text.as_str(), *line)),
            Block::Lua(program) => {
                let (prose, line) = pending.take().unzip();
                blocks.push(LibraryBlock {
                    program: program.clone(),
                    prose: prose.unwrap_or_default().to_owned(),
                    line,
                });
            }
            // `Block` is `#[non_exhaustive]` across the crate seam; an
            // unlisted variant has no library form.
            _ => {
                return Err(error::make_internal_error(
                    "an unrecognized block kind cannot load",
                ));
            }
        }
    }
    Ok(Library { key, path, blocks })
}

#[cfg(test)]
#[path = "walk_target-tests.rs"]
mod tests;
