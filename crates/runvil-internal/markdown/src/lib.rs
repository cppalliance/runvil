//! Markdown as plain data: text read into blocks, blocks written back as
//! text, and the two grammars that name a section and a block.
//!
//! [`parse`](fn@parse) reads any Markdown text into a list of [`Block`]s,
//! and [`render`](fn@render) writes a list of blocks back.
//! `pulldown-cmark` reads the Markdown and `serde_yaml_ng` reads and
//! writes the frontmatter; this crate only groups what they report.
//! [`split`] is the section path grammar, which every function that names
//! a section shares, and [`Path`] is the form that names a section of
//! parsed text. A [`Selector`] names one kind of block, [`pick`] says which
//! of its matches a lookup keeps, and [`nth`] is how an index counts.
//!
//! The crate holds no Lua. Its caller turns blocks into Lua tables and
//! back.
//!
//! ## Invariants
//!
//! - Depends on no workspace crate. `cargo test -p build-xtask` enforces
//!   the product and container boundaries.
//! - `pulldown-cmark` is named in one module, `parse::events`.
//! - Parsing never fails: any text is a document.
//! - A grammar refuses text with a message that says what was wrong and
//!   what was required. The caller puts its own function's name in front
//!   and raises it as the universal error.

mod block;
mod index;
mod parse;
mod path;
mod render;
mod selector;
#[cfg(test)]
mod test_support;

pub use block::{Block, Kind};
pub use index::{nth, position};
pub use parse::{parse, parse_body};
pub use path::{Path, Step, Steps, escape_title, split};
pub use render::render;
pub use selector::{Pick, Selector, pick};
