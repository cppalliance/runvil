//! Tool bindings, the shared tool set, and the per-binding output kind that
//! shape how bound tools reach Lua, and the section text a prompt read sees.

use std::borrow::Cow;
use std::collections::BTreeMap;

use mlua::Value;
use runvil_types::packs::PackId;
use runvil_types::tools::{ToolCatalog, ToolId, ToolInfo};
use serde_json::Value as Json;

use crate::scope::DiscoveryTool;

/// How a bound tool's output resumes into Lua at the `tools.call` boundary.
///
/// Declared on the binding (from the tool info's `structured_output`), not
/// the tool implementation, so the caller's tool catalog decides how scripts
/// receive the output. The model tool loop never consults the kind (its
/// results are always added to the conversation as text).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ToolOutputKind {
    /// The output text resumes as a Lua string.
    #[default]
    Plain,
    /// The output text is JSON, parsed at dispatch and resumed as a Lua
    /// table through the serde boundary; invalid JSON is the tool's error.
    Structured,
}

/// One tool bound under the name its caller reaches it by, holding the
/// tool's data - its schema, description, output kind, whether it lists
/// tools, and whether a stop leaves its calls running - and never its
/// implementation.
///
/// Run-time execution (schema preparation, script dispatch) reads the
/// binding alone; a call is issued as an effect naming the identity, and
/// performing it is the Harness's job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolBinding {
    /// The name the call reaches the tool by: its wire name in a section's
    /// frozen sets, or the full id of a catalog tool a script calls.
    alias: String,
    /// The tool's own catalog description.
    description: String,
    /// The selected stable live identity.
    id: ToolId,
    /// The JSON-Schema `object` the tool's arguments must match, advertised
    /// under the alias.
    schema: Json,
    /// How a script-initiated `tools.call` resumes this binding's output;
    /// the model tool loop ignores it.
    output_kind: ToolOutputKind,
    /// Whether the tool's output lists tools, which a script's call
    /// receives as plain text.
    describes_tools: bool,
    /// Whether a stop leaves a call to the tool in flight.
    survives_stop: bool,
}

impl ToolBinding {
    /// Binds `alias` to the tool `info` describes: the description is the
    /// tool's own, the output kind follows the tool info's
    /// structured-output flag, and the listing and stop flags are the tool
    /// info's.
    #[must_use]
    pub fn from_info(alias: &str, info: &ToolInfo) -> Self {
        Self {
            alias: alias.to_owned(),
            description: info.description.clone(),
            id: info.id.clone(),
            schema: info.parameters_schema.clone(),
            output_kind: if info.structured_output {
                ToolOutputKind::Structured
            } else {
                ToolOutputKind::Plain
            },
            describes_tools: info.describes_tools,
            survives_stop: info.survives_stop,
        }
    }

    /// Returns the name the call reaches the tool by.
    #[must_use]
    pub fn alias(&self) -> &str {
        &self.alias
    }

    /// Returns the tool's catalog description.
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Returns the selected stable live identity.
    #[must_use]
    pub fn id(&self) -> &ToolId {
        &self.id
    }

    /// Returns the JSON-Schema `object` the tool's arguments must match.
    #[must_use]
    pub fn schema(&self) -> &Json {
        &self.schema
    }

    /// Returns how a script-initiated `tools.call` resumes this binding's
    /// output.
    #[must_use]
    pub fn output_kind(&self) -> ToolOutputKind {
        self.output_kind
    }

    /// Returns whether the tool's output lists tools.
    #[must_use]
    pub fn describes_tools(&self) -> bool {
        self.describes_tools
    }

    /// Returns whether a stop leaves a call to the tool in flight.
    #[must_use]
    pub fn survives_stop(&self) -> bool {
        self.survives_stop
    }
}

/// The name the model calls the tool `info` describes by: its `wire_name`
/// when set, else its id with `/` and `.` turned into `_` (`web/fetch`
/// gives `web_fetch`).
#[must_use]
pub fn wire_name_of(info: &ToolInfo) -> Cow<'_, str> {
    match &info.wire_name {
        Some(wire_name) => Cow::Borrowed(wire_name),
        None => Cow::Owned(info.id.to_string().replace(['/', '.'], "_")),
    }
}

/// Outcome of a Lua block that may invoke `jump`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LuaBlockResult {
    /// Normal completion with an optional scalar return.
    Returned(Option<String>),
    /// `jump` transferred control to this heading (`## Name`).
    Jump(String),
}

/// Resolves a `call` / `jump` target from a heading string.
///
/// # Errors
/// Returns a Lua error when the value is not a string.
pub(crate) fn resolve_section_target(value: Value) -> mlua::Result<String> {
    match value {
        Value::String(s) => Ok(s.to_str()?.to_owned()),
        other => Err(mlua::Error::external(format!(
            "section target must be a string, got {}",
            other.type_name()
        ))),
    }
}

/// What a prompt read sees of one visible section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionText {
    /// The section's own raw Markdown.
    pub markdown: String,
    /// Its pre-parsed list items; empty for a section that isn't a list.
    pub items: Vec<String>,
}

/// Which library a `require` resolved to, opaque to this crate.
///
/// The caller encodes a library section's position in the identity
/// string; `require`'s cache keys on that identity, never on the heading
/// text, so one heading resolved from two positions names two libraries.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LibraryKey(String);

impl LibraryKey {
    /// Builds a key whose stable identity is `identity`.
    #[must_use]
    pub fn new(identity: String) -> Self {
        Self(identity)
    }

    /// Returns the identity the key was built from.
    #[must_use]
    pub fn identity(&self) -> &str {
        &self.0
    }
}

/// What `require` loads from one library section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Library {
    /// Which library this is: the cache key, and the position a `require`
    /// made while it loads resolves from.
    pub key: LibraryKey,
    /// The library section's heading with its markers, such as
    /// `## helpers`, which its load reports and a cycle error name.
    pub heading: String,
    /// The section's blocks in order.
    pub blocks: Vec<LibraryBlock>,
}

/// One block of a library section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryBlock {
    /// The block's compiled Lua.
    pub program: crate::LuaProgram,
    /// The block's prose template, the Markdown before it.
    pub prose: String,
}

/// How one roster Pack stands in its run, which the caller decides.
///
/// The two variants carry what the run needs of the Pack and nothing else,
/// so a Pack is either usable with a vendor or unavailable with a reason,
/// never both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RosterEntry {
    /// The Pack can serve the run: the vendor the caller picked it from.
    Usable {
        /// The vendor, one segment, such as `runvil` or `acme`.
        vendor: String,
    },
    /// The Pack cannot serve the run: the reason, written for the model
    /// that reads the refusal.
    Unavailable {
        /// Why it cannot serve.
        reason: String,
    },
}

impl RosterEntry {
    /// Returns the vendor when the Pack can serve the run.
    #[must_use]
    pub(crate) fn vendor(&self) -> Option<&str> {
        match self {
            RosterEntry::Usable { vendor } => Some(vendor),
            RosterEntry::Unavailable { .. } => None,
        }
    }
}

/// The run's tool set: the declared Packs, the run's roster, and the run
/// catalog. Nothing changes it during a run, so the run and every section
/// VM share one `Arc<ToolSet>`. Nothing in it is bound: each section entry
/// binds the tools it offers when it freezes its sets.
#[derive(Debug, Clone, Default)]
pub struct ToolSet {
    /// The Packs the frontmatter declares, in declaration order.
    declared: Vec<PackId>,
    /// Every Pack the caller reports the run holds, with or without tools:
    /// one entry per Pack, holding its vendor when it can serve the run and
    /// the reason, written for the model, when it can't.
    roster: BTreeMap<PackId, RosterEntry>,
    /// The run catalog: every catalog tool's `ToolInfo` by id, the handle
    /// the caller's context holds, which is what Lua sees.
    catalog: ToolCatalog,
}

impl ToolSet {
    /// Builds the run's tool set from the declared Packs in declaration
    /// order, the roster, and the run catalog.
    #[must_use]
    pub fn new(
        declared: Vec<PackId>,
        roster: BTreeMap<PackId, RosterEntry>,
        catalog: ToolCatalog,
    ) -> Self {
        Self {
            declared,
            roster,
            catalog,
        }
    }

    /// Returns the declared Packs in declaration order.
    #[must_use]
    pub fn declared(&self) -> &[PackId] {
        &self.declared
    }

    /// Returns every installed Pack the caller reports, each with its
    /// vendor when it can serve the run, or the reason, written for the
    /// model, when it cannot. Tests read it through `test_support`.
    #[must_use]
    pub(crate) fn roster(&self) -> &BTreeMap<PackId, RosterEntry> {
        &self.roster
    }

    /// Returns the run catalog.
    #[must_use]
    pub fn catalog(&self) -> &ToolCatalog {
        &self.catalog
    }

    /// Returns the vendor the caller reported for the Pack the tool `id`
    /// belongs to, `runvil` for the Engine's discovery tools, or `None`
    /// when the caller reported none, as for an adhoc tool or a Pack the
    /// roster lists as unable to serve.
    #[must_use]
    pub fn vendor_for(&self, id: &ToolId) -> Option<&str> {
        if DiscoveryTool::of(id).is_some() {
            return Some(DiscoveryTool::VENDOR);
        }
        self.roster.get(&id.pack()).and_then(RosterEntry::vendor)
    }
}

#[cfg(test)]
#[path = "handles-tests.rs"]
mod tests;
