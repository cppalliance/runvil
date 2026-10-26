//! The executor's import surface for `runvil-parser`, which parses a
//! prompt file into a [`Prompt`] tree; that crate documents the prompt
//! format.

pub(crate) use runvil_parser::{Block, BodyText, ParseError, ParseErrorKind, Prompt, Section};
