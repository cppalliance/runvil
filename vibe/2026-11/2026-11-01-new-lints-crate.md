---
name: New lints crate
overview: Add build-lints, a dylint library for workspace rules clippy has no lint for, and its first lint, io_try_without_context, which rejects `?` that converts a std::io::Error into another error type without adding the operation or path.
todos:
  - id: crate
    content: "Create lints/build-lints as its own cargo workspace: cdylib, nightly-2026-08-20 rust-toolchain.toml paired with the clippy_utils rev, dylint-link linker in .cargo/config.toml, dylint_linting and dylint_testing 6.3"
    status: completed
  - id: register
    content: "lib.rs registers every lint by hand in register_lints (dylint_library! plus #[unsafe(no_mangle)]), so the library can hold more than one lint"
    status: completed
  - id: io-lint
    content: "io_try_without_context: flag a TryDesugar match whose Try::branch operand is Result<_, std::io::Error> and whose residual target is not; Deny by default; skip external macros"
    status: completed
  - id: ui-test
    content: "ui/io_try_without_context.rs covers four flagged shapes (custom From, spelled-out Result, Box<dyn Error>, closure) and four allowed ones (map_err first, io to io, non-io Result, Option); cargo test in the lints crate passes"
    status: completed
  - id: wire
    content: "Name the library in the root Cargo.toml's [workspace.metadata.dylint] so `cargo dylint --all` runs it from the repository root"
    status: completed
  - id: survey
    content: "Run the lint over the whole workspace at warn level and record the existing sites (29 in 12 files)"
    status: completed
  - id: fix-sites
    content: "Add context at the 29 existing sites, then run `cargo dylint --all` with the lint at its Deny default"
    status: pending
  - id: ci
    content: "Add a CI job that installs cargo-dylint and dylint-link, runs `cargo test` in lints/build-lints, and runs `cargo dylint --all -- --workspace --all-targets --all-features`; document the command in AGENTS.md Verification"
    status: pending
  - id: naming
    content: "Rename runvil-lints to build-lints: `runvil-*` crates are by definition the Engine, and this crate is not"
    status: completed
isProject: false
---

# New lints crate

## Problem

The Runvil workspace requires a means by which custom lints can be implemented
and applied to the codebase.

## Chosen: a dylint based library

Dylint loads lint libraries built against `rustc_private` into a rustc
driver, so a workspace can write its own late lints with full type
information, the same API clippy's own lints use. This is the smallest
facility that can see the operand's type at a `?`: a structural check in
build-xtask reads source text and cannot tell an `io::Result` from any
other `Result`.

The crate holds every future lint, one module each, registered in
`register_lints`. `dylint_linting::declare_late_lint!` is not used
because it declares a whole library around one lint.

### Placement: `lints/build-lints/`, outside `crates/`

- The library needs a nightly with `rustc-dev` and `#![feature(rustc_private)]`,
  while the workspace pins stable 1.99 in `rust-toolchain.toml`. It has
  its own `[workspace]` table and `rust-toolchain.toml`, so it cannot be a
  workspace member.
- build-xtask's checks walk every crate under `crates/` and bind each one:
  `[lints] workspace = true` inheritance, the `build_ceiling` build-script
  wiring, and, for a `runvil-*` package name, the Engine guards. None of
  those can hold for a crate outside the workspace on another toolchain.
  Outside `crates/`, the walk never sees it.
- The root `Cargo.toml` names it in `[workspace.metadata.dylint]`, so
  `cargo dylint --all` from the repository root builds and runs it.

### Toolchain pairing

`clippy_utils` is unpublished and each git rev builds on one nightly. The
rev and `nightly-2026-08-20` come from `cargo dylint new` (cargo-dylint
6.3.2) and move together, with `cargo dylint upgrade`. This is a different
nightly from the one `cargo xtask api` pins (`nightly-2026-09-05`), and the
two are not tied to each other.

## The first lint: `io_try_without_context`

`x?` lowers to `match Try::branch(x) { Continue(v) => v, Break(r) => return
FromResidual::from_residual(r) }` (`MatchSource::TryDesugar`). The lint
fires when:

1. the scrutinee is a call to the `TryTraitBranch` lang item,
2. the operand's type is `Result<_, std::io::Error>`, and
3. the type of the break arm's `from_residual` call (the enclosing
   function's, closure's, or `try` block's return type) is not.

`std::io::Error` is resolved with clippy_utils' `PathLookup` on
`std::io::Error`, following std's re-export. On this nightly the type is
defined in `core::io`, and it has no diagnostic item.

Default level is `Deny`. Spans from external macros are skipped.

### Rejected: also flag io to io

When the function itself returns `std::io::Result`, `?` converts nothing.
Adding context there would mean wrapping in `io::Error::new(kind, msg)`,
which keeps the error type and also breaks implementations of
`std::io::Read` and `Write`. The caller that converts away from
`std::io::Error` is where the context goes, and the lint fires there.

### What counts as context

Any conversion before the `?` changes the operand's type and so satisfies
the lint: `.map_err(...)` into the universal `Error` with
`Error::with_cause`, anyhow's `.context(...)` or `.with_context(...)`, or a
crate's own conversion. The lint does not judge the message, only that the
bare `From` conversion was not the one used.

## Existing sites

Run at warn level over the whole workspace (`DYLINT_RUSTFLAGS="-W
io_try_without_context" cargo dylint --all -- --workspace --all-targets
--all-features --keep-going`), on 2026-11-04:

| File | Sites |
|---|---|
| `crates/build-xtask/src/new_crate.rs` | 6 |
| `crates/build-ui/tests/it/main.rs` | 6 |
| `crates/workshop/server/src/serve.rs` | 3 |
| `crates/workshop/server/src/agents/language_guide/tests.rs` | 3 |
| `crates/workshop/run-log/src/layout.rs` | 3 |
| `crates/workshop/desktop/build.rs` | 2 |
| `crates/workshop/run-log/src/recorder.rs` | 1 |
| `crates/workshop/run-log/src/append.rs` | 1 |
| `crates/runvil/tests/suite/support.rs` | 1 |
| `crates/runvil/tests/suite/shipped.rs` | 1 |
| `crates/runvil/tests/suite/prepare.rs` | 1 |
| `crates/runvil/tests/suite/parsing.rs` | 1 |

These counts may include a site twice when it compiles for both a lib and
its lib test target. The Engine and Harness crates have no sites outside
the `runvil` facade's test suite.

## The crate name: `build-lints`

The dylint library name is `build_lints`.

## Verification

- `cargo test` in `lints/build-lints`: the UI test passes.
- `cargo clippy --all-targets` and `cargo fmt --check` in
  `lints/build-lints`: clean.
- `cargo test -p build-xtask`: 254 passed. The new top-level directory and
  the root manifest's metadata table break no structural check.
- `node --test crates/workshop/ui/test/docs-claims.mjs`: 4 passed.
- `cargo fmt --all --check`: clean.
