//! The executor's import surface for `runvil-lua`, which implements the
//! sandboxed section VM, the Engine globals, and the coroutine protocol;
//! that crate documents them.
//!
//! This module is the one surface production code imports that crate
//! through: an item the Engine needs is re-exported here, and a production
//! module names `crate::lua` rather than `runvil_lua`.
//!
//! Three groups still name `runvil_lua` itself:
//!
//! - The error bridge, `error::convert` and `error::value`, which maps the
//!   Lua crate's error type variant for variant and implements its
//!   [`ErrorValue`](runvil_lua::ErrorValue) trait on this crate's error.
//! - Test-only code: the `#[cfg(test)]` shim install in `execute::section_vm`,
//!   which reaches `runvil_lua::test_support`, and this crate's test support,
//!   which takes `ToolOutputKind` from the crate root.
//! - The test suites, which build VM fixtures by hand.

pub(crate) use runvil_lua::{
    AdhocToolOutcome, Answer, Argv, Blocked, ChatOutcome, ChatResult, CoroStep, DiscoveryTool,
    Library, LibraryBlock, LibraryKey, LuaBlockResult, MessageList, ModelCall, ModelReport,
    NonEmpty, OfferMode, OfferedTool, OverflowReason, ProseState, QuotaResource, RenderedTools,
    Request, RosterEntry, ScriptReport, SectionText, SectionVm, SpawnVia, TaskDelivery, TaskPhase,
    TaskStatus, ToolBinding, ToolCallCounts, ToolCallOutcome, ToolCallRecord, ToolSet, Uninjected,
    UsageAnchor, VfsOp, VfsOutcome, YieldParse, current_catalog_packs, current_dynamic_set,
    current_static_set, exhaustion_detail, install_preludes, install_store_shims, install_ui,
    output_reserve, precheck, prepare_dispatch, prepare_model_dispatch, project_messages,
    render_item, resolve_model_binding, wire_name_of,
};

// The store operation behind `execute::perform_vfs_op`, the entry point
// a caller answers a `Vfs` effect through, and the
// model-facing message renderers a store failure carries.
pub(crate) use runvil_lua::{run_store_op, store_error_message, store_probe_message};

#[cfg(test)]
mod tests;
