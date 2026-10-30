//! The gateway progress wire type, as Workshop reads it: one live activity,
//! a busy flag and a producer-owned text.
//!
//! [`Progress`] is the shape of one `GET /admin/progress` event and of the
//! progress field in `GET /admin/status`. Workshop only ever deserializes
//! it; the Gateway owns the producing side and the anti-flicker policy
//! lives in [`crate::progress`].
//!
//! The field names and types are the contract. [`tests`] pins them against
//! a golden JSON document, so a rename or a type change on either side
//! fails here rather than in a status bar that silently stops updating.
//!
//! The text is user-visible, so a producer must never place a bearer key,
//! API key, or other credential in it.

use serde::Deserialize;

/// One snapshot of the Gateway's live activity.
///
/// Deserialization tolerates fields this type does not know, which is what
/// lets a newer Gateway add one without breaking an older Workshop. There
/// is no schema version.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Progress {
    /// Whether any activity is live. The UI shows an indeterminate
    /// barberpole while this is set.
    pub busy: bool,
    /// The newest live activity's text, e.g. `"Downloading qwen3-8b.gguf
    /// 45%"`; empty when idle.
    pub text: String,
}

#[cfg(test)]
#[path = "progress-wire-tests.rs"]
mod tests;
