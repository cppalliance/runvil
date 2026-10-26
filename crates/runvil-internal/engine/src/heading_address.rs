//! Heading resolution: the exact `(level, name)` address every control
//! surface resolves through.
//!
//! A section's Lua names other sections by heading string - `call`, `jump`,
//! `prompt.section`, `prompt.list`, `tasks.spawn`, and `tasks.fanout`'s
//! worker template. This module defines [`Visible`], the view of a caller's
//! visible set read in place over the shared prompt, [`find_visible`], the
//! one heading lookup those surfaces share, and [`resolve_visible`], its
//! adapter for surfaces where a miss is an error.
//!
//! `require` names a library section the same way but reaches wider: a
//! walked section's direct children, its siblings, and the siblings of each
//! of its ancestors up to the top level, or every top-level section from
//! the H1. [`Reachable`] is that view, [`find_reachable`] its lookup, and
//! [`resolve_reachable`] its adapter where a miss is an error. Each scope
//! of a reach sits at its own heading level and sibling names are unique,
//! so an exact address names at most one reachable section.
//!
//! [`SlicePath`] and [`SectionIndex`] name a section's place in the shared
//! prompt tree: its sibling slice by path, and its position in that slice.
//! A chain and a section's `prompt` reads find their slice by path and
//! read it in place.

use crate::parser::{Prompt, Section};
use crate::{Error, Result};

/// Where a sibling slice sits in the prompt tree: the index of each
/// ancestor section from the top level down to the slice's parent. The
/// empty path is the top-level slice. A chain names its walk position by
/// path so it borrows nothing from the tree the run shares through its
/// `Arc<Prompt>`; the path resolves to the slice on demand.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SlicePath(Vec<usize>);

/// The top-level slice's path, which the H1's reach borrows.
static TOP: SlicePath = SlicePath(Vec::new());

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
        let mut slice = runvil_parser::detail::sections(prompt);
        for &index in &self.0 {
            slice = slice.get(index).map_or(&[], Section::children);
        }
        slice
    }

    /// The name of the slice's section at `index`, or the prompt's title
    /// when `index` is past the slice: a chain's section name before its
    /// first entry.
    pub(crate) fn name_at<'p>(&self, prompt: &'p Prompt, index: SectionIndex) -> &'p str {
        self.resolve(prompt)
            .get(index.0)
            .map_or(prompt.title(), Section::name)
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

/// A section's visible set, read in place: its home slice minus itself,
/// plus its direct children.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Visible<'a> {
    home: &'a [Section],
    caller: Option<SectionIndex>,
}

impl<'a> Visible<'a> {
    /// A walked section's set: `home` minus `home[caller]`, plus
    /// `home[caller]`'s children. `caller` must index `home`; an
    /// out-of-range one yields all of `home` and no children, the same set
    /// as [`Visible::all`].
    pub(crate) fn of(home: &'a [Section], caller: SectionIndex) -> Visible<'a> {
        debug_assert!(
            caller.0 < home.len(),
            "a walked section lies inside its home slice"
        );
        Visible {
            home,
            caller: Some(caller),
        }
    }

    /// A whole slice and nothing else: the H1 pass's top-level set, and
    /// synthetic sets in tests.
    pub(crate) fn all(slice: &'a [Section]) -> Visible<'a> {
        Visible {
            home: slice,
            caller: None,
        }
    }

    /// The sections in order: the home slice in order with the caller
    /// skipped, then the children.
    pub(crate) fn iter(self) -> impl Iterator<Item = &'a Section> {
        let caller = self.caller;
        let children = caller
            .and_then(|c| self.home.get(c.0))
            .map_or(&[][..], Section::children);
        self.home
            .iter()
            .enumerate()
            .filter(move |&(index, _)| Some(SectionIndex(index)) != caller)
            .map(|(_, section)| section)
            .chain(children)
    }
}

/// What `require` can name from one place, read in place: from a walked
/// section, its home slice minus itself, its direct children, and each
/// ancestor's slice minus that ancestor; from the H1, the top-level slice.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Reachable<'a> {
    root: &'a [Section],
    path: &'a SlicePath,
    caller: Option<SectionIndex>,
}

/// A reachable section and where it sits: the path of its slice and its
/// index in that slice.
#[derive(Debug)]
pub(crate) struct Reached<'a> {
    pub(crate) section: &'a Section,
    pub(crate) path: SlicePath,
    pub(crate) index: SectionIndex,
}

impl<'a> Reachable<'a> {
    /// A walked section's reach: the section at `caller` of the slice that
    /// `path` leads to from `root`. An out-of-range index along the way
    /// leads to an empty slice, as a walk path's does.
    pub(crate) fn of(
        root: &'a [Section],
        path: &'a SlicePath,
        caller: SectionIndex,
    ) -> Reachable<'a> {
        Reachable {
            root,
            path,
            caller: Some(caller),
        }
    }

    /// The H1's reach: every top-level section.
    pub(crate) fn top(root: &'a [Section]) -> Reachable<'a> {
        Reachable {
            root,
            path: &TOP,
            caller: None,
        }
    }

    /// The sections with their positions, nearest scope first: the home
    /// slice with the caller skipped, the caller's children, then each
    /// ancestor's slice with that ancestor skipped, from the parent's
    /// outward to the top level.
    pub(crate) fn iter(self) -> impl Iterator<Item = Reached<'a>> {
        let ancestors = &self.path.0;
        let mut slices = Vec::with_capacity(ancestors.len());
        let mut home = self.root;
        for &index in ancestors {
            slices.push(home);
            home = home.get(index).map_or(&[][..], Section::children);
        }
        let mut scopes = vec![(self.path.clone(), home, self.caller)];
        if let Some(caller) = self.caller {
            scopes.push((
                self.path.child(caller),
                home.get(caller.0).map_or(&[][..], Section::children),
                None,
            ));
        }
        for (depth, (&slice, &ancestor)) in slices.iter().zip(ancestors).enumerate().rev() {
            scopes.push((
                SlicePath(ancestors[..depth].to_vec()),
                slice,
                Some(SectionIndex(ancestor)),
            ));
        }
        scopes.into_iter().flat_map(|(path, slice, skip)| {
            slice
                .iter()
                .enumerate()
                .map(|(index, section)| (SectionIndex(index), section))
                .filter(move |&(index, _)| Some(index) != skip)
                .map(move |(index, section)| Reached {
                    section,
                    path: path.clone(),
                    index,
                })
        })
    }
}

/// A section's heading with its markers, such as `### Name`.
pub(crate) fn section_heading(section: &Section) -> String {
    format!("{} {}", "#".repeat(section.level().into()), section.name())
}

/// Parses a section heading like `"### Name"` into an exact `(level, name)`
/// address.
///
/// The marker run must be one-or-more `#`, immediately followed by whitespace,
/// then a non-empty name. `"###Name"` (no whitespace) and a bare name are both
/// rejected, so a malformed heading can never be silently reinterpreted.
///
/// # Errors
/// Returns [`Error::Lua`] when the marker run is absent, is not followed by
/// whitespace, or the name is empty.
fn parse_heading_address(heading: &str) -> Result<(usize, String)> {
    let stripped = heading.trim();
    let level = stripped.chars().take_while(|&c| c == '#').count();
    if level == 0 {
        return Err(Error::lua_message(format!(
            "section heading must include ### markers, got bare name: {stripped}"
        )));
    }
    // The `#` run is ASCII, so a byte slice at `level` is a valid boundary.
    let rest = &stripped[level..];
    // Checked before the whitespace gate: a marker-only heading (`###`) has
    // no name to parse whether or not whitespace followed the markers.
    let name = rest.trim();
    if name.is_empty() {
        return Err(Error::lua_message(format!(
            "section heading has no name: {stripped}"
        )));
    }
    if !rest.starts_with(|c: char| c.is_whitespace()) {
        return Err(Error::lua_message(format!(
            "section heading must have whitespace after the {} markers: {stripped}",
            "#".repeat(level)
        )));
    }
    Ok((level, name.to_owned()))
}

/// Looks up a heading string like `"### Name"` in a caller's visible set,
/// read in place through `visible`, returning the single matching section
/// or `None`.
///
/// The heading is parsed into an exact `(level, name)` address; a section
/// matches only when BOTH its level and name are equal. A heading outside the
/// visible set and a heading that names no section anywhere are the same
/// `None`, because only the visible set is ever searched. More than one match
/// is rejected, so an ambiguous heading never resolves to an arbitrary first
/// hit.
///
/// # Errors
/// Returns [`Error::Lua`] when the heading is malformed (see
/// [`parse_heading_address`]) or when more than one visible section matches
/// the exact address.
pub(crate) fn find_visible<'a>(heading: &str, visible: Visible<'a>) -> Result<Option<&'a Section>> {
    find_one(heading, visible.iter(), |section| *section, "visible")
}

/// [`find_visible`] over a [`Reachable`] view: the single reachable section
/// at `heading`'s exact address, with its position, or `None`.
///
/// # Errors
/// Returns [`Error::Lua`] when the heading is malformed (see
/// [`parse_heading_address`]) or when more than one reachable section
/// matches the exact address.
pub(crate) fn find_reachable<'a>(
    heading: &str,
    reach: Reachable<'a>,
) -> Result<Option<Reached<'a>>> {
    find_one(
        heading,
        reach.iter(),
        |reached| reached.section,
        "reachable",
    )
}

/// The one candidate whose section has `heading`'s exact address, or
/// `None`; `set` names the candidates in the ambiguity error.
fn find_one<'a, T>(
    heading: &str,
    candidates: impl Iterator<Item = T>,
    section: impl Fn(&T) -> &'a Section,
    set: &str,
) -> Result<Option<T>> {
    let (level, name) = parse_heading_address(heading)?;
    let mut matches = candidates.filter(|candidate| {
        let section = section(candidate);
        usize::from(section.level()) == level && section.name() == name
    });
    let found = matches.next();
    if found.is_some() && matches.next().is_some() {
        return Err(Error::lua_message(format!(
            "section heading `{}` is ambiguous; more than one {set} section matches {} {name}",
            heading.trim(),
            "#".repeat(level)
        )));
    }
    Ok(found)
}

/// The miss error for `heading`, listing `available` in order and nothing
/// else, so the error channel cannot leak the rest of the document's
/// structure.
fn not_found<'a>(heading: &str, available: impl Iterator<Item = &'a Section>) -> Error {
    let available: Vec<String> = available.map(section_heading).collect();
    Error::lua_message(format!(
        "section heading `{}` not found; available sections: {}",
        heading.trim(),
        available.join(", ")
    ))
}

/// [`find_visible`] where a heading with no visible match is an error, for
/// the surfaces that must reach a section, such as `call` and `jump`.
///
/// # Errors
/// Returns [`Error::Lua`] for every [`find_visible`] error, and when no
/// visible section matches the exact address. The not-found message lists the
/// view's sections in its order and nothing else, so the error channel
/// cannot leak the rest of the document's structure.
pub(crate) fn resolve_visible<'a>(heading: &str, visible: Visible<'a>) -> Result<&'a Section> {
    find_visible(heading, visible)?.ok_or_else(|| not_found(heading, visible.iter()))
}

/// [`find_reachable`] where a heading with no reachable match is an error,
/// for `require`.
///
/// # Errors
/// Returns [`Error::Lua`] for every [`find_reachable`] error, and when no
/// reachable section matches the exact address. The not-found message
/// lists the reach in its order and nothing else.
pub(crate) fn resolve_reachable<'a>(heading: &str, reach: Reachable<'a>) -> Result<Reached<'a>> {
    find_reachable(heading, reach)?
        .ok_or_else(|| not_found(heading, reach.iter().map(|reached| reached.section)))
}

#[cfg(test)]
#[path = "heading_address-tests.rs"]
mod tests;
