//! Prompt-local model bindings: catalog, role bindings, and invocation.
//!
//! The caller supplies the
//! [`ModelCatalog`](runvil_types::models::ModelCatalog); building it happens
//! outside this crate. Building a run fills each of the five model roles
//! from the caller's role map into its [`ModelSet`], once, before any Lua
//! runs, and the run's first step checks the prompt's declared requirements
//! against the model that fills each role. `models.use` selects one binding for
//! a section, and a section that never calls it runs on the `default` role.
//! A model-facing section with no model fails with a model-binding failure
//! surfaced through the run error the caller receives.
mod error;
mod options;

pub use error::{CompletionError, CompletionErrorKind};
pub use options::{CompletionOptions, ModelBinding, ModelInvocation, ModelSet, Temperature};
// `runvil-lua` names the model id through this path; the rest of the catalog
// vocabulary is imported from `runvil-types` directly.
pub use runvil_types::models::ModelId;

#[cfg(test)]
mod tests;
