//! Small shared run-support primitives for the Runvil runtime.
//!
//! [`untrusted`] wraps untrusted external data in a nonce-guarded envelope
//! and [`cancel`] is the polled `AtomicBool` cancellation tree the Engine
//! observes. [`event`] is the value form of a run's reports, the
//! [`Event`](event::Event) enum a run returns from `step`, with the
//! payload-free boundaries' constructors in [`event_lifecycle`];
//! [`emitter`] is the provenance-stamping [`Emitter`](emitter::Emitter)
//! every Engine crate reports through and the [`EventSink`](emitter::EventSink)
//! a run drains; and [`metrics`] is the model-call metrics vocabulary those
//! events embed. [`models`] is the public model vocabulary (identity,
//! catalog, model info). [`tools`] is the runtime-agnostic tool vocabulary:
//! the implementation-free [`ToolInfo`](tools::ToolInfo), the
//! caller-provided [`ToolCatalog`](tools::ToolCatalog), trusted output, and
//! the model-safe tool error, and [`packs`] is the Pack
//! identity vocabulary, the [`PackId`](packs::PackId) a
//! prompt declares and a tool id sits under. The implementation trait
//! behind them (`Pack`) is the Plugin contract's, in
//! `runvil-plugin`; the Engine issues effects naming ids and never
//! holds an implementation. [`ids`] is the hierarchical, deterministic
//! identity of a run's chains and tasks and the [`Provenance`](ids::Provenance)
//! replay key stamped on every effect and event; [`timestamp`] is the UTC
//! instant a run starts from, rendered over std alone; and [`seed`] is the
//! caller's [`RunSeed`](seed::RunSeed) and the one-way derivations from
//! it. [`error`] is the universal [`Error`](error::Error) record that can
//! describe any Engine failure. This crate depends on no other runvil crate,
//! so every runvil crate may depend on it, and it declares no async runtime.
//!
//! ## Invariants
//!
//! - May depend on: no workspace crate. `cargo test -p build-xtask`
//!   enforces the product and container boundaries.
//! - [`untrusted`] neutralizes chat-template control delimiters in
//!   untrusted tool and Lua text, and is never applied to assistant
//!   replay or tool-call wire payloads.
//! - [`place`](error::place) is the only code that gives a location its
//!   prompt file or turns a known line into a prompt location, and
//!   [`coordinates`](error::coordinates) is the only code that formats a
//!   location as `file:line`. The parser's span constructors build
//!   file-less spans that `place` completes, and the Lua crate's
//!   `internal_at` writes Rust locations.
//! - A located failure's
//!   [`located_message`](error::Error::located_message) ends with
//!   ` (at file:line)`, except that a structure, fence, or list failure,
//!   the parser's own, ends with ` (prompt ..., line ..., column ...)`,
//!   and a frontmatter failure with ` (line ..., column ...)`.

pub mod cancel;
pub mod emitter;
pub mod error;
pub mod event;
pub mod event_lifecycle;
pub mod ids;
pub mod metrics;
pub mod models;
pub mod names;
pub mod packs;
pub mod seed;
#[cfg(test)]
mod test_support;
pub mod timestamp;
pub mod tools;
pub mod untrusted;
mod untrusted_inventory;
