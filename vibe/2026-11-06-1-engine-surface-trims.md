---
name: Engine surface trims
overview: Remove three pieces of Engine surface that carry no weight - a public type alias nothing outside its crate names, a one-line forwarder in the prompt crate's `detail` module, and a one-line wrapper with one caller in the Lua crate - with no change in behavior.
todos:
  - id: op-sink
    content: "Stop exporting the `OpSink` alias from `runvil-vfs` and narrow it to `pub(crate)`, in its own commit"
    status: pending
  - id: parse-error
    content: "Delete `runvil_prompt::detail::parse_error` and point its two callers at `runvil_types::error::failure`, in its own commit"
    status: pending
  - id: normalize-failure
    content: "Inline `install_normalize_failure` into `install_shims` in `runvil-lua`, in its own commit, and finish with the full gate set"
    status: pending
isProject: false
---

# Engine surface trims

<product-contract>

## Product Requirements

Three items in the Engine crates exist only to be named once. Removing them shrinks what a reviewer reads in `runvil-vfs`, `runvil-prompt`, and `runvil-lua` and changes nothing a run does. No replacement facility is needed for any of them.

- Problem and users:
  - `runvil-vfs` exports `OpSink`, a type alias for `Arc<dyn Fn(OpEvent<'_>) + Send + Sync>`. No crate outside `runvil-vfs` names it, in production or in tests, and the facade does not re-export it. The public builder takes `impl Fn(OpEvent<'_>)`.
  - `runvil_prompt::detail::parse_error` is one call to `runvil_types::error::failure` with its two arguments passed on unchanged. It has one production caller, `Scheduler::check_start`, and one test.
  - `install_normalize_failure` in `runvil-lua` is `lua.create_function(normalized)`. It has one caller, `install_shims` in `coro.rs`.
  - The user is the reviewer reading each crate's surface (`AGENTS.md`, Principles).
- Goals:
  - `OpSink` is nameable only inside `runvil-vfs`.
  - `prompt::detail` holds one item fewer and the Engine calls `runvil_types::error::failure` itself.
  - `install_shims` creates the `normalize_failure` capture directly from `normalized`.
- Non-goals:
  - Gating test-only exports of `runvil-lua` (`ErrorField`, `error_fields`, `RESERVED_NAMES`) behind `test-support`.
  - Any change to `runvil-types`.
  - Inlining `block_guard` (see the Decision Record).
  - Renaming anything, or any change to behavior, a persisted format, or the facade.
- Success criteria:
  - `crates/runvil/public-api.txt` does not change.
  - Every gate in `AGENTS.md` "Verification" passes with no new warning.
- Constraints:
  - Each change is its own commit and leaves the workspace green.
- Open questions: None

## Functional Specification

No behavior changes. Every failure, event, and Lua value is identical before and after.

- Inputs and outputs: none change.
- States and validation: none change.
- Errors and recovery: `check_start` still returns a `StructureInvalid` failure with the same message and no location, stamped by `with_prompt_context` as before.
- Security and privacy behavior: none change.

</product-contract>
<implementation-contract>

## Technical Design

Each change deletes a name and points its users at what the name already forwarded to.

- Architecture:
  - `OpSink` stays as a `pub(crate)` alias in `observe.rs`, because `Volume`, `VfsRef::from_router`, and `VfsRefBuilder` name it. Only the root re-export goes.
  - `parse_error` goes. `check_start` and its test call `runvil_types::error::failure`, which `runvil-engine` already imports in `engine/src/error.rs`.
  - `install_normalize_failure` goes. `install_shims` calls `lua.create_function(normalized)`, and the `error_value` module doc names `normalized` where it named the installer.
- Data model: none.
- Interfaces and APIs: `runvil_vfs::OpSink` and `runvil_prompt::detail::parse_error` leave the crates' surfaces. Neither is in `crates/runvil/public-api.txt`.
- Failure handling: unchanged.
- Observability: unchanged.
- Performance and scale: unchanged.

</implementation-contract>
<verification-contract>

## Testing Plan

No new test, because no behavior changes. The existing tests cover each call site: the vfs observer tests for `OpSink`, `a_parse_failure_run_raises_names_the_prompt_and_its_file` for `parse_error`, and the Lua prelude and `pcall` tests for the `normalize_failure` capture.

- Unit: unchanged.
- Exit criteria:
  - `cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features` and the workshop clippy run, both with `CARGO_BUILD_WARNINGS=deny`
  - `cargo fmt --all --check`
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`
  - `RUSTDOCFLAGS="-D warnings" cargo doc -p runvil --no-deps`
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`
  - `cargo +nightly-2026-09-05 xtask api --check`
  - `cargo test -p build-xtask`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`

</verification-contract>
<decision-record>

## Decision Record

- Decisions:
  - Narrow `OpSink` to `pub(crate)` instead of deleting it. Rationale: three in-crate items name it, and a named alias is shorter than the closure type at each of them.
  - Do not inline `block_guard`. Rationale: `GUARD_REGISTRY` is private to `coro.rs`, which both stores the guard (`install_shims`) and reads it back (`block_guard`). Inlining at the one caller in `vm/run.rs` would make the key `pub(crate)` and split the pair across two modules, to save a three-line function. That is not a clear win.
  - Reuse considered: all three removals reuse the facility the removed name pointed to (`runvil_types::error::failure`, `normalized`, the `impl Fn` builder). No new facility.

### Deferred and Out of Scope

- Deferred: moving `ErrorField`, `error_fields`, and `RESERVED_NAMES` behind `test-support`. The definitions need the same `cfg` as the export, because dead code fails the deny-warnings gate, so it is a larger change than a trim.
- Deferred: `runvil-types`, where another engineer is proposing work.
- Deferred: `block_guard`, for the reason above.

</decision-record>
<execution-plan>

## Execution Instructions

<step-1>

### Step 1: Stop exporting OpSink [pending]

- Component: `op-sink`
- Tests:
  - `cargo clippy -p runvil-vfs --all-targets --all-features`
  - `RUSTDOCFLAGS="-D warnings" cargo doc -p runvil --no-deps`

Build: in `crates/runvil-internal/vfs/src/observe.rs` change `pub type OpSink` to `pub(crate) type OpSink`, and in `lib.rs` drop `OpSink` from `pub use observe::{OpEvent, OpSink, Origin};`.

Commit: one commit. Suggested subject: ``Stop exporting `OpSink` from `runvil-vfs` ``.

</step-1>

<step-2>

### Step 2: Delete prompt::detail::parse_error [pending]

- Component: `parse-error`
- Tests:
  - `cargo nextest run --locked -p runvil-engine -p runvil-prompt --all-features`
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`

Build: delete `parse_error` from `crates/runvil-internal/prompt/src/detail.rs`, dropping `ErrorCode` from that file's imports if nothing else uses it. In `engine/src/execute/scheduler/drive.rs` (`check_start`) and `engine/src/error/tests-chain.rs`, call `runvil_types::error::failure` in its place.

Commit: one commit. Suggested subject: ``Delete `prompt::detail::parse_error` ``.

</step-2>

<step-3>

### Step 3: Inline install_normalize_failure [pending]

- Component: `normalize-failure`
- Tests:
  - `cargo nextest run --locked -p runvil-lua --all-features`
  - The full gate set in the Testing Plan

Build: delete `install_normalize_failure` from `crates/runvil-internal/lua/src/error-value-classify.rs` and its `Function` import, and re-export only `normalized` from `error-value.rs`. In `coro.rs` build the capture with `lua.create_function(normalized).map_err(error::lua)?`. In the `error_value` module doc name `normalized` where it named the installer.

Commit: one commit. Suggested subject: ``Inline `install_normalize_failure` into `install_shims` ``.

</step-3>

</execution-plan>
