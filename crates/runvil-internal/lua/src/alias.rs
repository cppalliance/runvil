//! The [`Alias`] type: an adhoc tool's name, checked against the
//! prompt-local alias grammar of [`runvil_types::names::is_alias`].

use runvil_types::names::is_alias;

use crate::error::{Result, lua_runtime};

/// A prompt-local alias that passed the supported wire grammar.
///
/// An alias never contains `/`, so it never reads as a tool id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Alias(String);

impl Alias {
    /// Validates `alias` against the wire grammar and wraps it.
    ///
    /// # Errors
    /// Returns a `LuaRuntime` failure when `alias` is empty, exceeds 64 bytes, starts
    /// with a non-letter, or contains a character other than a letter,
    /// digit, `_`, or `-` after its first byte.
    pub(crate) fn parse(alias: String) -> Result<Alias> {
        if is_alias(&alias) {
            Ok(Alias(alias))
        } else {
            Err(lua_runtime(format!(
                "invalid alias {alias:?}: expected [A-Za-z][A-Za-z0-9_-]{{0,63}}"
            )))
        }
    }
}

impl From<Alias> for String {
    fn from(alias: Alias) -> String {
        alias.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_alias_grammar_rejects_every_other_shape() {
        let error = Alias::parse("1search".to_owned()).expect_err("the message names the grammar");
        assert!(
            error
                .to_string()
                .contains("invalid alias \"1search\": expected [A-Za-z][A-Za-z0-9_-]{0,63}"),
            "unexpected message: {error}"
        );
    }
}
