//! The runtime-agnostic Runvil tool vocabulary.
//!
//! Performing a tool call is the Harness's job. The Engine binds and
//! advertises tools as data and issues each call as an effect naming the
//! tool's stable identity ([`ToolId`]), whatever name the model saw the
//! tool under.
//!
//! This module holds vocabulary only: the implementation-free
//! [`ToolInfo`] and the caller-supplied [`ToolCatalog`] of `ToolInfo` values,
//! trusted output ([`ToolOutput`], [`OutputTrust`]), the model-safe
//! [`ToolError`], a call's [`ToolCallOrigin`], and the contract errors. The
//! implementation trait behind a tool info (`Plugin`) is the Pack
//! contract's, in `runvil-plugin`, which the `plugin-*` crates
//! implement; the prompt parser and
//! the executor sit in their own crates and depend on `runvil-types`.

mod ids;
mod info;
mod origin;
mod output;
mod registry;

pub use ids::{ToolId, ToolIdError, ToolIdErrorKind};
pub use info::ToolInfo;
pub use origin::{ToolCallOrigin, ToolCaller};
pub use output::{OutputTrust, ToolError, ToolErrorKind, ToolOutput};
pub use registry::{ToolCatalog, ToolCatalogError, ToolCatalogErrorKind};

#[cfg(test)]
mod tests;
