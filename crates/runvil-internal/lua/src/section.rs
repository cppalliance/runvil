//! What the VM exchanges with its caller about sections: a block's result, a
//! `call` or `jump` target, the text a prompt read sees, and the library
//! sections `require` loads.

use mlua::Value;

/// Outcome of a Lua block that may invoke `jump`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LuaBlockResult {
    /// Normal completion with an optional scalar return.
    Returned(Option<String>),
    /// `jump` transferred control to this heading (`## Name`).
    Jump(String),
}

/// Resolves a `call` / `jump` target from a heading string.
///
/// # Errors
/// Returns a Lua error when the value is not a string.
pub(crate) fn resolve_section_target(value: Value) -> mlua::Result<String> {
    match value {
        Value::String(s) => Ok(s.to_str()?.to_owned()),
        other => Err(mlua::Error::external(format!(
            "section target must be a string, got {}",
            other.type_name()
        ))),
    }
}

/// What a prompt read sees of one visible section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionText {
    /// The section's own raw Markdown.
    pub markdown: String,
    /// Its pre-parsed list items; empty for a section that isn't a list.
    pub items: Vec<String>,
}

/// Which library a `require` resolved to, opaque to this crate.
///
/// The caller encodes a library section's position in the identity
/// string; `require`'s cache keys on that identity, never on the heading
/// text, so one heading resolved from two positions names two libraries.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LibraryKey(String);

impl LibraryKey {
    /// Builds a key whose stable identity is `identity`.
    #[must_use]
    pub fn new(identity: String) -> Self {
        Self(identity)
    }

    /// Returns the identity the key was built from.
    #[must_use]
    pub fn identity(&self) -> &str {
        &self.0
    }
}

/// What `require` loads from one library section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Library {
    /// Which library this is: the cache key, and the position a `require`
    /// made while it loads resolves from.
    pub key: LibraryKey,
    /// The library section's heading with its markers, such as
    /// `## helpers`, which its load reports and a cycle error name.
    pub heading: String,
    /// The section's blocks in order.
    pub blocks: Vec<LibraryBlock>,
}

/// One block of a library section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryBlock {
    /// The block's compiled Lua.
    pub program: crate::LuaProgram,
    /// The block's prose template, the Markdown before it.
    pub prose: String,
}
