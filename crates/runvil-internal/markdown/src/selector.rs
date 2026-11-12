//! Selectors: the word that names a kind of block.

use crate::{Block, Kind};

/// What a selector matches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selector {
    /// Every block of one kind.
    Kind(Kind),
    /// A fence whose language is exactly this one. The empty string is a
    /// fence with no language.
    Fence(String),
}

impl Selector {
    /// Reads a selector: a kind, or `fence` and a language. The language
    /// `""`, written as two quote characters, is that of a fence with
    /// none. Whitespace around the selector and extra spaces between its
    /// two words are ignored.
    ///
    /// # Errors
    /// Returns the message for a first word that is no kind, a language
    /// on a kind other than `fence`, or more than one language.
    pub fn parse(given: &str) -> Result<Selector, String> {
        let mut words = given.split_whitespace();
        let word = words.next().unwrap_or_default();
        let Some(kind) = Kind::from_name(word) else {
            let kinds = Kind::ALL.map(Kind::name).join(", ");
            return Err(format!("unknown kind \"{word}\"; kinds are {kinds}"));
        };
        match (kind, words.next(), words.next()) {
            (_, None, _) => Ok(Selector::Kind(kind)),
            (Kind::Fence, Some(lang), None) => Ok(Selector::Fence(
                if lang == "\"\"" { "" } else { lang }.to_owned(),
            )),
            (Kind::Fence, Some(_), Some(_)) => Err(format!(
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
    /// A kind compares by its name, so a `kind` that names none matches
    /// nothing. A language compares exactly and case-sensitively, and a
    /// block with no `lang` has no language to match.
    #[must_use]
    pub fn matches(&self, kind: &str, lang: Option<&str>) -> bool {
        match self {
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

#[cfg(test)]
mod tests {
    use super::Selector;
    use crate::{Block, Kind};

    fn fence(lang: &str) -> Block {
        Block::Fence {
            line: 1,
            lang: lang.to_owned(),
            info: lang.to_owned(),
            text: String::new(),
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
                    "unknown kind \"{word}\"; kinds are frontmatter, section, prose, fence, list, rule"
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
        let any = Selector::Kind(Kind::Fence);
        assert!(any.selects(&fence("lua")));
        assert!(any.selects(&fence("")));
        assert!(!any.selects(&Block::Rule { line: 1 }));
        assert!(Selector::Kind(Kind::Rule).selects(&Block::Rule { line: 1 }));
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
