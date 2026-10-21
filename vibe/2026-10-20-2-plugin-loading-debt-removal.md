---
name: Plugin loading debt removal
overview: "Two cheap, reversible fixes for debt the library Plugin loading work introduced: a non-library file in a Plugin directory can stop every Plugin from loading, and the public C contract omits the thread, unload, and failed-load rules the loader enforces and its unsafe comments rely on."
todos:
  - id: d1-1-list-order
    content: "D1-1: add list_plugin_files seam in load.rs, add the regression and characterization tests, see the regression test fail, then test the extension before reading metadata and narrow the two Errors docs"
    status: pending
  - id: d1-2-abi-contract
    content: "D1-2: add the Threads and lifetime section and RvHost field-doc lines to abi.rs, and point the Instance Send SAFETY comment at it"
    status: pending
  - id: verify-exit-checks
    content: "Run the exit checks after each code commit: fmt, both clippy runs, both nextest runs, three docs builds, build-xtask tests, docs-claims"
    status: pending
isProject: false
---

# Library Plugin loading debt removal

<product-contract>

## Product Requirements

- Scope and target work:
  - Target: the seven commits whose `Plan:` trailer names `vibe/2026-10-20-1-library-plugin-loading.md` in the `runvil` repository, from `19957d862` (Share one tool catalog per run) through `b1bbe33eb` (Close plan: library-plugin-loading). Baseline `c4578fd8f`. Endpoint and disposition ref `b1bbe33eb`. The worktree is excluded because it was clean.
  - Evidence: two independent passes (analysis, then challenge) read the target diffs and the endpoint tree through Git objects, with the commit log of all 2,236 reachable commits as testimony.
  - Analysis limits: nothing was built or run. Unix and Windows behavior of `std::fs::metadata` was reasoned from the standard library, not observed. The design record's Technical Design was read at cited lines only. `abi-tests.rs`, `fakes-tests.rs`, `lib-tests.rs`, and most test bodies were read by test name only.
  - Not in the target: `HostRunContext`'s `Debug` output now includes the catalog, the build scripts include the build ceiling through `#[path]`, and the plan file's design text was never synced to the built code. All three were judged and are not debt.
- Cleanup goals and non-goals:
  - Goal: one entry in a Plugin directory that is not a library can no longer stop every Plugin in that directory from loading.
  - Goal: the public C contract in `runvil_plugin::abi` states the thread, unload, and failed-load rules the loader enforces, so the soundness comment on the loader's two `unsafe impl` blocks cites a clause that exists.
  - Non-goal: any change to the C contract's types, verb numbers, or layout. Non-goal: changing what a library candidate's unreadable metadata does. It still fails the call, as the commit that added it intended.
- Success criteria:
  - A directory holding one library and one entry without the library extension, whose metadata read fails, lists the library and returns `Ok`.
  - A directory entry with the library extension whose metadata read fails still returns that error.
  - `abi.rs` states the thread, unload, and failed-load rules, and the `Instance` `Send` comment points at them.
  - Every gate in the Testing Plan's exit checks passes.

## Functional Specification

### Debt Inventory

- Debt added (both introduced, none worsened):
  - D1-1, introduced by `8c22e261c`. `plugin_files` in `crates/harness-internal/plugin-loader/src/load.rs` reads the metadata of every directory entry before it tests the extension (line 35 calls `is_regular_file(std::fs::metadata(&path))?`, line 36 tests the extension). Any error other than `NotFound` returns from `plugin_files`, and `insert_plugin_dir` (`plugin.rs` line 48, `for path in plugin_files(directory)? {`) returns `Err` before any library loads, so no report is produced. Impact: an entry named `notes.txt` that is a symbolic link that loops, or whose target sits in a directory the process cannot search, blocks every Plugin. This contradicts the documented rules that other files "are ignored" (`load.rs` doc, design record line 100) and that one failure never stops the others. The error also names no path. Reversal cost: low. Both functions are `pub(crate)`. Target state: the extension is tested first.
  - D1-2, introduced by `3a4662692` (the contract) and `388e6c443` (the `unsafe impl` pair). `crates/runvil-plugin/src/abi.rs` says nothing about threads. `RV_PLUGIN_UNLOAD` is documented only as "Free the Plugin instance." The comment at `load.rs` lines 137-138 justifies `unsafe impl Send for Instance` with "the C contract lets RV_PLUGIN_UNLOAD come on any thread", a clause the contract does not contain. `plugin.rs` docs name `runvil_plugin::abi` as the contract. The loader enforces the rules in code and tests, so a Plugin author who follows `abi.rs` literally can still break them and learn the rules only from `RV_ERR_MISUSE` or a crash when the library closes. Reversal cost: low today, because version 0.1 promises no compatibility, and it rises with each Plugin written against the silent text. Target state: the rules are in `abi.rs`.
- Cheap fixes: none. Exposed pre-existing debt: none.
- Rejected candidates, 16: residual-but-acceptable 8, weak/speculative 7, unrelated pre-existing 1, false 0.
  - Residual-but-acceptable: leftover `add_*` test names and assertion text after the rename (D1-6, stated in the commit message), no forwarding `PackRegistry::add` in an unpublished crate (D1-7), the public all-field `repr(C)` structs and single verb dispatch (D1-8), the fixture's private copy of the C types (D1-9, its purpose, with layout pins on both sides), a load that sets an instance and is then rejected gets no unload (D1-11, stated rule with three pinning tests), safe public functions that run foreign code (D1-12, recorded decision, same as `whisper-ffi`), the repeated `build.rs` include (D1-13, the runner already did it and the compiler guards the path), and the unused `packs` parameter (D1-17, reserved by `Deferred:` trailers and held by `#[expect]`).
  - Weak/speculative: whole-catalog scan per tool call (D1-3), no new test for the shared catalog (D1-4, the existing runner tests exercise it), a Pack-id collision that the parser already refuses (D1-5), the ignored unload status (D1-10, documented and an accepted risk), repeated counts of unsafe-owning crates (D1-14, planned edits only), the exact-case extension compare (D1-15, documented), and the Windows-only close check (D1-16).
  - Unrelated pre-existing: the empty-catalog fallback (D1-18) was moved, not created.

</product-contract>
<implementation-contract>

## Technical Design

- No public type, signature, verb number, or layout changes. Both changes sit inside two crates.
- D1-1, `crates/harness-internal/plugin-loader/src/load.rs`:
  - Add a private `fn list_plugin_files(directory: &Path, metadata: impl Fn(&Path) -> io::Result<std::fs::Metadata>) -> io::Result<Vec<PathBuf>>` holding the body of `plugin_files`. `plugin_files` becomes a one-line call passing `std::fs::metadata`. The parameter is the seam the regression test needs, because `std::fs::Metadata` cannot be constructed and a real failing entry needs a symbolic link loop that only Unix can make cheaply.
  - In the loop, test `path.extension() != Some(OsStr::new(DLL_EXTENSION))` first and `continue`; only then call `is_regular_file(metadata(&path))?`. A candidate that is a directory or a link to nowhere is still skipped, and a candidate whose metadata fails with anything but `NotFound` still returns the error.
  - Failure behavior: an entry without the library extension can no longer fail the listing. `read_dir`'s own iteration error (`entry?`) is unchanged.
  - Docs: the `# Errors` text of `plugin_files` (lines 24-27) and of `insert_plugin_dir` (`plugin.rs` lines 36-38) say the metadata error applies to an entry with the platform's library extension, and that other entries are never examined beyond their names.
- D1-2, `crates/runvil-plugin/src/abi.rs` and `load.rs`:
  - Add a `# Threads and lifetime` section to the `abi` module doc (after "The contract", before "Reserved verbs") stating four rules, all already enforced by the loader: (1) `RV_PLUGIN_LOAD` runs on the thread of the insert call, and each `RvHost` function answers only the `RvCall` it was handed, only while that load call runs, and only on its thread; any other use, including during `RV_PLUGIN_UNLOAD`, returns `RV_ERR_MISUSE`; (2) `RV_PLUGIN_UNLOAD` may come on any thread, because the last holder of the Plugin drops it, so the instance must be freeable from a thread other than the one that loaded it; (3) `RV_PLUGIN_UNLOAD` must stop every thread the Plugin started and release everything it holds before it returns, because a library Plugin's library closes right after, and a thread still running crashes the process; (4) a load the loader rejects (a refusal, a recorded misuse, a non-OK status, or no description, even when the Plugin returned `RV_OK`) gets no `RV_PLUGIN_UNLOAD`, so the Plugin frees whatever it made before returning on every such path.
  - Add "on the thread running the load call" to the field docs of `RvHost::refuse` and `RvHost::describe_plugin` (lines 112-116).
  - Point the SAFETY comment on `unsafe impl Send for Instance` (`load.rs` lines 137-138) at that section.
  - Wording: write "the loader" and "the `RvHost` functions", never a lowercase "host". Intra-doc links such as [`RV_PLUGIN_UNLOAD`] must resolve, because the docs build denies warnings.

</implementation-contract>
<verification-contract>

## Testing Plan

- Focused (D1-1), in `crates/harness-internal/plugin-loader/src/load-tests.rs`, using the existing `Scratch`, `touch`, `library_name`, and `file_names` helpers:
  - Regression test `plugin_files_does_not_read_the_metadata_of_an_entry_without_the_library_extension`: a scratch directory holds `library_name("plugin")` and `notes.txt`. The `metadata` argument returns `Err(io::Error::from(io::ErrorKind::PermissionDenied))` when the path's file name is `notes.txt` and `std::fs::metadata(path)` otherwise. It calls `list_plugin_files(dir.path(), metadata)` and asserts `file_names(&files) == [library_name("plugin")]`.
    - The assertion that fails at the endpoint ordering is the `.expect("the directory lists")` on the result: line 35's order calls `metadata` for `notes.txt` first and returns the `PermissionDenied` error, so the call panics. Establish this before the reorder: add `list_plugin_files` with today's order, run the test, see it fail with `PermissionDenied`, then reorder.
  - Characterization test `plugin_files_returns_the_metadata_error_of_an_entry_with_the_library_extension`: the same shape, but the failing entry is `library_name("locked")`, and it asserts `expect_err` with kind `PermissionDenied`. This one passes before and after the change. It guards the kept behavior against a reorder that drops the error, and it is not the regression test.
  - Existing checks that keep protecting the rest: `plugin_files_sorts_by_file_name_in_code_point_order`, `plugin_files_keeps_only_files_with_the_platform_library_extension`, `plugin_files_lists_only_the_top_level_and_skips_directories`, `plugin_files_makes_the_paths_of_a_relative_directory_absolute`, `plugin_files_returns_the_error_for_a_directory_it_cannot_read`, and the three `is_regular_file_*` tests, plus the end-to-end `a_file_without_the_library_extension_is_ignored` in `crates/harness-internal/test-plugin/tests/it/insert.rs`. Focused command: `cargo nextest run -p harness-plugin-loader`.
- Integration and end-to-end: the test-plugin crate's end-to-end tests (`cargo nextest run -p harness-test-plugin`) must still pass unchanged.
- Regression (D1-2): documentation only, so there is no new test. The rules it states are already pinned by `an_rvhost_function_called_from_another_thread_is_misuse` (`host-tests.rs`), `the_last_arc_may_drop_on_another_thread` and `an_rvhost_function_called_during_unload_is_misuse` and the three `a_load_that_sets_an_instance_and_..._is_never_unloaded` tests (`plugin-tests.rs`), and the compile-time `send_and_sync` check (`plugin-tests.rs` line 16). Verify with the docs build (below) and by reading the new section against those tests. Add no check on the text's shape.
- Architecture: none. No structural check is proposed.
- Exit checks, run in PowerShell from the `runvil` root after each code commit:
  - `cargo fmt --all --check`
  - both clippy runs with `CARGO_BUILD_WARNINGS=deny`: `cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features` and `cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`
  - both nextest runs: `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features` and `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`
  - with `RUSTDOCFLAGS='-D warnings'`: `cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`, `cargo doc -p runvil --no-deps`, and `cargo doc -p harness --no-deps`
  - `cargo test -p build-xtask`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`
  - skipped: `cargo +<pinned nightly> xtask api --check`, because nothing touches the `runvil` facade or `crates/runvil/public-api.txt`.

</verification-contract>
<decision-record>

## Decision Record

- Reversible decisions and consequences:
  - D1-1 is fixed by testing the extension before reading metadata. Consequence: entries without the library extension are never examined, and a candidate's metadata error still fails the call. Confidence: high - a local reorder that keeps the original commit's stated intent for every candidate.
  - The regression test goes through a private `list_plugin_files` with a `metadata` parameter. Consequence: one small private seam. Falsifier: if review finds the parameter speculative, replace it with a `#[cfg(unix)]` test that makes a symbolic link loop, which then runs only on Unix. Confidence: medium - the seam is the only way to fail on every platform, including the Windows machine this is built on.
  - D1-2 is fixed in `abi.rs`, because `plugin.rs` already names `runvil_plugin::abi` as the contract, and the design record itself says "the contract says so". The same text closes the documentation gap behind D1-11, which stays rejected as code. Confidence: high - documentation only, and every rule already exists in code and tests.
- User-resolved architecture choices: none. No remediation here changes a public interface, a persisted or wire format, component ownership, dependency direction, or a trust boundary, so nothing was raised for a decision.
- Rejected alternatives, assumptions, and risks:
  - Rejected: adding the failing entry's path to the metadata error. Reason: after the reorder only entries with the library extension can fail, it changes the error text a Host may already match, and the finding is about the unrelated entry. Revisit when a Host reports a bare OS error it cannot act on.
  - Rejected: recording a candidate's metadata failure as a report failure instead of returning `Err`. Reason: it changes `PluginReport`'s contents and the commit's explicit choice. Revisit if a Host wants one bad file never to stop a directory.
  - Rejected: folding the `read_dir` iteration error (`entry?`) into the same change. Reason: rarer, and its reason (an unreadable directory) is sound.
  - Assumption: a link loop or an unsearchable link target makes `std::fs::metadata` fail with a non-`NotFound` error on Unix, and the Windows exposure is smaller. Both were reasoned from the standard library, not observed. The seam test does not depend on either.
  - Risk: a reviewer may read the design record, not `abi.rs`, as the contract. The plan resolves it by stating the rules in `abi.rs`, which `plugin.rs` names as the contract.

### Deferred and Out of Scope

- Pin the catalog's Pack-id order with a test whose Pack id sorts after an undeclared `runvil` Pack (D1-4). Order is correct by construction. Revisit if a consumer depends on tool order.
- Rename the leftover `add_*` and `install_adds_*` test names and the three "adds" assertion texts in `crates/workshop/server/src/agents/tests.rs` (D1-6). Revisit when mixed vocabulary causes a mistake.
- A typed `kind()` on `PluginFailure` so a Host can filter dependency libraries reported as missing `runvil_plugin`. Already recorded as a risk in the plan. Revisit when the first real Plugin ships with dependency libraries.
- Say "no Packs yet" in the `harness::plugin` facade module doc (D1-17). Revisit when `insert_pack` lands.
- Skip closing a library after a failed unload (D1-10). Revisit if a real Plugin's unload fails.
- Case-insensitive extension matching on Windows (D1-15). Revisit if a source of uppercase library names appears.
- The empty-catalog fallback in `snapshot` (D1-18) is unrelated pre-existing code that was only moved.

</decision-record>
<project-survey>

## Project Survey

- Status: complete
- Build command: `cargo build --locked -p gateway` (CI's build; plain `cargo build` builds only the default member, the gateway). The desktop app is an explicit choice: `cargo build -p workshop`, or `cargo workshop` (alias for `run -p build-workshop --`). Rust edition 2024, floor `rust-version = "1.95"`, toolchain channel `stable` per `rust-toolchain.toml`. Engine crates also build at the MSRV in CI's msrv job.
- Focused test command pattern: `cargo nextest run --locked -p <crate> -E 'package(<crate>) & test(<name-substring>)'`. Nextest is configured in `.config/nextest.toml`, and CI uses `-E` filtersets in this shape. For one Workshop UI test file: `node --test crates/workshop/ui/test/<file>.mjs`.
- Component test command pattern: `cargo nextest run --locked -p <crate>`, for example `-p harness-plugin-loader`, `-p runvil-engine`, `-p harness`. Workshop components: `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`. Boundary and structural checks: `cargo test -p build-xtask`. Wording scan: `node --test crates/workshop/ui/test/docs-claims.mjs`. UI packages: `npm test --workspaces --if-present` run in `crates/workshop` after `npm ci`, and `npm test` run in `crates/gateway/config-ui/ui`.
- Full-suite test command: `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`, then the Workshop crates separately with the component command above. The ignored build-xtask tests run with `cargo nextest run --locked -p build-xtask --run-ignored only`.
- Linter command: `cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`, and for Workshop `cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`. The gate sets `CARGO_BUILD_WARNINGS=deny` on both. Never run a standalone `cargo check --workspace` beside clippy; the one exception is the headless gate `cargo check -p gateway --no-default-features`. UI typecheck is `npm run typecheck --workspaces --if-present` in `crates/workshop` (`tsc --noEmit`).
- Formatter check command: `cargo fmt --all --check` (the pre-commit hook and CI's fmt job both run it).
- Docs command: `cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api` with `RUSTDOCFLAGS="-D warnings"`. Also `cargo doc -p runvil --no-deps` (facade, default features) and `cargo doc -p harness --no-deps`, both with the same `RUSTDOCFLAGS`. The facade surface check is `cargo +<pinned nightly> xtask api --check` (nightly named in `crates/build-xtask/src/api/toolchain.rs`; run it with `CARGO_BUILD_WARNINGS` unset).
- Test placement and naming conventions: Unit tests sit in a sibling file named `<stem>-tests.rs` beside `<stem>.rs`, wired with `#[cfg(test)] #[path = "<stem>-tests.rs"] mod tests;` (seen in `harness-plugin-loader`, `gateway`, `build-xtask`). Integration tests live under the crate's top-level `tests/` as one Cargo target with a `main.rs` plus topic modules: `tests/suite/` in `harness` and `runvil`, `tests/it/` in `harness-test-plugin` and `gateway`. Test support lives in `support.rs`, `test_support.rs`, or `testing.rs`, called "test support" or "fixtures" in prose, never "harness". UI tests are `*.mjs` under `crates/workshop/ui/test/` and `*.test.mjs` beside sources, run by `node --test`. Engine tests never sleep, read the wall clock, wait with a timeout, or draw OS entropy; random input uses a fixed seed that the failure prints; temp directory names use the test name, process id, and a counter. Behavior changes ship with tests in the same change.
- Directory map: `crates/` holds every Rust crate plus two TypeScript and CSS packages (`crates/shared-ui`, `crates/workshop/look`). `crates/runvil` is the public Engine facade and `crates/runvil-internal/{types,engine,lua,parser,vfs,model-client}` are its private crates. `crates/runvil-plugin` is the Plugin C ABI and Pack registry. `crates/harness` is the Harness facade, with `crates/harness-internal/{runner,plugin-loader,test-plugin}` private. `crates/plugin-user-input`, `plugin-web`, and `plugin-mcp` are the `plugin-*` crates built on `runvil-plugin`. `crates/harness-gateway-client` connects the Harness to the gateway. `crates/gateway/*` (including `stt/` and `config-ui/`) plus `gateway-api-types`, `gateway-api-discovery`, `shared-error-source`, and `shared-loopback` make up the Gateway. `crates/workshop/*` is Workshop, with `ui/` its TypeScript front end and `desktop/` its Tauri app. `crates/build-*` is build tooling (`build-xtask` structural checks, `build-workshop` orchestrator, `build-ui`, `build-user-guide`, `build-llama-cuda`, `build-ceiling`). Outside `crates/`: `guide/` (books, chrome, landing site), `prompts/` (example prompt files), `tools/` (Node scripts for the gateway sidecar and TTS), `images/`, `.github/workflows/` (CI and release), `.githooks/` (`pre-commit` runs fmt; `pre-push` runs the docs-claims scan, the headless gateway check, clippy, and cargo deny), `.config/nextest.toml`, `.cargo/config.toml` (static CRT, `workshop` and `xtask` aliases), `vibe/` (dated change notes as markdown, plus a git-ignored `scratch/`), `local/` (git-ignored local gateway config, env files, and fixtures).
- Component boundaries: The Engine (`runvil` facade over `runvil-internal/*`) depends on no Harness, Gateway, or Workshop crate and never on `tokio`, `tokio-util`, `async-trait`, `reqwest`, `getrandom`, or `fastrand`, directly or transitively; `runvil-plugin` depends only on `runvil-types` and `runvil-vfs`. Dependency direction: `harness` -> `harness-runner`, `harness-plugin-loader`, `runvil`, `runvil-plugin`; `harness-runner` -> `runvil`, `runvil-plugin`; `harness-plugin-loader` -> `runvil-plugin` (opens a Plugin's dynamic library with `libloading`); `harness-test-plugin` -> `harness-plugin-loader`, `runvil-plugin`; `plugin-user-input`, `plugin-web`, and `plugin-mcp` -> `runvil-plugin` only; `harness-gateway-client` -> `harness`, `plugin-web`, `runvil`. The Gateway app lists no Engine or Harness crate and uses the `gateway-*` family. Workshop tiers flow one way: `workshop-server` -> features (`agents`, `gateway`, `run-log`, `workspace`, `menu`, `status`, `user-state`) -> services -> vocabulary (`workshop-protocol`, `workshop-registry`, `workshop-support`); same-tier crates do not depend on each other except `workshop-registry` -> `workshop-protocol`. `workshop-server` depends on `harness`, `runvil`, the `plugin-*` crates, and `harness-gateway-client`. The desktop app (`workshop`) depends on `workshop-server-api`, never `workshop-server`. `build-xtask` enforces these boundaries through `cargo test -p build-xtask`. Only the Harness and Hosts run prompts, and the Host makes every policy decision.
- Conventions summary: Five terms (Engine, Harness, Host, Pack, Plugin) have one meaning each and are capitalized in code comments, docs, rulebooks, and plans; Engine crates call the caller "the caller" and never name the Host. Workspace lints deny `unsafe_code`, clippy `all` and `pedantic`, `unwrap_used`, `expect_used`, `todo`, and `allow_attributes` without a reason; `missing_docs` warns. A build script in every crate fails the build when a Rust file passes 500 lines (`build-ceiling`). Source directories are flat by default: a subdirectory needs at least three files, otherwise the files sit beside the parent as `foo-bar.rs` with `#[path = "foo-bar.rs"] mod bar;`. A Workshop `lib.rs` is a facade only (docs, attributes, `mod`, `pub use`) and opens with a `//!` doc holding a `## Invariants` marker. Comments explain a non-obvious constraint and cite an upstream issue URL for every workaround. Error and status messages are concise, factual, and self-contained, since a model may read them. Cargo features gate real constraints only (toolchain or heavy native build). Persisted JSON round-trips exactly with sorted keys and finite numbers. Workshop UI CSS uses custom properties only, and every persisted UI value goes through the `ui-storage` adapter to the server. Licensed BSL-1.0, workspace version 0.3.0, crate versions and lints inherited from the workspace.

</project-survey>
<execution-plan>

## Execution Instructions

<step-1>

### Step 1: Test the library extension before reading metadata [completed]

- Component: none
- Tests: run in PowerShell from the `runvil` root, all must pass
  - `cargo nextest run -p harness-plugin-loader`
  - `cargo nextest run -p harness-test-plugin`
  - `cargo fmt --all --check`
  - `$env:CARGO_BUILD_WARNINGS = 'deny'; cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features; Remove-Item Env:CARGO_BUILD_WARNINGS`
  - `$env:CARGO_BUILD_WARNINGS = 'deny'; cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets; Remove-Item Env:CARGO_BUILD_WARNINGS`
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`
  - `$env:RUSTDOCFLAGS = '-D warnings'; cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api; cargo doc -p runvil --no-deps; cargo doc -p harness --no-deps; Remove-Item Env:RUSTDOCFLAGS`
  - `cargo test -p build-xtask`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`

Debt D1-1 (introduced by `8c22e261c`): `plugin_files` reads the metadata of every directory entry before it tests the extension, so one entry such as a `notes.txt` that is a looping symbolic link, or whose target sits in a directory the process cannot search, returns an error from `insert_plugin_dir` and no Plugin in that directory loads. The documented rule is that other files "are ignored" and that one failure never stops the others.

Work from `runvil` at `b1bbe33eb` ("Close plan: library-plugin-loading"). Line numbers below are from that commit. Make one commit holding the code, the tests, and the docs.

Artifacts:

- `crates/harness-internal/plugin-loader/src/load.rs`: `plugin_files` (line 28; line 35 calls `is_regular_file(std::fs::metadata(&path))?` and line 36 tests the extension) and its `# Errors` doc (lines 24-27). `is_regular_file` (line 49) does not change. Both functions are `pub(crate)` or private, so no public type or signature changes.
- `crates/harness-internal/plugin-loader/src/load-tests.rs`: the existing helpers `Scratch`, `touch`, `library_name`, and `file_names`.
- `crates/harness-internal/plugin-loader/src/plugin.rs`: the `# Errors` doc of `insert_plugin_dir` (lines 36-38).

Do the work in this order:

1. Add a private `fn list_plugin_files(directory: &Path, metadata: impl Fn(&Path) -> io::Result<std::fs::Metadata>) -> io::Result<Vec<PathBuf>>` holding the current body of `plugin_files` with today's order (metadata first, now through `metadata(&path)`). Make `plugin_files(directory)` a one-line call that passes `std::fs::metadata`; use a closure that calls it if the compiler rejects the function item. The parameter is the seam the regression test needs, because `std::fs::Metadata` cannot be constructed and a real failing entry needs a symbolic link loop that only Unix can make cheaply.
2. Add two tests to `load-tests.rs`:
   - Regression test `plugin_files_does_not_read_the_metadata_of_an_entry_without_the_library_extension`: a scratch directory holds `library_name("plugin")` and `notes.txt`. The `metadata` argument returns `Err(io::Error::from(io::ErrorKind::PermissionDenied))` when the path's file name is `notes.txt` and `std::fs::metadata(path)` otherwise. It calls `list_plugin_files(dir.path(), metadata)` with `.expect("the directory lists")` on the result and asserts `file_names(&files) == [library_name("plugin")]`.
   - Characterization test `plugin_files_returns_the_metadata_error_of_an_entry_with_the_library_extension`: the same shape, but the failing entry is `library_name("locked")`, and it asserts `expect_err` with kind `PermissionDenied`. It passes before and after the reorder. It guards the kept behavior against a reorder that drops the error, and it is not the regression test.
3. Before the reorder, run `cargo nextest run --locked -p harness-plugin-loader -E 'package(harness-plugin-loader) & test(plugin_files_does_not_read_the_metadata_of_an_entry_without_the_library_extension)'` and see it fail: today's order calls `metadata` for `notes.txt` first, so the `.expect("the directory lists")` panics with `PermissionDenied`. If it fails for any other reason or passes, stop and report instead of continuing.
4. Reorder the loop in `list_plugin_files`: test `path.extension() != Some(OsStr::new(DLL_EXTENSION))` first and `continue`; only then call `is_regular_file(metadata(&path))?`, keeping the `metadata` follows-a-link comment beside that call. A candidate that is a directory or a link to nowhere is still skipped, and a candidate whose metadata fails with anything but `NotFound` still returns the error. `read_dir`'s own iteration error (`entry?`) does not change.
5. Update the `# Errors` text of `plugin_files` and of `insert_plugin_dir` to say the metadata error applies to an entry with the platform's library extension, and that other entries are never examined beyond their names. Keep the sentence that an entry missing when its metadata is read is not an error.
6. Run the focused command, then the rest of the Tests list. The existing checks must pass unchanged: `plugin_files_sorts_by_file_name_in_code_point_order`, `plugin_files_keeps_only_files_with_the_platform_library_extension`, `plugin_files_lists_only_the_top_level_and_skips_directories`, `plugin_files_makes_the_paths_of_a_relative_directory_absolute`, `plugin_files_returns_the_error_for_a_directory_it_cannot_read`, the three `is_regular_file_*` tests, and the end-to-end `a_file_without_the_library_extension_is_ignored` in `crates/harness-internal/test-plugin/tests/it/insert.rs`.

Acceptance:

- A directory holding one library and one entry without the library extension, whose metadata read fails, lists the library and returns `Ok`. The regression test proves it.
- A directory entry with the library extension whose metadata read fails still returns that error. The characterization test proves it.
- Do not change what a library candidate's unreadable metadata does, and do not add the failing entry's path to the error text, which a Host may already match.
- Do not fold the `read_dir` iteration error into this change.
- Do not touch the C contract's types, verb numbers, or layout.

</step-1>

<step-2>

### Step 2: State the thread, unload, and failed-load rules in the C contract

- Component: none
- Tests: run in PowerShell from the `runvil` root, all must pass; this step adds no new test, so the docs build with warnings denied is the check on the new text
  - `cargo nextest run -p harness-plugin-loader`
  - `cargo nextest run -p harness-test-plugin`
  - `cargo fmt --all --check`
  - `$env:CARGO_BUILD_WARNINGS = 'deny'; cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features; Remove-Item Env:CARGO_BUILD_WARNINGS`
  - `$env:CARGO_BUILD_WARNINGS = 'deny'; cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets; Remove-Item Env:CARGO_BUILD_WARNINGS`
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`
  - `$env:RUSTDOCFLAGS = '-D warnings'; cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api; cargo doc -p runvil --no-deps; cargo doc -p harness --no-deps; Remove-Item Env:RUSTDOCFLAGS`
  - `cargo test -p build-xtask`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`

Debt D1-2 (introduced by `3a4662692` for the contract and `388e6c443` for the `unsafe impl` pair): `abi.rs` says nothing about threads, and `RV_PLUGIN_UNLOAD` is documented only as "Free the Plugin instance." The comment above `unsafe impl Send for Instance` in `load.rs` justifies it with "the C contract lets RV_PLUGIN_UNLOAD come on any thread", a clause the contract does not contain. `plugin.rs` docs name `runvil_plugin::abi` as the contract. The loader already enforces every rule below in code and tests, so this step writes them down and adds no behavior. Make one commit holding the docs and the comment edit. Step 1 is already committed and has shifted the line numbers in `load.rs`, so find the comment by its text.

Artifacts:

- `crates/runvil-plugin/src/abi.rs`: the `abi` module doc (`//!`) and the field docs of `RvHost::refuse` and `RvHost::describe_plugin` (lines 112-116 at the plan's base commit `b1bbe33eb`).
- `crates/harness-internal/plugin-loader/src/load.rs`: the two-line SAFETY comment above `unsafe impl Send for Instance {}`, which starts "the C contract lets RV_PLUGIN_UNLOAD come on any thread" (lines 137-138 at the base commit). The `unsafe impl Sync for Instance {}` comment below it does not cite the thread rule and stays as it is.

Do the work in this order:

1. Add a `# Threads and lifetime` section to the `abi` module doc, after "The contract" and before "Reserved verbs", stating four rules, all already enforced by the loader:
   1. `RV_PLUGIN_LOAD` runs on the thread of the insert call, and each `RvHost` function answers only the `RvCall` it was handed, only while that load call runs, and only on its thread. Any other use, including during `RV_PLUGIN_UNLOAD`, returns `RV_ERR_MISUSE`.
   2. `RV_PLUGIN_UNLOAD` may come on any thread, because the last holder of the Plugin drops it, so the instance must be freeable from a thread other than the one that loaded it.
   3. `RV_PLUGIN_UNLOAD` must stop every thread the Plugin started and release everything it holds before it returns, because a library Plugin's library closes right after, and a thread still running crashes the process.
   4. A load the loader rejects (a refusal, a recorded misuse, a non-OK status, or no description, even when the Plugin returned `RV_OK`) gets no `RV_PLUGIN_UNLOAD`, so the Plugin frees whatever it made before returning on every such path.
2. Add "on the thread running the load call" to the field docs of `RvHost::refuse` and `RvHost::describe_plugin`.
3. Change the SAFETY comment above `unsafe impl Send for Instance {}` so it points at the `# Threads and lifetime` section of `runvil_plugin::abi` instead of asserting a clause the contract lacked. Keep its second reason, that the loader never reads `plugin`.
4. Wording: write "the loader" and "the `RvHost` functions", never a lowercase "host". Intra-doc links such as [`RV_PLUGIN_UNLOAD`] must resolve, because the docs build denies warnings. Add no test or check on the text's shape.
5. Run the Tests list. Then read the new section against the tests that already pin each rule and confirm every rule matches: `an_rvhost_function_called_from_another_thread_is_misuse` (`host-tests.rs`) for rule 1, `an_rvhost_function_called_during_unload_is_misuse` (`plugin-tests.rs`) for rule 1's unload clause, `the_last_arc_may_drop_on_another_thread` (`plugin-tests.rs`) and the compile-time `send_and_sync` check (`plugin-tests.rs` line 16) for rule 2, and the three `a_load_that_sets_an_instance_and_..._is_never_unloaded` tests (`plugin-tests.rs`) for rule 4. Rule 3 has no test and rests on the loader's close-after-unload order.

Acceptance:

- `abi.rs` states the thread, unload, and failed-load rules, and the `Instance` `Send` comment points at them.
- The docs build passes with warnings denied.
- Do not change the C contract's types, verb numbers, or layout, and do not change any code or behavior in `load.rs` beyond that comment.

</step-2>

</execution-plan>
