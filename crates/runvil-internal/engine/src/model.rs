//! The model vocabulary a caller exchanges with a run: what a `Chat` effect
//! includes ([`Message`], [`ToolSchema`], [`ModelBinding`]) and what its
//! answer returns ([`Completion`], [`CompletionResult`],
//! [`CompletionError`]), plus the prompt-local binding vocabulary
//! ([`ModelSet`], [`ModelView`], [`ModelInvocation`]) and the model
//! identity a run's roles are bound to ([`ModelInfo`], [`ModelId`]).
//!
//! `models.default` sets the prompt-wide default role, conventionally
//! from the H1 body, and `models.use` sets a section entry's current
//! selection, which a later call replaces. A `models.infer` or
//! `models.loop` round with neither fails with a model-binding failure
//! surfaced through [`crate::RunError`].
//!
//! This module is the executor's import surface for the private
//! `runvil-model-client` crate, which holds the round and binding
//! vocabulary, and for the model identity `runvil-types` defines. Production
//! code reaches both through here: an item the Engine needs is re-exported,
//! and a production module names `crate::model` rather than the dependency.
//!
//! Three groups still name the dependency crates themselves:
//!
//! - This crate's test support, which builds scripted replies from
//!   `CompletionOptions` and the constructors a script names, none of which
//!   the production surface re-exports.
//! - The test suites, which build invocations and completions by hand for
//!   the same reason.
//! - The crate's bench, `benches/models_loop.rs`, a separate crate that
//!   cannot see a `pub(crate)` surface.
//!
//! The transport codec a round's performer runs - the request body builder,
//! the read loop over a transport's chunk source, and the error type it
//! builds a [`CompletionError`] from - lives in the Harness gateway client
//! crate, which is where a caller that performs a round takes it from. The
//! Engine never performs a completion.

pub(crate) use runvil_model_client::client::{
    Completion, CompletionResult, Message, ToolCall, ToolSchema,
};
// The helpers behind the validating constructors and the round's reply
// checks, which the Engine calls where the rule each implements is
// documented.
pub(crate) use runvil_model_client::detail;
pub(crate) use runvil_model_client::model::{
    CompletionError, CompletionErrorKind, ModelBinding, ModelInvocation, ModelSet, ModelSetError,
    ModelView, Temperature,
};
pub(crate) use runvil_types::models::{ModelId, ModelInfo, ThinkingMode};

#[cfg(test)]
mod tests;
