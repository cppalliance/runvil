//! Walk-target resolution: where a control transfer lands, what a prompt
//! read sees, and the library a `require` loads.
//!
//! Each of them names a section by a path and resolves it through
//! [`section_path`]. A control transfer takes a relative path, so it
//! reaches only a neighbor of the section it starts from: a sibling or a
//! direct child. The parent, the parent's siblings, a sibling's children,
//! and grandchildren have no relative form. The scheduler's chains resolve
//! jumps and `call` and `tasks.spawn` targets, fanout workers included,
//! through [`resolve_jump_target`]. The `prompt` reads take a path from
//! the top as well, which names any section, and [`read_section`]
//! resolves them. A relative path in a library's code starts at the
//! library, whose position [`library_position`] reads back from its key.
//! `require` takes only a path from the
//! top, and [`resolve_library`] turns the section it names into a
//! [`Library`] keyed by the section's position.

use crate::lua::{Library, LibraryBlock, LibraryKey, SectionRead, SectionText};
use crate::prompt::{Block, Prompt, Section};
use crate::section_path::{self, Lookup, Origin};
use crate::section_pos::{SectionIndex, SectionPos, SlicePath};
use crate::{Result, error};

/// What a prompt read sees of the section `path` names: its own raw
/// Markdown, read in place from the shared prompt text, with the file line
/// it starts on, or the not-found error when
/// the path names no section. A relative path starts at the library `from`
/// names, when a library's code made the read, and otherwise at `origin`,
/// the section that is running.
///
/// # Errors
/// Returns a `LuaRuntime` failure when the path is malformed (see
/// [`section_path::find`]), and an `Internal` failure for a key that names
/// no section position.
pub(super) fn read_section(
    path: &str,
    prompt: &Prompt,
    origin: Origin<'_>,
    from: Option<&LibraryKey>,
) -> Result<SectionRead> {
    let library = from.map(library_position).transpose()?;
    let origin = library.as_ref().map_or(origin, Origin::Section);
    let pos = match section_path::find(path, prompt, origin)? {
        Lookup::Found(pos) => pos,
        Lookup::Missing(error) => return Ok(SectionRead::Missing(error)),
    };
    let section = found_section(prompt, &pos)?;
    let (text, range) = section.markdown_text().shared();
    Ok(SectionRead::Found(SectionText::new(
        text,
        range,
        section.markdown_line(),
    )))
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

/// Resolves the relative `path` and classifies the target: a direct child
/// of a walked section starts a child-level walk, and anything else is a
/// section of the caller's own slice. The H1's children are the top-level
/// sections, where the walk already runs, so its targets never descend.
///
/// The path starts at the library `from` names, when a library's code
/// made the request, and otherwise at `origin`, the section that is
/// running.
///
/// # Errors
/// Returns a `LuaRuntime` failure when the path is malformed, is not one
/// of the two relative forms, or names no section (see
/// [`section_path::resolve_neighbor`]), and an `Internal` failure for a
/// key that names no section position.
pub(super) fn resolve_jump_target(
    path: &str,
    prompt: &Prompt,
    origin: Origin<'_>,
    from: Option<&LibraryKey>,
) -> Result<JumpTarget> {
    let library = from.map(library_position).transpose()?;
    let origin = library.as_ref().map_or(origin, Origin::Section);
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

/// The position of the library section `key` names: where a relative path
/// in that library's code starts.
///
/// # Errors
/// Returns an `Internal` failure when `key` is not one [`library_key`]
/// built.
pub(super) fn library_position(key: &LibraryKey) -> Result<SectionPos> {
    let mut indices = key
        .identity()
        .split('.')
        .map(str::parse)
        .collect::<std::result::Result<Vec<usize>, _>>()
        .map_err(|cause| {
            error::make_internal_with_cause_error("a library key names no section position", &cause)
        })?;
    let index = indices.pop().ok_or(error::make_internal_error(
        "a library key names no section position",
    ))?;
    let slice = indices
        .into_iter()
        .fold(SlicePath::root(), |path, ancestor| {
            path.child(SectionIndex::new(ancestor))
        });
    Ok(SectionPos {
        slice,
        index: SectionIndex::new(index),
    })
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
