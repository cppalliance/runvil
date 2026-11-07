//! Tool bindings, the per-binding output kind, the run's roster entries
//! and shared tool set, the Engine's discovery tools, and the wire name a
//! model calls a tool by.

use std::borrow::Cow;
use std::collections::BTreeMap;

use runvil_types::packs::PackId;
use runvil_types::tools::{ToolCatalog, ToolId, ToolInfo};
use serde_json::Value as Json;

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
/// tools, whether only a model may call it, and whether a stop leaves its
/// calls running - and never its implementation.
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
    /// The tool's id.
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
    /// Whether only a model may call the tool, so a script's `tools.call`
    /// refuses it.
    model_only_call: bool,
    /// Whether a stop leaves a call to the tool in flight.
    survives_stop: bool,
}

impl ToolBinding {
    /// Binds `alias` to the tool `info` describes: the description is the
    /// tool's own, the output kind follows the tool info's
    /// structured-output flag, and the listing, model-only, and stop flags
    /// are the tool info's.
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
            model_only_call: info.model_only_call,
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

    /// Returns the tool's id.
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

    /// Returns whether only a model may call the tool, so a script's
    /// `tools.call` refuses it.
    #[must_use]
    pub fn model_only_call(&self) -> bool {
        self.model_only_call
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

/// One of the Engine's two discovery tools, which join the end of a
/// section entry's Static Set when its Dynamic Set holds a tool: the model
/// finds Dynamic Set tools with the first and calls them with the second.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryTool {
    /// `dynamic/search`, which the model calls as `GetDynamicTools`.
    Search,
    /// `dynamic/call`, which the model calls as `CallDynamicTool`.
    Call,
}

impl DiscoveryTool {
    /// Both discovery tools, in the order they join the Static Set.
    pub const ALL: [DiscoveryTool; 2] = [DiscoveryTool::Search, DiscoveryTool::Call];

    /// The vendor both discovery tools report.
    pub const VENDOR: &'static str = "runvil";

    /// The tool's id, under the reserved Pack id `dynamic`.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            DiscoveryTool::Search => "dynamic/search",
            DiscoveryTool::Call => "dynamic/call",
        }
    }

    /// The wire name the model calls the tool by.
    #[must_use]
    pub fn wire_name(self) -> &'static str {
        match self {
            DiscoveryTool::Search => "GetDynamicTools",
            DiscoveryTool::Call => "CallDynamicTool",
        }
    }

    /// The discovery tool whose id is `id`, if it is one.
    #[must_use]
    pub fn of(id: &ToolId) -> Option<DiscoveryTool> {
        let id = id.to_string();
        DiscoveryTool::ALL.into_iter().find(|tool| tool.id() == id)
    }
}

#[cfg(test)]
mod tests {
    //! Tests for the tool binding built from a catalog tool info: the
    //! tool info's data is copied verbatim and its structured-output flag
    //! selects the binding's output kind. A catalog tool's wire name is its
    //! own when set and its default otherwise, and the tool set reads the run
    //! catalog and derives each tool's vendor from its Pack, `runvil` for the
    //! Engine's discovery tools. The discovery tools' ids stay under a
    //! reserved Pack id, so no vendor Pack can take the discovery namespace.

    use std::collections::BTreeMap;

    use runvil_types::packs::PackId;
    use runvil_types::tools::{ToolCatalog, ToolId, ToolInfo};
    use serde_json::json;

    use super::{DiscoveryTool, RosterEntry, ToolBinding, ToolOutputKind, ToolSet, wire_name_of};

    /// A tool info for `tools/fetch`.
    fn tool_info(structured: bool) -> ToolInfo {
        ToolInfo::new(
            ToolId::parse("tools/fetch").expect("the id is valid"),
            "Fetch a page",
            json!({"type": "object", "properties": {"url": {"type": "string"}}}),
        )
        .structured(structured)
    }

    #[test]
    fn a_structured_tool_info_binds_with_structured_output() {
        let info = tool_info(true);
        let binding = ToolBinding::from_info("fetch_alias", &info);
        assert_eq!(binding.output_kind(), ToolOutputKind::Structured);
        assert_eq!(binding.alias(), "fetch_alias");
        assert_eq!(binding.id(), &info.id);
        assert_eq!(binding.description(), "Fetch a page");
        assert_eq!(binding.schema(), &info.parameters_schema);
    }

    #[test]
    fn a_plain_tool_info_binds_with_plain_output() {
        let binding = ToolBinding::from_info("fetch_alias", &tool_info(false));
        assert_eq!(binding.output_kind(), ToolOutputKind::Plain);
    }

    #[test]
    fn a_new_tool_info_binds_callable_by_a_script() {
        assert!(
            !ToolBinding::from_info("fetch_alias", &tool_info(false)).model_only_call(),
            "a tool info the Pack did not mark stays callable from a script"
        );
    }

    #[test]
    fn a_model_only_tool_info_binds_with_the_flag_set() {
        let info = tool_info(false).model_only_call(true);
        let binding = ToolBinding::from_info("fetch_alias", &info);
        assert!(
            binding.model_only_call(),
            "the binding carries the catalog's refusal"
        );
    }

    #[test]
    fn the_test_seam_overrides_only_the_description() {
        let info = tool_info(true);
        let binding = ToolBinding::for_test("fetch_alias", "test double", &info);
        assert_eq!(binding.description(), "test double");
        assert_eq!(
            binding.output_kind(),
            ToolOutputKind::Structured,
            "the output kind still follows the tool info"
        );
    }

    #[test]
    fn a_catalog_tools_wire_name_is_its_own_when_set_and_its_default_otherwise() {
        let info = tool_info(false);
        assert_eq!(wire_name_of(&info), "tools_fetch");
        assert_eq!(wire_name_of(&info.wire_name("fetch")), "fetch");
        let dotted = ToolInfo::new(
            ToolId::parse("gh/issues.list").expect("the id is valid"),
            "List issues",
            json!({ "type": "object" }),
        );
        assert_eq!(
            wire_name_of(&dotted),
            "gh_issues_list",
            "both `/` and `.` turn into `_`"
        );
    }

    #[test]
    fn the_tool_set_reads_the_catalog_and_takes_each_tools_vendor_from_its_pack() {
        let pack = |name: &str| PackId::parse(name).expect("the Pack id is valid");
        let id = |id: &str| ToolId::parse(id).expect("the id is valid");
        let set = ToolSet::new(
            Vec::new(),
            BTreeMap::from([(
                pack("tools"),
                RosterEntry::Usable {
                    vendor: "acme".to_owned(),
                },
            )]),
            ToolCatalog::new([tool_info(false)]).expect("one tool"),
        );
        assert_eq!(
            set.catalog()
                .get(&id("tools/fetch"))
                .map(|info| info.description.as_str()),
            Some("Fetch a page")
        );
        assert_eq!(set.vendor_for(&id("tools/fetch")), Some("acme"));
        assert_eq!(
            set.vendor_for(&id("tools/rendered_only")),
            Some("acme"),
            "a tool outside the catalog reports its Pack's vendor"
        );
        assert_eq!(
            set.vendor_for(&id("adhoc/grab")),
            None,
            "an adhoc tool reports none"
        );
        assert_eq!(
            set.vendor_for(&id("web/fetch")),
            None,
            "a Pack with no vendor reports none"
        );
        for discovery in ["dynamic/search", "dynamic/call"] {
            assert_eq!(
                set.vendor_for(&id(discovery)),
                Some("runvil"),
                "the Engine's discovery tool {discovery} reports runvil"
            );
        }
    }

    #[test]
    fn every_discovery_tool_id_sits_under_a_reserved_pack() {
        for tool in DiscoveryTool::ALL {
            let id = ToolId::parse(tool.id()).expect("a discovery tool id is a valid tool id");
            assert!(
                id.pack().is_reserved(),
                "{} is not under a reserved Pack id",
                tool.id()
            );
        }
    }
}
