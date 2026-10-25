//! `runvil-plugin` - the contract a Plugin crate implements, and the
//! one crate it depends on.
//!
//! A Pack has a fully qualified name (FQN) such as `runvil/web`. Its
//! first segment is the vendor, and its Pack id is the second segment,
//! `web`, which prefixes its tools (`web/fetch`). A Plugin is an entry
//! point which installs zero or more Packs. A linked Plugin is the
//! `install` function of a Plugin crate (a `plugin-*` crate):
//!
//! ```text
//! pub fn install(packs: &mut PackRegistry, config: Value) -> Result<(), InstallError>
//! ```
//!
//! It calls [`PackRegistry::insert`] once for each Pack it offers, with the
//! sub-object of `config` under that Pack's id (`null` when the key is
//! missing), and returns the first error. A Plugin can also be a C entry
//! point with the shape in [`abi`]: a dynamic library exports it as
//! `runvil_plugin`, or a Plugin crate links it in under a name of its own.
//! The Harness loads these entry points when a Host inserts them; they
//! insert no Packs yet.
//! `runvil/subagents` is a built-in Pack every registry holds, which
//! reserves its id for the Engine's subagent tools.
//!
//! The Host owns the [`PackRegistry`]; a Plugin only inserts its Packs into it.
//!
//! Each Pack is a [`PackInfo`], built with [`PackInfo::new`]: its FQN, an
//! optional Lua prelude, the per-run services its calls read, and the
//! `construct` function the Host calls once, when it builds its Packs,
//! to build the one [`Pack`] object every run shares. [`Pack::call`]
//! performs one tool call with the [`ToolContext`] the Harness lends it.
//! [`Pack::render`] returns the tools the model sees, given the tools a
//! section offered from the Pack and the [`RenderContext`] that names the
//! section entry and its model.
//! [`HostServices`] maps service ids to the shared objects a Host provides,
//! each read back through a typed [`ServiceKey`].
//!
//! The Engine never depends on this crate; it sees only `ToolInfo` values,
//! preludes, and ids. The Engine and filesystem names a Plugin author
//! needs are re-exported here, so a Plugin crate names no other
//! runvil crate.
//!
//! The `test-support` feature adds `testing::TestCall`, which lends a
//! [`ToolContext`] to a Plugin crate's own tests.
//!
//! ## Invariants
//!
//! - May depend on: `runvil-types` and `runvil-vfs` among
//!   workspace crates. As an Engine crate it declares no async runtime, no
//!   `async-trait`, and no HTTP client. `cargo test -p build-xtask`
//!   enforces the Engine manifest guard and the product and container
//!   boundaries.
//! - Only `[dev-dependencies]` enable `test-support`, which the
//!   `test-support` leak guard enforces.
//! - A Host builds each Pack object once, through
//!   [`PackInfo::construct`], and every run shares it. Host-wide services
//!   reach only `construct`, and a run's own services reach only calls,
//!   through [`ToolContext::service`]. Cleanup goes in the Pack's
//!   `Drop`; there is no shutdown hook.
//! - Every tool a Pack offers sits under the Pack id `construct`
//!   receives, as `web/fetch` sits under `web`. A tool that does not is
//!   dropped from every run's catalog.
//! - [`Pack::ready`] resolves `Ok` once [`Pack::tools`] is complete,
//!   or `Err` with a reason a model can read when the Pack can't serve
//!   now. The Harness awaits it before each run's snapshot, and a Pack
//!   whose `ready` fails is unavailable to that run. A Pack that
//!   recovers serves the runs that start after it does.
//! - [`Pack::ready`] resolves within a bound the Pack owns and
//!   enforces in its own task. It uses no runtime timer, because it may be
//!   polled outside any particular runtime.
//! - [`Pack::ready`] is cancellation-safe, and after its first
//!   resolution it answers at once unless the Pack's state has changed.
//! - [`Pack::call`] must not block while polled. Every effect of a run
//!   is polled on the same task, so blocking or CPU-heavy work goes to the
//!   Host's runtime.
//! - [`Pack::call`] must not panic. A panicking call's effect is
//!   answered `Dropped`, and the panic is logged.
//! - [`Pack::call`] marks the trust of every output correctly: output
//!   that embeds data an attacker can influence is
//!   [`ToolOutput::untrusted`].
//! - [`Pack::call`] is cancellation-aware: its future may be dropped
//!   before it finishes, on the occasions its invariants list.
//! - [`Pack::render`] runs in the effect loop with the rules of
//!   [`Pack::call`]: it must not block while polled, it must not panic,
//!   and it is cancellation-aware.
//! - [`Pack::render`] takes and returns plain data: its
//!   [`RenderContext`] and its tool lists hold no Rust-only handle such as
//!   [`HostServices`], so a later C verb can pass them serialized. A fact
//!   joins [`RenderContext`] only as plain data.

pub mod abi;
mod context;
mod pack;
mod registry;
mod service;
#[cfg(feature = "test-support")]
pub mod testing;

pub use context::{RenderContext, ToolContext};
pub use pack::{Pack, PackFuture, PackInfo};
pub use registry::{InstallError, PackOrigin, PackRegistry};
pub use service::{HostServices, ServiceError, ServiceId, ServiceKey, TOKIO_RUNTIME};

pub use runvil_types::models::{ModelId, ModelInfo, ThinkingMode};
pub use runvil_types::packs::{PackId, PackIdError, PackIdErrorKind};
pub use runvil_types::tools::{
    OutputTrust, ToolCallOrigin, ToolCaller, ToolError, ToolErrorKind, ToolId, ToolIdError,
    ToolIdErrorKind, ToolInfo, ToolOutput,
};
pub use runvil_vfs::{Access, Entry, FileType, PathReason, Stat, VfsError};
