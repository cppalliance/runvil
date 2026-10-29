//! The Runvil model vocabulary: what a model round exchanges, and how a
//! prompt binds models. No transport.
//!
//! [`client`] holds the chat-completions protocol vocabulary: the wire
//! types that go out of the Engine in a `Chat` effect and come back in its
//! answer ([`client::Message`], [`client::ToolSchema`],
//! [`client::Completion`]), with the validating constructors that run the
//! neutral reply checks on every completion. [`model`] holds the catalog
//! and prompt-local binding vocabulary: the
//! [`ModelCatalog`](runvil_types::models::ModelCatalog) the
//! caller supplies, the validated [`ModelId`](runvil_types::models::ModelId)
//! identity, and the
//! [`model::ModelBinding`]/[`model::ModelSet`]/[`model::ModelView`] types
//! model selections resolve and freeze through, with
//! [`model::CompletionError`] as the failure a round reports.
//!
//! The metrics vocabulary in [`runvil_types::metrics`] (`Usage`,
//! `LlamaTimings`, `VllmMetrics`, `ClientTiming`, `CallMetrics`) is
//! canonical in `runvil-types`, and this crate uses it from there:
//! [`client::Completion`] holds a round's metrics. The
//! model identity/catalog vocabulary
//! ([`ModelId`](runvil_types::models::ModelId),
//! [`ModelCatalog`](runvil_types::models::ModelCatalog),
//! [`ModelInfo`](runvil_types::models::ModelInfo),
//! [`ThinkingMode`](runvil_types::models::ThinkingMode)) is canonical there
//! too, and `runvil-lua` reaches only the id through the `model` path.
//!
//! This crate contains no HTTP, no wire parsing, no prompt parser, no Lua
//! runtime, and no executor.
//!
//! ## Invariants
//!
//! - May depend on: `runvil-types`. `cargo test -p build-xtask`
//!   enforces the product and container boundaries.

pub mod client;
pub mod detail;
pub mod model;
mod normalize;
#[cfg(test)]
mod test_support;

/// Crate-internal result alias over the failure a model round reports.
type Result<T> = std::result::Result<T, model::CompletionError>;
