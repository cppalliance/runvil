//! The coroutine protocol: validated request and answer types for the
//! yield/resume boundary between section Lua and the scheduler driver.
//!
//! Every suspending Engine call, one per [`Request`] variant, is a Lua-side
//! shim that yields a request table; the driver validates the yield into a
//! [`Request`], dispatches it, and resumes the coroutine with the
//! `(ok, result)` envelope rendered from an [`Answer`]. The two enums are
//! the audit surface: what a script can cause the Engine to do is one
//! short read, and each variant's fields are the compiler-checked
//! per-message contract.
//!
//! The submodules split the protocol along that boundary: `request` the
//! request vocabulary (the [`Request`] and [`VfsOp`] enums), `record` the
//! message-record types a chat request is built from and their
//! validation, `answer` the answer vocabulary (the [`Answer`] enum and its
//! payload types), `parse` the yield-to-request validation, and `render`
//! the answer-to-envelope rendering. `non_empty` holds the [`NonEmpty`]
//! sequence two of them use.

mod answer;
mod non_empty;
mod parse;
mod record;
mod render;
mod request;
#[cfg(test)]
mod tests;

pub use answer::{
    Answer, Blocked, ChatOutcome, ChatResult, TaskDelivery, TaskPhase, TaskStatus, ToolCallOutcome,
    VfsOutcome,
};
pub use non_empty::NonEmpty;
pub use parse::YieldParse;
pub(crate) use record::parse_record;
pub use record::{ContentPart, MessageContent, MessageRecord, RecordRole, ToolCallRecord};
pub(crate) use render::{Retained, render_envelope};
pub use request::{AdhocToolOutcome, Request, SpawnVia, VfsOp};
