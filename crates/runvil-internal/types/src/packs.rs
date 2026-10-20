//! The Pack identity vocabulary: [`PackId`] and its parse
//! error, plus [`Prelude`], the Lua source a declared Pack hands
//! the Engine.
//!
//! A Pack ships in a crate and is installed under a local, one-segment
//! [`PackId`], such as `web`, and every tool it offers sits under that
//! name, such as `web/fetch`. The Engine knows Packs by identity alone:
//! a prompt declares them by plain name, which makes each one required
//! and runs its prelude, and a tool names its Pack through its
//! [`ToolId`] prefix. A run's catalog holds the tools of every Pack the
//! caller can serve, declared or not. The Pack contract - the
//! `PluginInfo`, the `Plugin` trait, and the services a Pack reads - is
//! in `runvil-plugin`, and installing Packs is the caller's; the
//! Engine never builds a Pack.

use std::str::FromStr;

use crate::names::{GlobalName, GlobalNameErrorKind};
use crate::tools::ToolId;

#[cfg(test)]
#[path = "packs-tests.rs"]
mod tests;

/// The local name a Pack is installed under: one segment, such as `web`.
///
/// A Pack's id is the first segment of every tool id it contributes:
/// `web` contributes `web/fetch`. An id is the name alone, and a name
/// resolves to the only installed Pack of that name. A `@` version
/// marker in the id is a parse error.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub struct PackId(GlobalName);

impl PackId {
    /// Parses a Pack identity, requiring exactly one segment.
    ///
    /// # Errors
    /// Returns [`PackIdError`] when the id has more than one segment
    /// ([`PackIdErrorKind::SegmentCount`]), is empty or has an empty
    /// segment ([`PackIdErrorKind::Empty`]), a segment contains a control
    /// byte ([`PackIdErrorKind::Control`]), or a segment contains any
    /// other character than a lowercase ASCII letter, a digit, `-`, `_`,
    /// or `.` ([`PackIdErrorKind::Charset`]).
    pub fn parse(name: &str) -> Result<PackId, PackIdError> {
        let name =
            GlobalName::parse(name).map_err(|e| PackIdError::from_global_name_kind(e.kind()))?;
        if name.segments().len() != 1 {
            return Err(PackIdError {
                kind: PackIdErrorKind::SegmentCount,
                reason: "a Pack id is one segment, such as `web`, with no '/'",
            });
        }
        Ok(PackId(name))
    }

    /// Builds an identity from the first segment of a validated tool id.
    ///
    /// Crate-internal: backs [`crate::tools::ToolId::pack`]. The source
    /// tool id was validated at parse, so its first segment is already a
    /// valid Pack id.
    pub(crate) fn from_segment(segment: &str) -> PackId {
        PackId(GlobalName::from_validated_segment(segment))
    }

    /// Returns whether `tool` belongs to this Pack.
    ///
    /// A tool belongs to a Pack when the tool id's first segment is the
    /// Pack's id, so `web/fetch` belongs to `web`. Every tool a Pack
    /// contributes belongs to it. The caller must check this when it
    /// assembles the run's catalog.
    #[must_use]
    pub fn contains(&self, tool: &ToolId) -> bool {
        tool.pack_segment() == self.0.first()
    }
}

impl FromStr for PackId {
    type Err = PackIdError;

    fn from_str(name: &str) -> Result<PackId, PackIdError> {
        PackId::parse(name)
    }
}

impl std::fmt::Display for PackId {
    /// The one-segment string form, such as `web`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl serde::Serialize for PackId {
    /// Serializes the id as its one-segment string.
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for PackId {
    /// Deserializes the id from its one-segment string and validates it
    /// the same way `PackId::parse` does. An invalid string, or any value
    /// that is not a string, such as a map or a number, fails
    /// deserialization with a message naming the plain-name form.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(PackIdVisitor)
    }
}

/// Deserializes a [`PackId`] from a string alone. It asks for any value,
/// not a string, because YAML would otherwise read a scalar such as `42` or
/// `true` as its text.
struct PackIdVisitor;

impl serde::de::Visitor<'_> for PackIdVisitor {
    type Value = PackId;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a Pack's plain name, such as `web`")
    }

    fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<PackId, E> {
        PackId::parse(text)
            .map_err(|error| E::custom(format!("invalid Pack id `{text}`: {}", error.reason)))
    }
}

/// Classifies a [`PackIdError`] so callers can handle each kind of failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PackIdErrorKind {
    /// The id had more than one segment.
    SegmentCount,
    /// A segment was empty.
    Empty,
    /// A segment contained a control byte.
    Control,
    /// A segment contained a character other than a lowercase ASCII letter,
    /// a digit, `-`, `_`, or `.` that is not a control byte, such as an
    /// uppercase letter, `@`, or a non-ASCII character.
    Charset,
}

/// The reason a [`PackId`] failed to parse.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PackIdError {
    /// A stable classification of why the id was rejected.
    kind: PackIdErrorKind,
    /// A human-readable reason.
    reason: &'static str,
}

impl PackIdError {
    /// Returns the stable classification of this error.
    #[must_use]
    pub fn kind(&self) -> PackIdErrorKind {
        self.kind
    }

    /// Maps a global-name rejection onto the Pack-id error vocabulary.
    fn from_global_name_kind(global_kind: GlobalNameErrorKind) -> PackIdError {
        let (kind, reason) = match global_kind {
            GlobalNameErrorKind::Empty => (PackIdErrorKind::Empty, "segments must not be empty"),
            GlobalNameErrorKind::Control => (
                PackIdErrorKind::Control,
                "segments must not contain a control character",
            ),
            GlobalNameErrorKind::Charset => (
                PackIdErrorKind::Charset,
                "segments may contain only lowercase ASCII letters, digits, '-', '_', '.'",
            ),
        };
        PackIdError { kind, reason }
    }
}

impl std::fmt::Display for PackIdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid Pack id: {}", self.reason)
    }
}

impl std::error::Error for PackIdError {}

/// Holds the Lua source a declared Pack adds to every section's VM.
///
/// A prelude defines tables and functions, such as `sh.run(script)`, that
/// call the Pack's own tools through `tools.call`. The Engine runs
/// the source knowing only the Pack's id, which names the prelude in
/// tracebacks and error messages. The chunk receives the Pack's local
/// name as `...`, so `local pack = ...` lets it call
/// `tools.call(pack .. "/ask")` whatever name the Pack was installed
/// under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prelude {
    /// The Pack that contributed the source.
    pack: PackId,
    /// The Lua source, compiled from text in every section VM.
    source: String,
}

impl Prelude {
    /// Pairs a Pack's id with the prelude source it contributes.
    #[must_use]
    pub fn new(pack: PackId, source: impl Into<String>) -> Prelude {
        Prelude {
            pack,
            source: source.into(),
        }
    }

    /// Returns the id of the Pack that contributed this prelude.
    #[must_use]
    pub fn pack(&self) -> &PackId {
        &self.pack
    }

    /// Returns the prelude's Lua source.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }
}
