//! The executor's import surface for `runvil-lua`, which implements the
//! sandboxed section VM, the Engine globals, and the coroutine protocol;
//! that crate documents them.

pub(crate) use runvil_lua::{
    AdhocToolOutcome, Answer, Argv, Blocked, ChatOutcome, ChatResult, CoroStep, DiscoveryTool,
    Library, LibraryBlock, LibraryKey, LuaBlockResult, MessageList, ModelReport, NonEmpty,
    OfferMode, OfferedTool, OverflowReason, ProseState, QuotaResource, RenderedTools, Request,
    ScriptReport, SectionText, SectionVm, SpawnVia, TaskDelivery, TaskPhase, TaskStatus,
    ToolBinding, ToolCallCounts, ToolCallOutcome, ToolCallRecord, ToolSet, Uninjected, UsageAnchor,
    VfsOp, VfsOutcome, YieldParse, current_catalog_packs, current_dynamic_set,
    current_tool_bindings, exhaustion_detail, install_preludes, install_section_loop_shim,
    install_store_shims, install_ui, output_reserve, precheck, prepare_dispatch,
    prepare_model_dispatch, project_messages, render_item, resolve_model_binding, wire_name_of,
};

// The store operation behind `execute::perform_vfs_op`, the entry point
// a caller answers a `Vfs` effect through, and the
// model-facing message renderers a store failure carries.
pub(crate) use runvil_lua::{run_store_op, store_error_message, store_probe_message};

#[cfg(test)]
mod tests;
