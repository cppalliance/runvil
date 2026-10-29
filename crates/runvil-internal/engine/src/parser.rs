//! The executor's import surface for `runvil-parser`, which parses a
//! prompt file into a [`Prompt`] tree; that crate documents the prompt
//! format.
//!
//! This module is the one surface production code imports that crate
//! through: an item or helper the Engine needs is re-exported here, and a
//! production module names `crate::parser` rather than `runvil_parser`. The
//! `detail` module is re-exported whole, since its helpers are the parser's
//! own algorithms, called where the rule each one implements is documented.
//!
//! Three groups still name `runvil_parser` itself:
//!
//! - `error::convert`, the error bridge, which maps the parser's error type
//!   variant for variant.
//! - The test suites, which import the prompt fixtures under
//!   `runvil_parser::test_support` and call a `detail` helper directly.
//! - The crate's bench, `benches/models_loop.rs`, a separate crate that
//!   cannot see a `pub(crate)` surface.

pub(crate) use runvil_parser::detail;
pub(crate) use runvil_parser::{
    Block, BodyText, ModelKeyword, ParseError, ParseErrorKind, Prompt, Section,
};
