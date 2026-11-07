//! Rendering an error and its cause chain as one line of text for a
//! person or a model.
//!
//! A `thiserror` variant renders only its own message; its `#[source]` is
//! reachable through `source()` but not repeated in the text. Where the
//! text leaves the program - a run's failure in the log, a failure pushed
//! to a session's client - the chain is walked here so the reader sees
//! the cause and not just the outermost frame.

use std::error::Error;

/// Renders an error's text followed by the text of each cause in its
/// `source()` chain, separated by `: `.
///
/// A cause is skipped when its text already appears in the text rendered
/// so far. Some error variants copy their source's text into their own
/// message. Appending such a cause again would print its text twice. The
/// check is a plain substring test.
#[must_use]
pub fn display_chain(error: &dyn Error) -> String {
    let mut rendered = error.to_string();
    let mut cause = error.source();
    while let Some(current) = cause {
        let text = current.to_string();
        if !text.is_empty() && !rendered.contains(&text) {
            rendered.push_str(": ");
            rendered.push_str(&text);
        }
        cause = current.source();
    }
    rendered
}

#[cfg(test)]
mod tests {
    //! Tests for `display_chain` rendering of an error and its causes.

    use super::display_chain;

    /// A leaf cause with its own text.
    #[derive(Debug, thiserror::Error)]
    #[error("disk gone")]
    struct Leaf;

    /// An outer error whose text does not mention its cause.
    #[derive(Debug, thiserror::Error)]
    #[error("the prompt could not be read")]
    struct Outer(#[source] Leaf);

    /// An outer error that copies its cause's text into its own message, the
    /// shape of a variant such as `LuaRuntime { message, source }`.
    #[derive(Debug, thiserror::Error)]
    #[error("lua runtime error: {message}")]
    struct Copying {
        message: String,
        #[source]
        source: Leaf,
    }

    #[test]
    fn a_two_level_chain_renders_the_cause_after_the_outer_text() {
        let rendered = display_chain(&Outer(Leaf));
        assert_eq!(
            rendered, "the prompt could not be read: disk gone",
            "the cause follows the outer text after a colon"
        );
    }

    #[test]
    fn a_cause_already_quoted_by_the_outer_text_is_not_appended_twice() {
        let error = Copying {
            message: "disk gone".to_owned(),
            source: Leaf,
        };
        let rendered = display_chain(&error);
        assert_eq!(
            rendered, "lua runtime error: disk gone",
            "a cause whose text the outer message already includes is skipped"
        );
        assert_eq!(
            rendered.matches("disk gone").count(),
            1,
            "the cause text appears exactly once"
        );
    }

    #[test]
    fn a_leaf_renders_as_its_own_text() {
        assert_eq!(display_chain(&Leaf), "disk gone");
    }
}
