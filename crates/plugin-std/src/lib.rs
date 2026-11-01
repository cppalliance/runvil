//! `plugin-std` - the Pack that gives a run the standard file tools a
//! coding agent needs: read, write, string replace, delete, glob, grep,
//! and list directory.
//!
//! The crate's Plugin, [`install`], inserts the Pack `runvil/std`, whose
//! Pack id is `std`, and a run then holds all of its tools unless it
//! declares another vendor's `std`:
//!
//! | Lua id | Name the model calls |
//! |---|---|
//! | `std/read` | `Read` |
//! | `std/write` | `Write` |
//! | `std/replace` | `StrReplace` |
//! | `std/rm` | `Delete` |
//! | `std/glob` | `Glob` |
//! | `std/grep` | `Grep` |
//! | `std/dir` | `ListDir` |
//!
//! A script calls a tool by its Lua id, `tools.call("std/read", { path =
//! "/notes.md" })`, and offers it to the model with `tools.offer("std")`
//! or `tools.offer("std/read")`. The model then calls it by its wire name,
//! `Read`, which is Cursor's name for the same tool.
//!
//! The Pack has no prelude, so declaring it, as in `uses: [std]`, only
//! makes it required. It needs no per-run service and takes no
//! configuration but `null` or `{}`.
//!
//! # The definitions
//!
//! Each tool's description and parameter schema live in a JSON file under
//! `schemas/`, embedded at build time. They are Cursor's own
//! definitions, so a model that knows Cursor's tools already knows these,
//! with the lines that describe behavior this Pack does not have removed.
//! `schemas/EDITS.md` lists every such edit. ListDir's definition is this
//! crate's own, because Cursor's counterpart is deprecated, and the same
//! file says so.
//!
//! # This build
//!
//! Read is real. It returns a window of a text file's lines, each with a
//! six-wide line number, and it refuses a directory, a file over 16 MiB,
//! and binary data. A read that stops early ends with a line that names
//! the offset to continue from. File text comes back as untrusted output.
//!
//! ListDir is real. It returns a bullet tree of a directory, listed breadth
//! first, with a trailing `/` on each directory, files and directories in
//! one byte-ordered list, and every entry named `.git` skipped. A link is
//! listed and not entered, and an `ignore_globs` pattern leaves out what
//! it matches at any depth. The tree holds at most 15,000 bytes of lines,
//! and a listing that reaches that says so. Names come back as untrusted
//! output.
//!
//! Grep is real. It searches the files under a path with a Rust regular
//! expression, in path order, and prints each matching line as `N:text`
//! with optional context lines as `N-text`, where `N` is the line number
//! Read shows. Output stops at 15,000 bytes, a call with an `offset`
//! pages through the same result, and a binary file, a link, an over-size
//! file, and an entry that cannot be read are skipped and counted, never
//! a failure. A search yields to the executor now and then, so a cancel
//! or a stop can drop it. File text comes back as untrusted output.
//!
//! Write, StrReplace, Delete, and Glob are still stubs: a call with
//! arguments that match the schema answers [`NOT_IMPLEMENTED`]. For every
//! tool, a call whose arguments do not match fails with an
//! invalid-arguments [`ToolError`](runvil_plugin::ToolError) naming the
//! parameter. The stubs exist so that the crate, the definitions, the
//! ids, the wire names, and the Lua path can be proven before any file
//! logic exists.
//!
//! # Where the file access goes
//!
//! Every filesystem operation a tool performs goes through the
//! call's [`Access`](runvil_plugin::Access), never the disk directly. That
//! puts every read and write under the run's `ModePolicy`, so a run in
//! `Mode::Ask` refuses every mutation without the Pack deciding anything,
//! and a run in `Mode::Plan` allows mutations to markdown paths alone. The
//! tools only report the resulting `VfsError` as a `ToolError` the model
//! can read.
//!
//! Shell is deliberately absent. An in-process shell over the same
//! filesystem is a separate Pack, so a run that wants one declares it.
//!
//! ## Invariants
//!
//! - May depend on: `runvil-plugin`, `shared-*` crates, and outside
//!   libraries. `cargo test -p build-xtask` enforces the Plugin family's
//!   allow-list.
//! - The crate names no async runtime and no filesystem of its own. Every
//!   operation reaches files through the call's `Access`.
//! - A tool's description and parameter schema come from its JSON file
//!   unchanged, except for the edits `schemas/EDITS.md` lists. A test per
//!   tool holds the wire name in place.
//! - A call whose arguments the schema refuses is an
//!   [`InvalidArguments`](runvil_plugin::ToolErrorKind::InvalidArguments)
//!   failure that names the parameter. No other outcome reports the
//!   arguments, so a model can tell a bad call from a missing
//!   implementation.
//! - A key the schema does not declare is refused by name, with the
//!   parameters listed, by the three real tools, Read, ListDir, and
//!   Grep. The stubs let it through. Grep alone accepts `-n` when it is
//!   `true`, because Cursor's definition once declared it.

mod check;
mod dir;
mod grep;
mod matcher;
mod order;
mod pack;
mod read;
mod refusal;
mod spec;
mod text;

pub use crate::pack::{NOT_IMPLEMENTED, install};
