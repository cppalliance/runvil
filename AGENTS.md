# Runvil

Multi-crate Rust workspace for the Engine, the Harness that runs it, and Workshop, a Host. The Gateway is a separate product in its own repository, `cppalliance/runvil-gateway`.

## Definitions

Seven terms have exactly one meaning each, everywhere in this repository: code comments, docs, rulebooks, and plans. Write them capitalized.

- **Engine**: the `runvil-*` crates. The Engine parses a prompt and steps a run. Whenever a run needs a model reply, a Pack's render of the tools a section offers it, a Pack tool's result, a timer, or a file outside a prelude's install, the Engine emits an effect and waits for its caller to answer it. In production the caller is the Harness.
- **Harness**: the `runvil` crate, which is the one crate a Host depends on, and the `harness-*` crates. The Harness steps the Engine, performs every effect, returns each answer, and records every run through the recorder the Host supplies. Production code runs prompts only through the Harness.
- **Host**: an application that runs prompts through the Harness, such as Workshop or Papergate. The Host makes every policy decision. It may also use the Engine's parser and types to read prompts and show events.
- **Pack**: code which installs an optional native prelude, an optional Lua prelude, and zero or more tools.
- **Plugin**: an entry point which installs zero or more Packs.
- **Static Set**: the tools a section entry renders into the model's tool list at its first loop round, the first round a `models.loop` call sends. It is frozen after that.
- **Dynamic Set**: the tools the model finds and calls through the Engine's two discovery tools.

### Using the terms

- "Host", in any capitalization, means only the Host. Other senses use another word: "machine", "real files", "Engine globals", "Engine call", the part's own name ("the desktop app"), or a verb such as run, serve, embed, or hold.
- "Engine", "Harness", and "Plugin" mean only the defined terms. Anything else gets a qualified lowercase name, such as the Gateway's speech engine or a Tauri plugin. This repository's own checks are "structural checks", "test support", or "fixtures".
- Engine crates call the code that steps a run "the caller" and never mention the Host. They name the Harness only to say that a responsibility belongs to it, never how the Harness performs it.
- Names defined elsewhere keep their own meaning (the HTTP `Host` header, Cargo's `harness = false`), crate names are written as they are, and quotations of people stay verbatim.
- `crates/workshop/ui/test/docs-claims.mjs` enforces these rules in every `AGENTS.md`, every `## Invariants` crate doc, and every `.cursor/rules` file.

## Principles

- Design for human review. A reviewer reads one crate at a time: its code, its surface (every item another crate can name, methods, fields, and variants included), and the items it names from other crates. Keep all three small, and prefer more, smaller crates to fewer, larger ones.
- Do more with less. Before building, check whether existing facilities subsume the work: reuse one; else make the smallest improvement that enables the feature, preferring one that serves a problem class beyond this case when it costs no more; else add a new facility, which needs a material benefit beyond tidiness. Prefer simple primitives that generalize over mechanisms specified as a list of requirements.
- Error and status messages assume a model reads them: concise, factual, and self-contained, naming what is missing or unmet with required versus actual.
- There are no users and no recorded runs, logs, or stored data to preserve, so backward compatibility does not matter: change formats, schemas, and APIs freely, and never bump a format, schema, or crate version number.

## Engineering

- Prefer types and compiler checks, then behavior tests and deterministic fault injection. Gate on yes/no properties, never on a score threshold that ordinary work edits.
- Behavior changes ship with tests in the same change. Preserve product and behavior tests during refactors.
- A Cargo feature gates a real constraint such as a toolchain requirement or heavy native build, not product shape. Feature-disabled builds must not leak optional types into core paths.
- Runtime and serve paths never compile native dependencies or invoke build tools.
- Unsafe code stays in its explicitly owned boundary.
- Engine and Harness crates report every failure as the universal `Error` in `runvil-types`, which `runvil-engine` and `runvil` re-export. A new failure is a new `ErrorCode` variant in its hundreds family with a never-used number, plus an `ErrorFact` variant only when no key names the value. No other error enum or struct, `thiserror` derive, or `impl std::error::Error`; a foreign cause joins through `Error::with_cause`. Allowed: `VfsError` in `runvil-vfs`, throwaway test error types, crate-private `Err` markers that the caller turns into `Error`, and pairs such as `InjectFailure`.
- Comments explain a non-obvious constraint, ordering requirement, or workaround. A platform or external-bug workaround cites its upstream issue URL.
- JSON that reaches a recorder or a replay comparison round-trips exactly: object keys sorted, numbers finite, parsed with `float_roundtrip`. A value derived and then logged is rounded to its meaningful precision at the source.

## Verification

- Tests: `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`, and `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`.
- Lint: `cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`, and `cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`, both with `CARGO_BUILD_WARNINGS=deny`. Clippy covers `cargo check`, so never run a standalone `cargo check --workspace`.
- Format: `cargo fmt --all --check`.
- Docs: `cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api` and `cargo doc -p runvil-engine --no-deps` (default features), both with `RUSTDOCFLAGS="-D warnings"`. Clippy does not run rustdoc lints, so never skip these.
- Facade surface: `cargo +<nightly> xtask api --check`, with the nightly named in `crates/build-xtask/src/api/toolchain.rs` and `CARGO_BUILD_WARNINGS` unset (`Remove-Item Env:CARGO_BUILD_WARNINGS` in PowerShell). It fails when the surface names an internal crate or differs from `crates/runvil-engine/public-api.txt`.
- Structural checks: `cargo test -p build-xtask`.
- Wording: `node --test crates/workshop/ui/test/docs-claims.mjs`, from any directory, no `npm ci` needed.

## Structural Rules

- If Cargo rejects a dependency cycle, the design is wrong, not the graph.
- Pseudo-random test input comes only from a fixed seed that the failure prints. Temporary directory names come from the test's name, the process id, and a counter.
- A file holds one concept and has no length limit. Split a file only when it holds a second unrelated concept. Size limits sit on functions and types, and clippy and `cargo test -p build-xtask` enforce them. Split an oversized type by responsibility; a new entry in the allowlist in `crates/build-xtask/src/type_size.rs` is a plan decision with a reason, never a way to make a change pass.
- Production code uses the standard module layout: `foo.rs`, with `foo/bar.rs` for its children. No `#[path]`, and no inline `mod` blocks. A module never has exactly one child file (tests aside): merge it, or make it a peer module `foo_bar.rs` declared beside `foo.rs`.
- Test code goes at the bottom of the file under test in a `#[cfg(test)] mod tests { }` block when practical (if in doubt, beside), and otherwise in `foo-tests.rs` beside it, wired with `#[path = "foo-tests.rs"] mod tests;`. A big test file may split by topic into siblings named after it, `<stem>-<topic>.rs` (so `foo-tests-topic.rs`). These are the only files wired with `#[path]`, apart from the cfg-selected platform files in `crates/runvil-internal/vfs/src/real.rs`, so a hyphen in a file name means test code. Top-level `tests/` and `benches/` trees follow Cargo's conventions.
- Crates outside the Engine and Harness keep their older layout until their own pass.

## SPA and CSS Rules

- Workshop UI component CSS uses the custom properties in `@workshop/look` and `ui/src/tokens/component.css`, never raw color, size, or spacing values.
- Every persisted value goes through the `ui-storage` adapter to the server. Account state (preferences and ephemera such as editor toggles, zoom, recent files, and command history) goes to `ui-state.json` in the state directory (`workshop-user-state`, `/user/state`). Workspace-scoped state, anything that should travel with the `.rvwork` document, goes to the workspace file (`/workspace/file/state`). A new persisted value is a new allow-listed key in one of those two buckets, added on the server first.
