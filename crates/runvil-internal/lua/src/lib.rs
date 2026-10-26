//! Sandboxed Lua execution for a section's Lua block.
//!
//! A section's Lua chunk runs in a fresh, restricted `mlua` VM: only the
//! `string`, `table`, `math`, and `utf8` standard libraries plus the safe base
//! functions are available; the raw input `args` string and the runtime `sys`
//! table are exposed; a writable `var` table is provided for the block to
//! populate; an always-on `store` table gives the block the run's virtual
//! files; and an every-Nth-instruction hook polls the run's cancel flag, so
//! even an unbounded loop aborts promptly once the run is cancelled.
//! Direct `print` and `warn` are unavailable. A persistent `log(message)`
//! callback accepts one bounded, single-line UTF-8 string and reports it
//! through the run's emitter as an `Event::Lua` checkpoint. A persistent
//! `json` table decodes JSON text into Lua values (`json.decode` and
//! `json.decode_lines`), encodes Lua values as compact JSON text
//! (`json.encode` and `json.encode_lines`), holds `json.null`, the value
//! every decoded JSON null becomes, and marks a table to encode as a JSON
//! array (`json.array`).
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
//! `store` carrying its `reason` and fields, which surfaces from
//! `SectionVm::run_chunk` as a Lua-category error.
//!
//! In scheduler mode the VM also holds `tasks`, the shims for spawning,
//! waiting on, checking, noting, and cancelling tasks. Pack preludes
//! are the Lua source each Pack the prompt declares contributes: each
//! runs once per VM in an environment of its own before `require`
//! installs, and its globals are checked against the reserved-name list and
//! raw-set into `_G`. The `input` table that `user-input` defines is one:
//! `input.ask()` is an ordinary tool call to that Pack's ask tool.
//!
//! The `tools` table names every tool by its id: `tools.get` reads a tool
//! object, `tools.packs` holds one object per Pack the run knows, the
//! adhoc Pack included, whose `tools` lists that Pack's tool objects, and
//! `tools.adhoc` defines a Lua function as a tool of the adhoc Pack.
//! `tools.offer` and `tools.offer_dynamic` remember tools and Packs, by
//! id or object, for the section's Static Set and Dynamic Set, which the
//! section's first model call freezes; after that, every offer raises.
//!
//! `messages` is the section's one conversation: a list the VM creates
//! empty at each section entry and serves as a global that no assignment
//! replaces. `models.loop` and a model handle's `loop` run over it, and a
//! compactor rewrites it in place.
//!
//! Most of this crate's public items exist for `runvil-engine`'s
//! executor, which drives the VM and the coroutine protocol; the facade
//! re-exports only the store protocol ([`VfsOp`], [`VfsOutcome`]). The
//! `test_support` module, built for this crate's tests and under the
//! `test-support` feature, holds what tests reach beyond that surface.
//!
//! ## Invariants
//!
//! - May depend on: `runvil-types`, `runvil-model-client`, and
//!   `runvil-vfs`. `cargo test -p build-xtask` enforces the product and
//!   container boundaries.
//! - Scheduler state changes only through a typed `Request` yielded by the
//!   installed shim; a direct or malformed yield fails the block without
//!   changing scheduler state.

mod alias;
mod argv;
mod collection;
mod compactors;
mod coro;
mod detail;
mod dispatch;
mod engine_globals;
mod error;
#[path = "error-value.rs"]
mod error_value;
mod globals;
mod handles;
mod hardening;
mod iteration;
mod json;
mod messages;
mod models;
mod models_loop;
mod packs;
mod prelude;
mod program;
mod projection;
mod prose;
mod protocol;
mod proxy;
mod require;
mod scope;
mod sys;
mod tools;
mod vm;
mod walk_budget;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
#[cfg(test)]
pub(crate) mod test_vm;
#[cfg(test)]
mod tests;

// The executor-facing surface: every item `runvil-engine` names crosses
// here. The facade re-exports only `VfsOp` and `VfsOutcome`.
pub use crate::argv::Argv;
pub use crate::collection::render_item;
pub use crate::compactors::{
    OverflowReason, UsageAnchor, exhaustion_detail, output_reserve, precheck,
};
pub use crate::coro::{install_section_loop_shim, install_store_shims};
pub use crate::dispatch::{
    ModelReport, ScriptReport, ToolDispatch, prepare_dispatch, prepare_model_dispatch,
};
pub use crate::engine_globals::{
    install_ui, run_store_op, store_error_message, store_probe_message,
};
pub use crate::error::{Error, QuotaResource};
pub use crate::error_value::{
    ErrorField, ErrorKind, ErrorValue, ParseErrorKindError, Raised, store_error_value_fields,
};
pub use crate::globals::{RESERVED_NAMES, Reserved, reserved_name};
pub use crate::handles::{
    Library, LibraryBlock, LibraryKey, LuaBlockResult, SectionText, ToolBinding, ToolOutputKind,
    ToolSet, wire_name_of,
};
pub use crate::messages::MessageList;
pub use crate::models::ModelRuntime;
pub use crate::prelude::install_preludes;
pub use crate::program::LuaProgram;
pub use crate::projection::project_messages;
pub use crate::prose::ProseState;
pub use crate::protocol::{
    AdhocToolOutcome, Answer, Blocked, ChatOutcome, ChatResult, ContentPart, MessageContent,
    MessageRecord, NonEmpty, RecordRole, Request, SpawnVia, TaskDelivery, TaskPhase, TaskStatus,
    ToolCallOutcome, ToolCallRecord, VfsOp, VfsOutcome, YieldParse,
};
pub use crate::scope::{
    DiscoveryTool, Offer, OfferMode, OfferTarget, OfferedTool, RenderedTools, ToolCallCounts,
    ToolRuntime,
};
pub use crate::sys::enrich_sys_model;
pub use crate::vm::{
    AdhocTools, CoroStep, InjectFailure, Injected, SectionVm, Uninjected, current_catalog_packs,
    current_dynamic_set, current_tool_bindings, resolve_model_binding,
};
