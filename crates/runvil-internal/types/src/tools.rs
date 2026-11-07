//! The runtime-agnostic Runvil tool vocabulary.
//!
//! Performing a tool call is the Harness's job. The Engine binds and
//! advertises tools as data and issues each call as an effect naming the
//! tool's stable identity ([`ToolId`]), whatever name the model saw the
//! tool under.
//!
//! This module holds vocabulary only: the implementation-free
//! [`ToolInfo`] and the caller-supplied [`ToolCatalog`] of `ToolInfo` values,
//! trusted output ([`ToolOutput`], [`OutputTrust`]), the [`tool_error`] a
//! failed call returns, and a call's [`ToolCallOrigin`]. The implementation
//! trait behind a tool info (`Pack`) is the Plugin contract's, in
//! `runvil-plugin`, which the `plugin-*` crates implement; the prompt
//! parser and the executor sit in their own crates and depend on
//! `runvil-types`.

mod ids;
mod info;
mod origin;
mod output;
mod registry;

pub use ids::ToolId;
pub use info::ToolInfo;
pub use origin::{ToolCallOrigin, ToolCaller};
pub use output::{OutputTrust, ToolOutput, tool_error};
pub use registry::ToolCatalog;

#[cfg(test)]
#[path = "tools-tests.rs"]
mod tests;
