//! The run log's failure vocabulary.

use std::io;

use crate::RunId;

/// Why a run log operation failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LogError {
    /// The database refused an operation; the database's error is
    /// the source.
    #[error("run log database")]
    Database {
        /// The database's error.
        #[source]
        source: turso::Error,
    },
    /// The log file could not be addressed; `context` names the operation
    /// and the path, and the I/O error is the source.
    #[error("run log file: {context}")]
    Io {
        /// The operation and the path it addressed, such as
        /// `create /a/runs`.
        context: String,
        /// The I/O error.
        #[source]
        source: io::Error,
    },
    /// A payload could not be serialized on the way in or parsed on the
    /// way out; the serde error is the source.
    #[error("run log payload")]
    Payload {
        /// The serde error.
        #[source]
        source: serde_json::Error,
    },
    /// No run with this id was ever begun in this log.
    #[error("run log: unknown run {0}")]
    UnknownRun(RunId),
    /// The run has already ended; nothing more may be written to it.
    #[error("run log: run {0} has ended")]
    RunEnded(RunId),
    /// A stored row disagrees with the schema: expected the named shape,
    /// found the described value.
    #[error("run log: corrupt row: {0}")]
    Corrupt(String),
}

impl LogError {
    /// [`LogError::Io`] for `source`, with `context` naming the operation
    /// and the path. There is no `From<io::Error>`: a bare `?` would drop
    /// both.
    pub(crate) fn io(context: impl Into<String>, source: io::Error) -> Self {
        LogError::Io {
            context: context.into(),
            source,
        }
    }
}

impl From<turso::Error> for LogError {
    fn from(source: turso::Error) -> Self {
        LogError::Database { source }
    }
}

impl From<serde_json::Error> for LogError {
    fn from(source: serde_json::Error) -> Self {
        LogError::Payload { source }
    }
}
