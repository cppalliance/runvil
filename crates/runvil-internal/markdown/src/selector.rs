//! Selectors: the word that names a kind of block, or `*` for every
//! block.

use crate::index::{at_lines, position};
use crate::{Block, Kind};

/// What a selector matches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selector {
    /// Every block, of whatever kind. It is written `*`.
    Any,
    /// Every block of one kind.
    Kind(Kind),
    /// A fence whose language is exactly this one. The empty string is a
    /// fence with no language.
    Fence(String),
}

/// The word that selects every block.
const ANY: &str = "*";

impl Selector {
    /// Reads a selector: `*`, a kind, or `fence` and a language. The
    /// language `""`, written as two quote characters, is that of a fence
    /// with none. Whitespace around the selector and extra spaces between
    /// its two words are ignored.
    ///
    /// # Errors
    /// Returns the message for a first word that is neither `*` nor a
    /// kind, a language on anything other than `fence`, or more than one
    /// language.
    pub fn parse(given: &str) -> Result<Selector, String> {
        let mut words = given.split_whitespace();
        let word = words.next().unwrap_or_default();
        let kind = Kind::from_name(word);
        if kind.is_none() && word != ANY {
            let kinds = Kind::ALL.map(Kind::name).join(", ");
            return Err(format!(
                "unknown kind \"{word}\"; kinds are {kinds}, and {ANY} selects every block"
            ));
        }
        match (kind, words.next(), words.next()) {
            (None, None, _) => Ok(Selector::Any),
            (Some(kind), None, _) => Ok(Selector::Kind(kind)),
            (Some(Kind::Fence), Some(lang), None) => Ok(Selector::Fence(
                if lang == "\"\"" { "" } else { lang }.to_owned(),
            )),
            (Some(Kind::Fence), Some(_), Some(_)) => Err(format!(
                "a fence selector takes one language, got \"{given}\""
            )),
            _ => Err(format!(
                "only a fence selector takes a language, got \"{given}\""
            )),
        }
    }

    /// Whether the selector matches a block whose `kind` field is `kind`
    /// and whose `lang` field, when it has one, is `lang`.
    ///
    /// `*` matches whatever `kind` holds. A kind compares by its name, so
    /// a `kind` that names none matches no other selector. A language
    /// compares exactly and case-sensitively, and a block with no `lang`
    /// has no language to match.
    #[must_use]
    pub fn matches(&self, kind: &str, lang: Option<&str>) -> bool {
        match self {
            Selector::Any => true,
            Selector::Kind(wanted) => wanted.name() == kind,
            Selector::Fence(wanted) => kind == Kind::Fence.name() && lang == Some(wanted.as_str()),
        }
    }

    /// Whether the selector matches `block`.
    #[must_use]
    pub fn selects(&self, block: &Block) -> bool {
        let lang = match block {
            Block::Fence { lang, .. } => Some(lang.as_str()),
            _ => None,
        };
        self.matches(block.kind().name(), lang)
    }
}

/// Which of a selector's matches a lookup keeps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pick {
    /// The match at this position, counted from 0.
    Match(usize),
    /// None: the block is not there. The message says what is missing.
    Missing(String),
}

/// Picks one of the blocks a selector matched in one section.
///
/// `lines` holds the line of each match, in order, or `None` for a match
/// that records none. With `n`, the pick is the `n`th match. Without it,
/// the selector has to match exactly one block. `given` is the selector
/// as it was written, and `place` names the section in a miss.
///
/// # Errors
/// Returns the message that asks for an index when there is no `n` and
/// the selector matched several blocks. That is a choice only the caller
/// can make, not an absence.
pub fn pick(
    given: &str,
    place: &str,
    lines: &[Option<i64>],
    n: Option<i64>,
) -> Result<Pick, String> {
    let given = given.trim();
    let count = lines.len();
    if count == 0 {
        return Ok(Pick::Missing(format!(
            "no block in {place} matches selector \"{given}\""
        )));
    }
    match n {
        Some(n) => Ok(match position(count, n) {
            Some(at) => Pick::Match(at),
            None => Pick::Missing(format!(
                "selector \"{given}\" matches {count} block{} in {place}; there is no block {n}",
                if count == 1 { "" } else { "s" }
            )),
        }),
        None if count == 1 => Ok(Pick::Match(0)),
        None => Err(format!(
            "selector \"{given}\" matches {count} blocks{}; add an index, such as 1 for the first",
            at_lines(lines)
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{Pick, Selector, pick};
    use crate::{Block, Kind};

    #[test]
    fn without_an_index_the_pick_is_the_only_match() {
        assert_eq!(
            pick("fence", "\"/G\"", &[Some(20)], None),
            Ok(Pick::Match(0))
        );
        assert_eq!(
            pick(" list ", "\"/Guide/Templates\"", &[], None),
            Ok(Pick::Missing(
                "no block in \"/Guide/Templates\" matches selector \"list\"".to_owned()
            ))
        );
        assert_eq!(
            pick("fence", "\"/G\"", &[Some(20), Some(24), Some(30)], None),
            Err(
                "selector \"fence\" matches 3 blocks, at lines 20, 24, and 30; add an index, \
                 such as 1 for the first"
                    .to_owned()
            )
        );
        assert_eq!(
            pick("prose", "\"/G\"", &[None, Some(4)], None),
            Err(
                "selector \"prose\" matches 2 blocks; add an index, such as 1 for the first"
                    .to_owned()
            )
        );
    }

    #[test]
    fn with_an_index_the_pick_is_that_match_or_a_miss() {
        let lines = [Some(20), Some(24), Some(30)];
        assert_eq!(pick("fence", "\"/G\"", &lines, Some(3)), Ok(Pick::Match(2)));
        assert_eq!(
            pick("fence", "\"/G\"", &lines, Some(-1)),
            Ok(Pick::Match(2))
        );
        assert_eq!(
            pick("fence", "\"/G\"", &lines, Some(-3)),
            Ok(Pick::Match(0))
        );
        assert_eq!(
            pick("fence", "\"/Guide/Templates\"", &lines, Some(4)),
            Ok(Pick::Missing(
                "selector \"fence\" matches 3 blocks in \"/Guide/Templates\"; there is no \
                 block 4"
                    .to_owned()
            ))
        );
        assert_eq!(
            pick("fence", "section `Template`", &[Some(25)], Some(2)),
            Ok(Pick::Missing(
                "selector \"fence\" matches 1 block in section `Template`; there is no block 2"
                    .to_owned()
            ))
        );
        assert_eq!(
            pick("list", "section `Template`", &[], Some(1)),
            Ok(Pick::Missing(
                "no block in section `Template` matches selector \"list\"".to_owned()
            ))
        );
    }

    fn fence(lang: &str) -> Block {
        Block::Fence {
            line: 1,
            source: format!("```{lang}\n```\n"),
            lang: lang.to_owned(),
            info: lang.to_owned(),
            text: String::new(),
        }
    }

    fn rule() -> Block {
        Block::Rule {
            line: 1,
            source: "---\n".to_owned(),
        }
    }

    #[test]
    fn a_kind_alone_is_a_selector() {
        for kind in Kind::ALL {
            assert_eq!(Selector::parse(kind.name()), Ok(Selector::Kind(kind)));
        }
        assert_eq!(Selector::parse("  list\t"), Ok(Selector::Kind(Kind::List)));
    }

    #[test]
    fn a_star_alone_selects_every_block() {
        assert_eq!(Selector::parse("*"), Ok(Selector::Any));
        assert_eq!(Selector::parse(" *\t"), Ok(Selector::Any));
        assert!(Selector::Any.selects(&fence("lua")));
        assert!(Selector::Any.selects(&rule()));
        // Whatever the `kind` field holds, a name of no kind included.
        assert!(Selector::Any.matches("note", None));
        assert!(Selector::Any.matches("", None));
        for given in ["**", "*fence", "* *"] {
            assert!(Selector::parse(given).is_err(), "{given:?}");
        }
    }

    #[test]
    fn a_fence_selector_may_name_one_language() {
        assert_eq!(
            Selector::parse("fence markdown"),
            Ok(Selector::Fence("markdown".to_owned()))
        );
        assert_eq!(
            Selector::parse("  fence    c++ "),
            Ok(Selector::Fence("c++".to_owned()))
        );
        assert_eq!(
            Selector::parse("fence \"\""),
            Ok(Selector::Fence(String::new()))
        );
    }

    #[test]
    fn a_first_word_that_is_no_kind_is_refused() {
        for (given, word) in [
            ("fences", "fences"),
            ("fences lua", "fences"),
            ("Fence", "Fence"),
            ("code", "code"),
            ("", ""),
            ("   ", ""),
        ] {
            assert_eq!(
                Selector::parse(given),
                Err(format!(
                    "unknown kind \"{word}\"; kinds are frontmatter, section, prose, fence, list, \
                     rule, and * selects every block"
                )),
                "{given:?}"
            );
        }
    }

    #[test]
    fn a_language_on_any_other_kind_and_a_second_language_are_refused() {
        assert_eq!(
            Selector::parse("list lua"),
            Err("only a fence selector takes a language, got \"list lua\"".to_owned())
        );
        assert_eq!(
            Selector::parse("* lua"),
            Err("only a fence selector takes a language, got \"* lua\"".to_owned())
        );
        assert_eq!(
            Selector::parse("prose a b"),
            Err("only a fence selector takes a language, got \"prose a b\"".to_owned())
        );
        assert_eq!(
            Selector::parse("fence lua strict"),
            Err("a fence selector takes one language, got \"fence lua strict\"".to_owned())
        );
    }

    #[test]
    fn a_kind_matches_every_block_of_that_kind() {
        let fences = Selector::Kind(Kind::Fence);
        assert!(fences.selects(&fence("lua")));
        assert!(fences.selects(&fence("")));
        assert!(!fences.selects(&rule()));
        assert!(Selector::Kind(Kind::Rule).selects(&rule()));
        assert!(!Selector::Kind(Kind::Rule).matches("break", None));
    }

    #[test]
    fn a_language_matches_exactly_and_the_empty_one_matches_a_fence_with_none() {
        let lua = Selector::Fence("lua".to_owned());
        assert!(lua.selects(&fence("lua")));
        assert!(!lua.selects(&fence("Lua")));
        assert!(!lua.selects(&fence("luau")));
        assert!(!lua.selects(&fence("")));
        let unnamed = Selector::Fence(String::new());
        assert!(unnamed.selects(&fence("")));
        assert!(!unnamed.selects(&fence("lua")));
        // A block with no `lang` field has no language, not the empty one.
        assert!(!unnamed.matches("fence", None));
        assert!(!lua.matches("prose", Some("lua")));
    }
}
