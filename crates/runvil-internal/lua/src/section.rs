//! What the VM exchanges with its caller about sections: a block's result, a
//! `call` or `jump` target, the text a prompt read sees, and the library
//! sections `require` loads.

use std::num::NonZeroU32;

use mlua::{MultiValue, Value};

use crate::error::{Result, make_lua_runtime_error};

/// Outcome of a Lua block that may invoke `jump`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LuaBlockResult {
    /// Normal completion with an optional scalar return.
    Returned(Option<String>),
    /// `jump` transferred control to the section this path names, called
    /// on this prompt line, which is `None` when the call found no author
    /// frame running.
    Jump(String, Option<NonZeroU32>),
}

/// Renders a returned Lua scalar as the section's result string. Tables and other
/// non-scalar returns are errors.
fn value_to_string(value: &Value) -> Result<String> {
    match value {
        Value::String(s) => Ok(s.to_string_lossy()),
        Value::Integer(i) => Ok(i.to_string()),
        Value::Number(n) => Ok(n.to_string()),
        Value::Boolean(b) => Ok(b.to_string()),
        other => Err(make_lua_runtime_error(format!(
            "cannot return a {} as a result",
            other.type_name()
        ))),
    }
}

/// Converts only the first returned Lua value into an optional scalar string.
///
/// Later values are ignored. A missing or nil first value returns `None`.
///
/// # Errors
/// Returns a `LuaRuntime` failure when the first value is not a supported
/// scalar.
pub(crate) fn scalar_return(returned: MultiValue) -> Result<Option<String>> {
    match returned.into_iter().next() {
        None | Some(Value::Nil) => Ok(None),
        Some(value) => value_to_string(&value).map(Some),
    }
}

/// Reads a `call` / `jump` target: a section path, as the author wrote it.
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

/// What a prompt read sees of one section.
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
/// string; `require`'s cache keys on that identity, never on the path's
/// text, so a library loads once however its path is spaced.
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
    /// Which library this is: the cache key.
    pub key: LibraryKey,
    /// The library section's path from the top of the prompt, such as
    /// `/helpers`, which its load reports and a cycle error name.
    pub path: String,
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
    /// The prompt line the prose template starts on, which its render
    /// receives; `None` when no prose comes right before the block.
    pub line: Option<NonZeroU32>,
}
