//! A section's place in the prompt tree.
//!
//! [`SectionPos`] names a section by its sibling slice, a [`SlicePath`],
//! and its [`SectionIndex`] in that slice. A chain and a section's lookups
//! find their slice by path and read it in place, so they borrow nothing
//! from the tree the run shares.

use crate::prompt::{Prompt, Section, detail};

/// Where a sibling slice sits in the prompt tree: the index of each
/// ancestor section from the top level down to the slice's parent. The
/// empty path is the top-level slice. A chain names its walk position by
/// path so it borrows nothing from the tree the run shares through its
/// `Arc<Prompt>`; the path resolves to the slice on demand.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SlicePath(Vec<usize>);

impl SlicePath {
    /// The top-level slice.
    pub(crate) fn root() -> Self {
        Self::default()
    }

    /// The slice of `parent`'s children, where `parent` is the section at
    /// `index` of this slice.
    pub(crate) fn child(&self, index: SectionIndex) -> Self {
        let mut path = self.0.clone();
        path.push(index.0);
        Self(path)
    }

    /// Each ancestor's index, from the top level down.
    pub(crate) fn indices(&self) -> &[usize] {
        &self.0
    }

    /// Resolves the path against `prompt`. A path the scheduler built is
    /// always in range; an out-of-range index resolves to the empty slice,
    /// which the walk treats as exhausted rather than panicking.
    pub(crate) fn resolve<'p>(&self, prompt: &'p Prompt) -> &'p [Section] {
        let mut slice = detail::sections(prompt);
        for &index in &self.0 {
            slice = slice.get(index).map_or(&[], Section::children);
        }
        slice
    }
}

/// A section's position in its sibling slice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SectionIndex(usize);

impl SectionIndex {
    /// The slice's first section.
    pub(crate) const FIRST: SectionIndex = SectionIndex(0);

    /// The section at `index` in its slice.
    pub(crate) fn new(index: usize) -> SectionIndex {
        SectionIndex(index)
    }

    /// The position as a slice index.
    pub(crate) fn get(self) -> usize {
        self.0
    }

    /// The section after this one in the same slice.
    pub(crate) fn next(self) -> SectionIndex {
        SectionIndex(self.0 + 1)
    }
}

/// A section's place in the prompt tree: the path of its sibling slice and
/// its index in that slice. A position the walk moves to may sit past its
/// slice's last section, where it names no section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SectionPos {
    /// The path of the section's sibling slice.
    pub(crate) slice: SlicePath,
    /// The section's index in that slice.
    pub(crate) index: SectionIndex,
}

impl SectionPos {
    /// The top-level section at `index`.
    pub(crate) fn top(index: SectionIndex) -> SectionPos {
        SectionPos {
            slice: SlicePath::root(),
            index,
        }
    }

    /// The next sibling.
    pub(crate) fn next(&self) -> SectionPos {
        SectionPos {
            slice: self.slice.clone(),
            index: self.index.next(),
        }
    }

    /// This section's child at `index`.
    pub(crate) fn child(&self, index: SectionIndex) -> SectionPos {
        SectionPos {
            slice: self.slice.child(self.index),
            index,
        }
    }

    /// The section's sibling slice, resolved against `prompt`.
    pub(crate) fn siblings<'p>(&self, prompt: &'p Prompt) -> &'p [Section] {
        self.slice.resolve(prompt)
    }

    /// The section, or `None` when the index is past its slice.
    pub(crate) fn section<'p>(&self, prompt: &'p Prompt) -> Option<&'p Section> {
        self.siblings(prompt).get(self.index.0)
    }

    /// The section's name, or the prompt's title when the index is past
    /// its slice: a chain's section name before its first entry.
    pub(crate) fn name<'p>(&self, prompt: &'p Prompt) -> &'p str {
        self.section(prompt).map_or(prompt.title(), Section::name)
    }
}
