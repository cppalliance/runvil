//! The tool vocabulary the executor offers and advertises.
//!
//! The caller-supplied [`ToolCatalog`] of `ToolInfo` values lists every
//! tool the run can use: the prompt's Lua calls one by id, and a section
//! offers one to its model with `tools.offer` before its first loop round.
//! The Engine issues each call as a
//! `ToolCall` effect naming the [`ToolId`](runvil_types::tools::ToolId)
//! for the caller to perform; the installed Pack the id names answers it
//! (the `Pack` trait, in `runvil-plugin`). The runtime-agnostic
//! vocabulary - [`ToolCatalog`], [`ToolId`](runvil_types::tools::ToolId),
//! the output and error types - sits in the `runvil-types` crate's
//! `tools` module; this module is the crate-internal import surface for it,
//! and other crates name the vocabulary through `runvil_types::tools`.

pub(crate) use runvil_types::tools::ToolCatalog;
