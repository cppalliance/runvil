//! The run-scoped tool set and the `argv` derivation: the pieces
//! `ChainState::root` assembles once per run.
//!
//! The tool set is built once. [`derive_argv`] is not: `ChainState::root`
//! calls it for the run's own args, and every args fork calls it again, so
//! a contained chain parses the input it was given.

use std::collections::BTreeMap;

use runvil_types::packs::PackId;
use runvil_types::tools::ToolCatalog;

use crate::lua::{RosterEntry, ToolSet};
use crate::parser::Prompt;

/// Builds the run's shared tool set: the frontmatter's declared Packs,
/// and the run catalog and roster, all moved out of the config. Nothing
/// is bound here: each section entry binds the tools it offers when it
/// freezes its sets.
pub(super) fn run_tool_set(
    prompt: &Prompt,
    tools: ToolCatalog,
    packs: BTreeMap<PackId, RosterEntry>,
) -> ToolSet {
    ToolSet::new(
        prompt
            .frontmatter()
            .uses()
            .iter()
            .map(|entry| entry.id().clone())
            .collect(),
        packs,
        tools,
    )
}

/// Derives a section's `argv` from the args string under the prompt's
/// declaration: a default-declared prompt wraps the interface prose into
/// the default shape (`argv.prose`, with the empty string present, not
/// absent); a structured declaration parses the string as JSON, and a parse
/// failure or a JSON `null` reads as nil (`if argv then` is the malformed
/// check). The executor never hard-errors on shape.
pub(super) fn derive_argv(prompt: &Prompt, args: &str) -> Option<serde_json::Value> {
    if prompt.frontmatter().args().is_default() {
        return Some(serde_json::json!({ "prose": args }));
    }
    match serde_json::from_str(args) {
        Ok(serde_json::Value::Null) | Err(_) => None,
        Ok(value) => Some(value),
    }
}
