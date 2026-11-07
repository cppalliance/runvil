//! Stable tool identity.

use std::str::FromStr;

use crate::error::Error;
use crate::names::{GlobalName, make_invalid_id_error};
use crate::packs::PackId;

/// Identifies a tool by its Pack's local name and the Pack's own segments.
///
/// A tool id is a [`GlobalName`] of two or more segments, such as
/// `web/fetch`. The first segment is the id of the Pack that contributed
/// the tool, and the last is the tool's own name. For example, `web/fetch`
/// comes from `web`.
///
/// The tool id stays its identity under any name a model request uses for
/// it. When a Pack is bound to a prompt, a selected tool can be offered
/// under a prompt-local alias. Calls under that alias still reach the same
/// tool.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub struct ToolId(GlobalName);

impl ToolId {
    /// Parses a tool id of two or more segments (`pack/name`).
    ///
    /// # Errors
    /// Returns an [`InvalidValue`](crate::error::ErrorCode::InvalidValue)
    /// error when the id has fewer than two segments, is empty or has an
    /// empty segment, a segment contains a control byte, or a segment
    /// contains any other character outside the set of lowercase ASCII
    /// letters, digits, `-`, `_`, and `.`.
    pub fn parse(id: &str) -> Result<ToolId, Error> {
        let invalid =
            |reason: &str| make_invalid_id_error(format!("invalid tool id: {reason}"), id);
        let name = GlobalName::validate(id).map_err(invalid)?;
        if name.segments().len() < 2 {
            return Err(invalid(
                "a tool id has two or more segments, its Pack's name first, such as `web/fetch`",
            ));
        }
        Ok(ToolId(name))
    }

    /// Returns the tool's name: the last segment, such as `fetch` in
    /// `web/fetch`.
    #[must_use]
    pub fn name(&self) -> &str {
        let segments = self.0.segments();
        &segments[segments.len() - 1]
    }

    /// Returns the id of the Pack that contributed this tool.
    ///
    /// It is the tool id's first segment, validated when the tool id was
    /// parsed. This method reuses that validation and builds the
    /// [`PackId`] directly.
    #[must_use]
    pub fn pack(&self) -> PackId {
        PackId::from_segment(self.0.first())
    }

    /// Returns the first segment, the contributing Pack's local name.
    ///
    /// Crate-internal: backs [`PackId::contains`] without building a
    /// [`PackId`].
    pub(crate) fn pack_segment(&self) -> &str {
        self.0.first()
    }
}

impl FromStr for ToolId {
    type Err = Error;

    fn from_str(id: &str) -> Result<ToolId, Error> {
        ToolId::parse(id)
    }
}

impl std::fmt::Display for ToolId {
    /// Formats the id in its canonical slash-separated form, such as
    /// `web/fetch`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl serde::Serialize for ToolId {
    /// Serializes the id as a single slash-separated string.
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for ToolId {
    /// Deserializes the id from its slash-separated string.
    ///
    /// The string is validated with the same rules as `ToolId::parse`. An
    /// invalid string is a deserialization error.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = <String as serde::Deserialize>::deserialize(deserializer)?;
        ToolId::parse(&text).map_err(serde::de::Error::custom)
    }
}
