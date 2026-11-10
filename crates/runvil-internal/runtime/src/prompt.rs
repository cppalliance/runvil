//! The executor's import surface for `runvil-prompt`, which loads a prompt
//! file into a ready-to-run [`Prompt`] tree; that crate documents the prompt
//! format.
//!
//! This module is the one surface production code imports that crate
//! through: an item or helper the Engine needs is re-exported here, and a
//! production module names `crate::prompt` rather than `runvil_prompt`. The
//! `detail` module is re-exported whole, since its helpers are the loaded
//! crate's own algorithms, called where the rule each one implements is
//! documented.
//!
//! Two groups still name `runvil_prompt` itself:
//!
//! - The test suites, which import the prompt fixtures under
//!   `runvil_prompt::test_support` and call a `detail` helper directly.
//! - The crate's bench, `benches/models_loop.rs`, a separate crate that
//!   cannot see a `pub(crate)` surface.

pub(crate) use runvil_prompt::detail;
pub(crate) use runvil_prompt::{Block, BodyText, Prompt, RoleRequirements, Section};
