//! Walk-target resolution: the visible-set machinery every control transfer
//! resolves against, and the wider reach `require` resolves libraries in.
//!
//! A control transfer or a prompt read addresses only the running section's
//! visible set: its home slice minus itself, plus its direct children. The
//! parent, aunts/uncles, nieces/nephews, and grandchildren are never in the
//! set, so a resolution error that lists the set cannot leak the rest of
//! the document's structure. The scheduler's chains resolve jumps and
//! `call` and `tasks.spawn` targets, fanout workers included, through these
//! helpers, and the
//! `prompt.section` and `prompt.list` reads resolve through them too, so
//! every such surface agrees on what a heading may name. `require` alone
//! also reaches the siblings of each ancestor (see [`Reachable`]), and
//! [`resolve_library`] turns the section it names into a [`Library`] keyed
//! by the section's position.

use crate::heading_address::{self, Reachable, SectionIndex, SectionPos, SlicePath, Visible};
use crate::lua::{Library, LibraryBlock, LibraryKey, SectionText};
use crate::parser::{Block, Prompt, Section};
use crate::{Error, Result};

/// The position of `target` in `slice`, matched on the parser-unique
/// `(level, name)` pair; `None` when the slice does not contain it.
pub(super) fn section_position(slice: &[Section], target: &Section) -> Option<SectionIndex> {
    slice
        .iter()
        .position(|s| s.level() == target.level() && s.name() == target.name())
        .map(SectionIndex::new)
}

/// The H1 pass's visible set: the whole top-level slice. H1 is
/// section 0, so it excludes nothing and has no children.
pub(super) fn h1_visible_sections(prompt: &Prompt) -> &[Section] {
    runvil_parser::detail::sections(prompt)
}

/// What a prompt read sees of the section `heading` names in a caller's
/// visible set, read in place through `visible`: its own raw Markdown and
/// its pre-parsed list items, or `None` when no visible section has that
/// heading.
///
/// # Errors
/// Returns [`Error::Lua`] when the heading is malformed or matches more
/// than one visible section (see [`heading_address::find_visible`]).
pub(super) fn read_visible(heading: &str, visible: Visible<'_>) -> Result<Option<SectionText>> {
    Ok(
        heading_address::find_visible(heading, visible)?.map(|section| SectionText {
            markdown: section.markdown().to_owned(),
            items: section
                .items()
                .iter()
                .map(|item| item.as_str().to_owned())
                .collect(),
        }),
    )
}

/// Where a jump, a `call`, or a spawn lands, resolved against the caller's
/// visible set: the target's position, and whether reaching it descends
/// into the caller's child slice.
#[derive(Debug)]
pub(super) struct JumpTarget {
    /// The target's position.
    pub(super) pos: SectionPos,
    /// Whether the target is a direct child of the caller, so a jump to it
    /// starts a child-level walk.
    pub(super) descends: bool,
}

/// Resolves `heading` against the visible set of the section at `jumper`
/// (its sibling slice minus itself, plus its direct children), read in
/// place through [`Visible::of`], and classifies the target: a direct
/// child of the jumper starts a child-level walk; anything else is a
/// sibling within the jumper's own slice.
///
/// Resolution is an exact `(level, name)` match (see
/// [`heading_address::resolve_visible`]): two visible sections sharing an
/// address error loudly as ambiguous instead of silently resolving to the
/// first.
///
/// # Errors
/// Returns [`Error::Lua`] when the heading is malformed, matches no visible
/// section, or matches more than one.
pub(super) fn resolve_jump_target(
    heading: &str,
    prompt: &Prompt,
    jumper: &SectionPos,
) -> Result<JumpTarget> {
    let siblings = jumper.siblings(prompt);
    let jumper_section = jumper.section(prompt).ok_or(Error::internal(
        "a jumping section is absent from its sibling slice",
    ))?;
    let target = heading_address::resolve_visible(heading, Visible::of(siblings, jumper.index))?;
    if let Some(index) = section_position(jumper_section.children(), target) {
        return Ok(JumpTarget {
            pos: jumper.child(index),
            descends: true,
        });
    }
    // `target` was resolved out of the visible set read from exactly these
    // two slices, so a miss here is an internal invariant violation, not a
    // user-facing Lua error.
    let index = section_position(siblings, target).ok_or(Error::internal(
        "resolved jump target is absent from the jumper's sibling slice",
    ))?;
    Ok(JumpTarget {
        pos: SectionPos {
            slice: jumper.slice.clone(),
            index,
        },
        descends: false,
    })
}

/// Where a `require` resolves from: the H1 pass, or a walked section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum RequireOrigin {
    /// The H1 pass, which reaches every top-level section.
    H1,
    /// The walked section at this position.
    Section(SectionPos),
}

impl RequireOrigin {
    /// The position `key` names: its library section's own.
    ///
    /// # Errors
    /// Returns [`Error::Internal`] when `key` is not one [`library_key`]
    /// built.
    fn from_key(key: &LibraryKey) -> Result<Self> {
        let mut indices = key
            .identity()
            .split('.')
            .map(str::parse)
            .collect::<std::result::Result<Vec<usize>, _>>()
            .map_err(|cause| {
                Error::internal_with_cause("a library key names no section position", cause)
            })?;
        let index = indices
            .pop()
            .ok_or(Error::internal("a library key names no section position"))?;
        let slice = indices
            .into_iter()
            .fold(SlicePath::root(), |path, ancestor| {
                path.child(SectionIndex::new(ancestor))
            });
        Ok(Self::Section(SectionPos {
            slice,
            index: SectionIndex::new(index),
        }))
    }

    /// What a `require` from here can name in `prompt`.
    fn reach<'p>(&'p self, prompt: &'p Prompt) -> Reachable<'p> {
        let root = runvil_parser::detail::sections(prompt);
        match self {
            Self::H1 => Reachable::top(root),
            Self::Section(pos) => Reachable::of(root, pos),
        }
    }
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

/// Resolves `heading` for a `require` and builds the library it names:
/// from the position `key` names when a library is loading, otherwise from
/// `origin`.
///
/// # Errors
/// Returns [`Error::Lua`] when the heading is malformed or names no
/// section in the reach, the miss listing the reach and nothing else, and
/// [`Error::Internal`] for a key [`library_key`] did not build or a block
/// kind the Engine cannot run.
pub(super) fn resolve_library(
    prompt: &Prompt,
    origin: &RequireOrigin,
    heading: &str,
    key: Option<&LibraryKey>,
) -> Result<Library> {
    let keyed = key.map(RequireOrigin::from_key).transpose()?;
    let from = keyed.as_ref().unwrap_or(origin);
    let reached = heading_address::resolve_reachable(heading, from.reach(prompt))?;
    build_library(reached.section, library_key(&reached.pos))
}

/// The library `section` makes under `key`: each Lua block in order,
/// paired with the prose block right before it, or with an empty template
/// when no prose block comes right before it. Prose after the last Lua
/// block pairs with nothing.
///
/// # Errors
/// Returns [`Error::Internal`] for a block kind the Engine cannot run.
fn build_library(section: &Section, key: LibraryKey) -> Result<Library> {
    let mut blocks = Vec::new();
    let mut pending = "";
    for block in section.blocks() {
        match block {
            Block::Prose { text, .. } => pending = text.as_str(),
            Block::Lua(program) => blocks.push(LibraryBlock {
                program: program.clone(),
                prose: std::mem::take(&mut pending).to_owned(),
            }),
            // `Block` is `#[non_exhaustive]` across the crate seam; an
            // unlisted variant has no library form.
            _ => return Err(Error::internal("an unrecognized block kind cannot load")),
        }
    }
    Ok(Library {
        key,
        heading: heading_address::section_heading(section),
        blocks,
    })
}

#[cfg(test)]
#[path = "walk_target-tests.rs"]
mod tests;
