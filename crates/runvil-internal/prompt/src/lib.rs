//! Loads a prompt file: its frontmatter, its Markdown structure, and its Lua.
//!
//! A prompt is one markdown file: YAML frontmatter, a required H1, optional H1
//! blocks, then H2 sections.
//! H1 and section content are alternating sequences of exact `lua` fences and
//! prose ([`Block`]). Sections nest recursively (H3 under H2, H4 under H3, and
//! so on through H6). A section with a prologue, prose, and an epilog is
//! exactly `[Lua, Prose, Lua]`.
//!
//! Prose capture follows the pending-Markdown model: Markdown accumulates
//! after each heading or ordinary `lua` fence, and each prose block is the
//! pending buffer the following `lua` fence consumes. A `---` thematic break
//! resets the pending buffer without becoming part of the prose; it has
//! no control-flow meaning, so content below a break parses and runs
//! normally. Markdown left after the final `lua` fence is inert trailing
//! commentary, never an error.
//!
//! Every `lua` fence in the H1 body and the sections compiles at load time, so
//! a loaded [`Prompt`] is ready to run: a syntax error surfaces here, never
//! partway through a run.
//!
//! ## Invariants
//!
//! - May depend on: `runvil-types` and `runvil-lua`.
//!   `cargo test -p build-xtask` enforces the product and container
//!   boundaries.

mod body_text;
mod contract;
pub mod detail;
mod error;
mod fence;
mod finder;
mod frontmatter;
mod parse;
mod position;
mod prompt;
mod sections;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub use body_text::BodyText;
pub use contract::{ArgDecl, ArgType, ArgsDecl, ArgsDeclIter, RoleRequirements};
use error::Result;
pub use finder::PromptFinder;
pub use frontmatter::{FileDecl, Frontmatter, MAX_TOOL_ITERATIONS};
pub use prompt::{Block, Prompt, Section};

#[cfg(test)]
mod tests;
