//! The model vocabulary a caller exchanges with a run: what a `Chat` effect
//! includes ([`Message`], [`ToolSchema`], [`ModelBinding`]) and what its
//! answer returns ([`Completion`], [`CompletionResult`],
//! [`CompletionError`]), plus the prompt-local binding vocabulary
//! ([`ModelSet`], [`ModelView`], [`ModelInvocation`]) and the model
//! identity a run's roles are bound to ([`ModelInfo`], [`ModelId`]).
//!
//! This module is the executor's import surface for the private
//! `runvil-model-client` crate, which holds the round and binding
//! vocabulary, and for the model identity `runvil-types` defines. Production
//! code reaches both through here: an item the Engine needs is re-exported,
//! and a production module names `crate::model` rather than the dependency.
//!
//! Two groups still name the dependency crates themselves:
//!
//! - This crate's test support and its test suites, which also need names
//!   the production surface does not re-export, such as
//!   `CompletionOptions`, `RawExchange`, `Role`, and `ToolSchemaError`.
//! - The crate's bench, `benches/models_loop.rs`, a separate crate that
//!   cannot see a `pub(crate)` surface.
//!
//! Performing a round, its transport codec included, is the caller's job.
//! The Engine never performs a completion.

pub(crate) use runvil_model_client::client::{
    Completion, CompletionResult, Message, ToolCall, ToolSchema,
};
// The helpers behind the validating constructors and the round's reply
// checks, which the Engine calls where the rule each implements is
// documented.
pub(crate) use runvil_model_client::detail;
pub(crate) use runvil_model_client::model::{
    CompletionError, CompletionErrorKind, ModelBinding, ModelInvocation, ModelSet, ModelView,
    Temperature,
};
pub(crate) use runvil_types::models::{ModelId, ModelInfo, ThinkingMode};

#[cfg(test)]
mod tests;
