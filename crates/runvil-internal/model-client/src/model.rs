//! Prompt-local model bindings: catalog, bind/use declarations, and invocation.
//!
//! The caller supplies the
//! [`ModelCatalog`](runvil_types::models::ModelCatalog); building it happens
//! outside this crate. The prompt's frontmatter declares each model role, prepare
//! resolves every role against that catalog under the role's hard keywords
//! and context minimum, and the run builds its binding set from those
//! bindings before any Lua runs. H2 `models.use` selects at most one binding
//! per section; `models.default` supplies the prompt-wide default,
//! conventionally from H1, for sections that omit `models.use`. Model-facing
//! sections with neither binding fail with a
//! model-binding failure surfaced through the run error the caller receives.

mod error;
mod options;

pub use error::{CompletionError, CompletionErrorKind};
pub use options::{
    CompletionOptions, ModelBinding, ModelInvocation, ModelSet, ModelSetError, ModelView,
    Temperature, TemperatureError,
};
// `runvil-lua` names the model id through this path; the rest of the catalog
// vocabulary is imported from `runvil-types` directly.
pub use runvil_types::models::ModelId;

#[cfg(test)]
mod tests;
