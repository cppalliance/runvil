//! Sandboxed Lua execution for a section's Lua block.
//!
//! A section's Lua chunk runs in a fresh, restricted `mlua` VM: only the
//! `string`, `table`, `math`, and `utf8` standard libraries plus the safe base
//! functions are available; the raw input `args` string and the runtime `sys`
//! table are exposed; a writable `var` table is provided for the block to
//! populate; an always-on `store` table gives the block the run's virtual
//! files; and an every-Nth-instruction hook polls the run's cancel flag, so
//! even an unbounded loop aborts promptly once the run is canceled.
//! Direct `print` and `warn` are unavailable. A persistent `log(message)`
//! callback accepts one bounded, single-line UTF-8 string and reports it
//! through the run's emitter as an `Event::Lua` checkpoint. A persistent
//! `json` table decodes JSON text into Lua values (`json.decode` and
//! `json.decode_lines`), encodes Lua values as compact JSON text
//! (`json.encode` and `json.encode_lines`), holds `json.null`, the value
//! every decoded JSON null becomes, and marks a table to encode as a JSON
//! array (`json.array`). A persistent `markdown` table parses Markdown
//! text into plain tables (`markdown.parse`), finds a section or a block
//! in them (`markdown.section`, `markdown.section_or`, and
//! `markdown.select`), and writes them back as text (`markdown.render`).
//!
//! The chunk's top-level return value becomes the section's result (the finish
//! case of the exit rule). The `var` table is read back afterward as JSON for
//! prose substitution.
//!
//! The `store` table is a deterministic Engine global (like `var`), always
//! present and independent of tool scoping. Its methods are backed by the
//! store view derived from the run's VFS access capability, threaded in
//! from the executor, so every section
//! in a run shares one set of virtual files even though contexts clear on each
//! transition. A failed store op raises a structured error value of kind
//! `store` carrying its `reason` and fields, which surfaces from the
//! block's coroutine (`SectionVm::start_block_coro` and its resumes) as
//! that store failure, never as a Lua runtime error.
//!
//! The VM also holds `tasks`, the shims for spawning,
//! waiting on, checking, noting, and canceling tasks, for reading the
//! calling chain's inbox, and for sending to a task's. Each Pack the
//! prompt declares can contribute a prelude, Lua that defines globals once
//! per VM before `require` installs ([`install_preludes`] states the
//! rules).
//!
//! The `tools` table names every tool by its id: `tools.get` reads a tool
//! object, `tools.packs` holds one object per Pack the run knows, the
//! adhoc Pack included, whose `tools` lists that Pack's tool objects, and
//! `tools.adhoc` defines a Lua function as a tool of the adhoc Pack.
//! `tools.offer` and `tools.offer_dynamic` remember tools and Packs, by
//! id or object, for the section's Static Set and Dynamic Set, which the
//! section's first loop round freezes; after that, every offer raises.
//! `models.infer` freezes nothing.
//!
//! `messages` is the section's one conversation: a list the VM creates
//! empty at each section entry and serves as a global that no assignment
//! can rebind. `models.loop` runs over it, and a compactor rewrites it in
//! place.
//!
//! Most of this crate's public items exist for `runvil-runtime`'s
//! executor, which drives the VM and the coroutine protocol; the facade
//! re-exports the store protocol ([`VfsOp`], [`VfsOutcome`],
//! [`make_store_error`]) and the roster entry ([`RosterEntry`]);
//! `crates/runvil-engine/public-api.txt` holds the full surface. The
//! `test_support` module, built for this crate's tests and under the
//! `test-support` feature, holds what tests reach beyond that surface.
//!
//! ## Invariants
//!
//! - May depend on: `runvil-types`, `runvil-model-client`,
//!   `runvil-markdown`, and `runvil-vfs`. `cargo test -p build-xtask`
//!   enforces the product and container boundaries.
//! - Scheduler state changes only through a typed `Request` yielded by the
//!   installed shim; a direct or malformed yield fails the block without
//!   changing scheduler state.
//! - Every Engine-call yield goes through the recording `yield` that a
//!   VM's shim chunks share, which first sets the VM's call line to the
//!   author's line, or clears it when no author frame runs.
//! - Every universal error that crosses into a VM's Lua as an error table
//!   is placed through `authored_at`, except an internal one, which keeps
//!   its Rust location. The table's `message`, which `tostring` returns,
//!   is the error's prose after its `file:line` when it is placed on a
//!   prompt line; the `file:line` goes into that `message` only, never
//!   into the prose.

mod adhoc;
mod argv;
mod bindings;
mod chunk_line;
mod collection;
mod compactors;
mod coro;
mod dispatch;
mod engine_globals;
mod engine_globals_store;
mod error;
mod error_value;
mod globals;
mod hardening;
mod iteration;
mod json;
mod library_env;
mod markdown;
mod messages;
mod models;
mod models_loop;
mod order;
mod packs;
mod prelude;
mod program;
mod projection;
mod prompt_reads;
mod prose;
mod protocol;
mod proxy;
mod require;
mod scope;
mod section;
mod sys;
mod tools;
mod var;
mod vm;
mod walk_budget;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
#[cfg(test)]
pub(crate) mod test_vm;
#[cfg(test)]
mod tests;

// The executor-facing surface: every item `runvil-runtime` names crosses here.
pub use crate::adhoc::AdhocTools;
pub use crate::argv::Argv;
pub use crate::bindings::{
    DiscoveryTool, RosterEntry, ToolBinding, ToolOutputKind, ToolSet, wire_name_of,
};
pub use crate::collection::render_item;
pub use crate::compactors::{OverflowReason, UsageAnchor, output_reserve, precheck};
pub use crate::coro::install_store_shims;
pub use crate::dispatch::{ScriptReport, ToolDispatch, prepare_dispatch, prepare_model_dispatch};
pub use crate::engine_globals::install_ui;
pub use crate::engine_globals_store::{
    make_store_failure_error, run_store_op, store_error_message, store_probe_message,
};
pub use crate::error::{
    QuotaResource, make_context_exhausted_error, make_internal_at_error, make_internal_error,
    make_lua_quota_error, make_lua_runtime_error, make_task_canceled_error,
    make_task_consumed_error, make_task_not_owned_error, make_tool_loop_exhausted_error,
};
#[cfg(any(test, feature = "test-support"))]
pub use crate::error_value::store_error_value_fields;
pub use crate::error_value::{ErrorField, ErrorKind, error_fields, error_kind, make_store_error};
pub use crate::globals::{RESERVED_NAMES, Reserved, reserved_name};
pub use crate::messages::MessageList;
pub use crate::prelude::install_preludes;
pub use crate::program::LuaProgram;
pub use crate::projection::project_messages;
pub use crate::prose::ProseState;
pub use crate::protocol::{
    AdhocToolOutcome, Answer, Blocked, ChatOutcome, ChatResult, ContentPart, MessageContent,
    MessageRecord, ModelCall, NonEmpty, RecordRole, Request, TaskDelivery, TaskPhase, TaskStatus,
    ToolCallOutcome, ToolCallRecord, VfsOp, VfsOutcome, YieldParse,
};
pub use crate::scope::{OfferMode, OfferedTool, Offers, RenderedTools, ToolCallCounts};
pub use crate::section::{
    Library, LibraryBlock, LibraryKey, LuaBlockResult, SectionRead, SectionText,
};
pub use crate::vm::{
    CoroStep, InjectFailure, Injected, SectionVm, Uninjected, current_catalog_packs,
    current_dynamic_set, current_static_set,
};
