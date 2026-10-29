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
//! The round and binding vocabulary comes from the private
//! `runvil-model-client` crate, and [`ModelId`], [`ModelInfo`], and
//! [`ThinkingMode`] from `runvil-types`; both are re-exported here. The
//! transport codec a round's performer runs (the request body builder,
//! the read loop over a transport's chunk source, and the error type it
//! builds a [`CompletionError`] from) is not: the facade publishes it from
//! `runvil-model-client` directly. The Engine itself never performs a
//! completion.

pub(crate) use runvil_model_client::client::{
    Completion, CompletionResult, Message, ToolCall, ToolSchema,
};
pub(crate) use runvil_model_client::model::{
    CompletionError, CompletionErrorKind, ModelBinding, ModelInvocation, ModelSet, ModelView,
    Temperature,
};
pub(crate) use runvil_types::models::{ModelId, ModelInfo, ThinkingMode};

#[cfg(test)]
mod tests;
