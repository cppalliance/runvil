//! Prompt-local model bindings: catalog, role bindings, and invocation.
//!
//! The caller supplies the
//! [`ModelCatalog`](runvil_types::models::ModelCatalog); building it happens
//! outside this crate. Prepare fills each of the five model roles from the
//! caller's role map, checking the prompt's declared requirements against the
//! model that fills the role, and the run builds its [`ModelSet`] from those
//! bindings once, before any Lua runs. `models.use` selects one binding for
//! a section, and a section that never calls it runs on the `default` role.
//! A model-facing section with no model fails with a model-binding failure
//! surfaced through the run error the caller receives.
mod error;
mod options;

pub use error::{CompletionError, CompletionErrorKind};
pub use options::{
    CompletionOptions, ModelBinding, ModelInvocation, ModelSet, Temperature, TemperatureError,
};
// `runvil-lua` names the model id through this path; the rest of the catalog
// vocabulary is imported from `runvil-types` directly.
pub use runvil_types::models::ModelId;

#[cfg(test)]
mod tests;
