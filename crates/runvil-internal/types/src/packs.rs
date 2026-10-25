//! The Pack identity vocabulary: [`PackId`] and its parse error, the
//! [`RESERVED_PACK_IDS`] no Pack may take, [`PackRef`], one entry of a
//! prompt's `uses:` list, and [`Prelude`], the Lua source a declared Pack
//! hands the Engine.
//!
//! A Pack has a fully qualified name such as `runvil/web`, and its
//! [`PackId`] is that name's second segment, `web`. Every tool it offers
//! sits under that id, such as `web/fetch`. The Engine knows Packs by
//! identity alone: a prompt declares them under `uses:`, each entry a
//! [`PackRef`], which makes each one required and runs its prelude, and a
//! tool names its Pack through its [`ToolId`] prefix. Which Packs a run
//! holds, and so which tools its catalog offers, is the caller's choice.
//! The Plugin contract - the `PackInfo`, the `Pack` trait, and the
//! services a Pack reads - is in `runvil-plugin`, and building Packs is
//! the caller's; the Engine never builds a Pack.

use std::str::FromStr;

use crate::names::{GlobalName, GlobalNameErrorKind};
use crate::tools::ToolId;

#[cfg(test)]
#[path = "packs-tests.rs"]
mod tests;

/// The Pack ids the Engine holds tools under itself, which no Pack of any
/// vendor may take: `adhoc`, which holds a section's own tools, and
/// `dynamic`, for the Engine's discovery tools.
///
/// They are strings because a [`PackId`] cannot be built in a const.
pub const RESERVED_PACK_IDS: &[&str] = &["adhoc", "dynamic"];

/// A Pack's id: one segment, the second segment of the Pack's fully
/// qualified name, such as `web` for `runvil/web`.
///
/// A Pack's id is the first segment of every tool id it contributes:
/// `web` contributes `web/fetch`. A run holds at most one Pack with a
/// given id, so within a run the id names that Pack. A `@` version
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

    /// Returns whether this id is reserved, such as `adhoc`: the Engine
    /// holds the tools under it itself, so no Pack may take it.
    #[must_use]
    pub fn is_reserved(&self) -> bool {
        RESERVED_PACK_IDS.contains(&self.0.first())
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
    /// deserialization with a message naming the one-segment form.
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
        formatter.write_str("a Pack id, such as `web`")
    }

    fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<PackId, E> {
        PackId::parse(text)
            .map_err(|error| E::custom(format!("invalid Pack id `{text}`: {}", error.reason)))
    }
}

/// The vendor a bare `uses:` entry means.
const RUNVIL_VENDOR: &str = "runvil";

/// What a `uses:` entry may be, for the refusal of any other form.
const ENTRY_FORMS: &str = "a Pack id, such as `web`, or vendor/name, such as `acme/web`";

/// One `uses:` entry: a bare Pack id, which means a runvil Pack, or
/// vendor/name.
///
/// A bare entry, such as `web`, always names the `runvil` Pack of that
/// id, and `vendor/name`, such as `acme/poop`, is the only way to name
/// another vendor's Pack. `web` and `runvil/web` are the same entry. Only
/// deserializing a `uses:` entry builds one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackRef {
    /// The vendor, `runvil` for a bare entry.
    vendor: String,
    /// The Pack id, the entry's last segment.
    id: PackId,
}

impl PackRef {
    /// Returns the vendor, which is `"runvil"` when the entry is bare.
    #[must_use]
    pub fn vendor(&self) -> &str {
        &self.vendor
    }

    /// Returns the Pack id, the entry's last segment.
    #[must_use]
    pub fn id(&self) -> &PackId {
        &self.id
    }

    /// Parses one entry: one or two valid segments.
    fn parse(text: &str) -> Result<PackRef, &'static str> {
        let name = GlobalName::parse(text)
            .map_err(|error| PackIdError::from_global_name_kind(error.kind()).reason)?;
        let (vendor, id) = match name.segments() {
            [id] => (RUNVIL_VENDOR, id),
            [vendor, id] => (vendor.as_str(), id),
            _ => {
                return Err(
                    "a uses entry is a Pack id, such as `web`, or vendor/name, such as `acme/web`",
                );
            }
        };
        Ok(PackRef {
            vendor: vendor.to_owned(),
            id: PackId::from_segment(id),
        })
    }
}

impl<'de> serde::Deserialize<'de> for PackRef {
    /// Deserializes one `uses:` entry from its string. An invalid string,
    /// or any value that is not a string, such as a map or a number, fails
    /// deserialization with a message naming both entry forms.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(PackRefVisitor)
    }
}

/// Deserializes a [`PackRef`] from a string alone, asking for any value
/// for the reason [`PackIdVisitor`] does.
struct PackRefVisitor;

impl serde::de::Visitor<'_> for PackRefVisitor {
    type Value = PackRef;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(ENTRY_FORMS)
    }

    fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<PackRef, E> {
        PackRef::parse(text)
            .map_err(|reason| E::custom(format!("invalid uses entry `{text}`: {reason}")))
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
/// tracebacks and error messages. The chunk receives the Pack's id as
/// `...`, so `local pack = ...` lets it call `tools.call(pack .. "/ask")`
/// without spelling the id out.
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
