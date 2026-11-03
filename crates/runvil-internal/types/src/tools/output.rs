//! Tool output with mandatory trust, and the constructor of a tool call's
//! failure.

use crate::error::{Error, ErrorCode};

/// Whether a tool's output is trusted or must be treated as untrusted data.
///
/// Every [`ToolOutput`] carries a trust level, so it cannot be forgotten. An
/// [`OutputTrust::Untrusted`] result is nonce-wrapped before it can reach
/// model input. A run log records the level as `trusted` or `untrusted`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum OutputTrust {
    /// The output was produced by trusted, first-party code.
    Trusted,
    /// The output contains external data that an attacker could influence.
    Untrusted,
}

/// Holds the text a successful tool call returns, with its trust level.
///
/// Build one with [`ToolOutput::trusted`] or [`ToolOutput::untrusted`]. The
/// trust level travels with the text, so code that handles the output reads
/// the level from the output itself.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ToolOutput {
    text: String,
    trust: OutputTrust,
}

impl ToolOutput {
    /// Builds a trusted output whose text is added to model input verbatim.
    #[must_use]
    pub fn trusted(text: impl Into<String>) -> ToolOutput {
        ToolOutput {
            text: text.into(),
            trust: OutputTrust::Trusted,
        }
    }

    /// Builds an untrusted output that is nonce-wrapped before reaching a model.
    #[must_use]
    pub fn untrusted(text: impl Into<String>) -> ToolOutput {
        ToolOutput {
            text: text.into(),
            trust: OutputTrust::Untrusted,
        }
    }

    /// Borrows the output text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns whether the output is trusted or untrusted.
    #[must_use]
    pub fn trust(&self) -> OutputTrust {
        self.trust
    }
}

/// Builds a tool call's failure: `code` is one of the tool codes, such as
/// [`ToolInvalidArguments`](ErrorCode::ToolInvalidArguments), and `text` is
/// both the message and the prose the model reads, so it must be safe to
/// show the model. A cause goes in the message only, through
/// [`Error::with_cause`].
#[must_use]
pub fn tool_error(code: ErrorCode, text: impl Into<String>) -> Error {
    let text = text.into();
    Error::new(code, text.clone(), text)
}
