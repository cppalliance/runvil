//! Section paths in a prompt: the forms a path takes, and the one lookup
//! every function that names a section resolves through.
//!
//! A section's Lua names another section by a path of titles. `name` is
//! the direct child with that title and `../name` is the sibling, both
//! counted from an [`Origin`], the section the path starts at. `/a/b`
//! starts at the top of the prompt and names one title per level, so it
//! names the same section from everywhere. The grammar of steps, escapes,
//! and trimming is `runvil_markdown::split`, which a path over parsed
//! text shares.
//!
//! Which forms a function takes is that function's rule, so the module
//! has one entry per rule: [`resolve_neighbor`] for the functions that run
//! a section, which take the two relative forms; [`find`] for the `prompt`
//! reads, which take all three; and [`resolve_from_top`] for `require`,
//! which takes only a path from the top. A well-formed path that names no
//! section is a miss, and its error says where the lookup looked and
//! lists nothing.

use runvil_markdown::{Steps, escape_title, split};

use crate::prompt::{Prompt, Section, detail};
use crate::section_pos::{SectionIndex, SectionPos};
use crate::{Error, Result, error};

/// The section a relative path starts at.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Origin<'a> {
    /// The H1, whose children are the top-level sections and which has no
    /// siblings.
    H1,
    /// The section at this position.
    Section(&'a SectionPos),
}

impl Origin<'_> {
    /// The direct child titled `title`.
    fn child(self, prompt: &Prompt, title: &str) -> Option<SectionPos> {
        match self {
            Origin::H1 => position(detail::sections(prompt), title, None).map(SectionPos::top),
            Origin::Section(pos) => {
                let children = pos.section(prompt)?.children();
                position(children, title, None).map(|index| pos.child(index))
            }
        }
    }

    /// The sibling titled `title`: another section under the same parent,
    /// never the section itself.
    fn sibling(self, prompt: &Prompt, title: &str) -> Option<SectionPos> {
        match self {
            Origin::H1 => None,
            Origin::Section(pos) => {
                position(pos.siblings(prompt), title, Some(pos.index)).map(|index| SectionPos {
                    slice: pos.slice.clone(),
                    index,
                })
            }
        }
    }
}

/// The index of the section titled `title` in `slice`, passing over the
/// section at `skip`. Sibling titles are unique, so there is at most one.
fn position(slice: &[Section], title: &str, skip: Option<SectionIndex>) -> Option<SectionIndex> {
    slice
        .iter()
        .enumerate()
        .map(|(index, section)| (SectionIndex::new(index), section))
        .find(|(index, section)| Some(*index) != skip && section.name() == title)
        .map(|(index, _)| index)
}

/// What a well-formed path named.
#[derive(Debug)]
pub(crate) enum Lookup {
    /// The section at this position.
    Found(SectionPos),
    /// No section. The error is the not-found error for the path.
    Missing(Error),
}

impl Lookup {
    /// The section's position, for a function that must reach one.
    ///
    /// # Errors
    /// Returns the not-found error of a miss.
    pub(crate) fn required(self) -> Result<SectionPos> {
        match self {
            Lookup::Found(pos) => Ok(pos),
            Lookup::Missing(error) => Err(error),
        }
    }

    /// The section's position, or `None` for a miss.
    pub(crate) fn found(self) -> Option<SectionPos> {
        match self {
            Lookup::Found(pos) => Some(pos),
            Lookup::Missing(_) => None,
        }
    }
}

/// Resolves a path that names a neighbor of `origin`: `name` for a direct
/// child or `../name` for a sibling. These two forms are a section's
/// visible set, and they are all that `call`, `jump`, `tasks.spawn`, and
/// `tasks.fanout` take.
///
/// # Errors
/// Returns a `LuaRuntime` failure when the path is malformed, starts with
/// `/`, is neither relative form, or names no section.
pub(crate) fn resolve_neighbor(
    given: &str,
    prompt: &Prompt,
    origin: Origin<'_>,
) -> Result<SectionPos> {
    let path = titles(given)?;
    if path.from_top {
        return Err(error::make_lua_runtime_error(format!(
            "section path \"{given}\" starts with \"/\"; only require and prompt.section take a \
             path from the top"
        )));
    }
    neighbor(given, &path.titles, prompt, origin)?.required()
}

/// Looks up a path a `prompt` read takes: either relative form from
/// `origin`, or a path from the top, which names any section.
///
/// # Errors
/// Returns a `LuaRuntime` failure when the path is malformed, or is
/// relative and neither relative form. A path that names no section is a
/// [`Lookup::Missing`], not an error.
pub(crate) fn find(given: &str, prompt: &Prompt, origin: Origin<'_>) -> Result<Lookup> {
    let path = titles(given)?;
    if path.from_top {
        from_top(given, &path.titles, prompt)
    } else {
        neighbor(given, &path.titles, prompt, origin)
    }
}

/// Resolves a path from the top of the prompt, the only form `require`
/// takes. It names the same section wherever it is written.
///
/// # Errors
/// Returns a `LuaRuntime` failure when the path is malformed, does not
/// start with `/`, or names no section.
pub(crate) fn resolve_from_top(given: &str, prompt: &Prompt) -> Result<SectionPos> {
    let path = titles(given)?;
    if !path.from_top {
        return Err(error::make_lua_runtime_error(format!(
            "section path \"{given}\" does not start with \"/\"; require takes a path from the \
             top, such as \"/helpers\""
        )));
    }
    from_top(given, &path.titles, prompt)?.required()
}

/// The path from the top that names the section at `pos`, such as
/// `/Main/Kid`: how the Engine prints a section's address.
pub(crate) fn path_from_top(prompt: &Prompt, pos: &SectionPos) -> String {
    let mut path = String::new();
    let mut slice = detail::sections(prompt);
    for &index in pos.slice.indices().iter().chain([&pos.index.get()]) {
        let Some(section) = slice.get(index) else {
            break;
        };
        path.push('/');
        path.push_str(&escape_title(section.name()));
        slice = section.children();
    }
    path
}

/// A path's titles, after the checks every path in a prompt shares.
struct Titles {
    /// Whether the path starts with `/`.
    from_top: bool,
    /// The title of each step, in order. There is at least one.
    titles: Vec<String>,
}

/// Splits `given` into its titles.
///
/// # Errors
/// Returns a `LuaRuntime` failure for a path the grammar refuses, and for
/// the two forms only a path over parsed text has: `//` and an index.
fn titles(given: &str) -> Result<Titles> {
    let Steps { from_top, steps } = split(given)
        .map_err(|message| error::make_lua_runtime_error(format!("section {message}")))?;
    if steps.iter().any(|step| step.deep) {
        return Err(error::make_lua_runtime_error(format!(
            "section path \"{given}\" has an empty step"
        )));
    }
    if steps.iter().any(|step| step.index.is_some()) {
        return Err(error::make_lua_runtime_error(format!(
            "section path \"{given}\" has an index; a path in a prompt takes none"
        )));
    }
    Ok(Titles {
        from_top,
        titles: steps.into_iter().map(|step| step.title).collect(),
    })
}

/// What a lookup wanted of the section it searched.
#[derive(Clone, Copy)]
enum Wanted {
    /// A direct child with the title.
    Child,
    /// A sibling with the title.
    Sibling,
}

/// Looks up the relative path whose titles are `titles` from `origin`.
///
/// # Errors
/// Returns a `LuaRuntime` failure when the titles are neither `name` nor
/// `..` and a name.
fn neighbor(given: &str, titles: &[String], prompt: &Prompt, origin: Origin<'_>) -> Result<Lookup> {
    let (wanted, title) = match titles {
        [name] if name == ".." => {
            return Err(error::make_lua_runtime_error(format!(
                "section path \"{given}\" needs a name after \"..\""
            )));
        }
        [name] => (Wanted::Child, name),
        [parent, name] if parent == ".." && name != ".." => (Wanted::Sibling, name),
        _ => {
            return Err(error::make_lua_runtime_error(format!(
                "section path \"{given}\" has more than one name; a relative path is a child's \
                 name, or \"../\" and a sibling's name"
            )));
        }
    };
    let found = match wanted {
        Wanted::Child => origin.child(prompt, title),
        Wanted::Sibling => origin.sibling(prompt, title),
    };
    Ok(match found {
        Some(pos) => Lookup::Found(pos),
        None => Lookup::Missing(make_not_found_error(given, prompt, origin, wanted, title)),
    })
}

/// Looks up the path from the top whose titles are `titles`: one child
/// per step, starting from the top-level sections.
///
/// # Errors
/// Returns a `LuaRuntime` failure when a title is `..`, and an `Internal`
/// failure for a path with no step.
fn from_top(given: &str, titles: &[String], prompt: &Prompt) -> Result<Lookup> {
    if titles.iter().any(|title| title == "..") {
        return Err(error::make_lua_runtime_error(format!(
            "section path \"{given}\" uses \"..\"; a path from the top takes none"
        )));
    }
    let mut reached: Option<SectionPos> = None;
    for title in titles {
        let origin = reached.as_ref().map_or(Origin::H1, Origin::Section);
        let Some(pos) = origin.child(prompt, title) else {
            // The walk stopped at `origin`, which has no such child.
            let error = make_not_found_error(given, prompt, origin, Wanted::Child, title);
            return Ok(Lookup::Missing(error));
        };
        reached = Some(pos);
    }
    reached
        .map(Lookup::Found)
        .ok_or(error::make_internal_error("a section path has no step"))
}

/// The error for a path that names no section: the path as it was given,
/// the section that was searched, and what was wanted there. It lists no
/// other section, so the error cannot show the rest of the prompt's
/// structure.
fn make_not_found_error(
    given: &str,
    prompt: &Prompt,
    searched: Origin<'_>,
    wanted: Wanted,
    title: &str,
) -> Error {
    let looked = match (searched, wanted) {
        (Origin::H1, Wanted::Child) => format!("the top level has no section `{title}`"),
        (Origin::H1, Wanted::Sibling) => "the H1 has no siblings".to_owned(),
        (Origin::Section(pos), Wanted::Child) => {
            format!("`{}` has no child `{title}`", path_from_top(prompt, pos))
        }
        (Origin::Section(pos), Wanted::Sibling) => {
            format!("`{}` has no sibling `{title}`", path_from_top(prompt, pos))
        }
    };

    error::make_lua_runtime_error(format!("section `{}` not found: {looked}", given.trim()))
}

#[cfg(test)]
#[path = "section_path-tests.rs"]
mod tests;
