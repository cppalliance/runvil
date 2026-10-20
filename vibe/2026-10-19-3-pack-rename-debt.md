---
name: Pack rename debt removal
overview: "Debt review of the Pack and Plugin rename (runvil fc4a1c0ed..8faa39a76, baseline upstream/master) found no introduced or worsened debt and one cheap fix: HostContext keeps a services field that only its Debug impl reads since HostContext::install was removed."
todos:
  - id: remove-hostcontext-services
    content: "C1: remove HostContext's dead services field, its Debug line, and the stale doc clauses in crates/harness-internal/runner/src/host.rs; pass harness-runner, harness, and workshop-server tests, fmt, clippy, and rustdoc"
    status: pending
isProject: false
---

# Pack Rename Debt Removal

<product-contract>

## Product Requirements

- Scope and target work:
  - Repository: `runvil`.
  - Target: `fc4a1c0ed..8faa39a76`, the six commits of the Pack and Plugin rename plan (`vibe/2026-10-19-2-pack-plugin-rename.md`): `7182e88e5`, `901797bf9`, `808028b3f`, `176c78340`, `61cea9eb6`, and `8faa39a76`.
  - Baseline: `upstream/master` at `fc4a1c0ed`, which is also the merge base. Endpoint and disposition ref: `8faa39a76`. The worktree was clean and is excluded.
  - The same plan's commits in `runvil-docs` (`62ae759`) and `runvil-design` (`69b03e8`) are outside this target.
- Cleanup goals:
  - Remove the dead `services` field from `HostContext` that `61cea9eb6` left behind, along with the doc text that describes it.
- Non-goals:
  - Any change to behavior, public interfaces, wire or persisted formats, or Debug output beyond dropping the dead field.
  - Any alias or compatibility path for `plugins:` or the `plugins` Lua global (see Decision Record).
  - Any rejected candidate listed under Debt Inventory.
- Success criteria:
  - `HostContext` holds only `built`, and nothing in `crates/harness-internal/runner/src/host.rs` reads `self.services`.
  - The `runvil` gates that cover `harness-runner` and its callers pass with no test changes.

## Functional Specification

### Debt Inventory

- Debt added: none.
  - Introduced: 0. Worsened: 0.
  - The behavior-bearing changes match their commit claims and the design record's acceptance criteria: the registry move, FQN-keyed config and ready failures, the per-run pick, `PackRef` parsing, the mcp Pack's listing and call paths, and the Workshop wiring. No incorrect behavior, credential path, durable-data path, or irreversible-corruption path was found.
- Cheap fixes:
  - C1, the dead `HostContext.services` field, from `61cea9eb6`:
    - Facts at `8faa39a76`:
      - `crates/harness-internal/runner/src/host.rs` declares `services: HostServices` on `HostContext` (line 53).
      - `HostContext::new` builds every Pack with its own `services` argument (line 91), then stores it (line 103).
      - The only read of the field is `.field("services", &self.services)` in `HostContext`'s `Debug` impl (line 234).
      - Before `61cea9eb6`, `HostContext::install` read it to build each Pack (line 124 at `808028b3f`).
      - `HostContext::begin_run` takes the run's own `services` as a parameter (line 206), and `HostRunContext` stores that value in its own field.
    - Impact:
      - The field is dead state. The struct doc misdescribes the type ("the Host-wide services their `construct` functions read"), though every `construct` has already run when `new` returns.
      - The hand-written `Debug` keeps the dead-code lint quiet.
    - Reversal cost: none. The field is private, and `Debug` output is not a stable format.
    - Target state: `HostContext { built }`, with its docs describing only the built Packs.
- Exposed pre-existing debt: none retained after the challenge. D1-1 is recorded under Decision Record as a watch item.
- Rejected candidates, 19 in all:
  - Residual-but-acceptable, 9:
    - D1-1: the vocabulary spelled into prompt and script surfaces.
    - D1-2: no vendor check in `PackRegistry::add`.
    - D1-4: each no-alias schema change.
    - D1-5: the mcp two-tool dispatch and JSON listing.
    - D1-8: the bounded MCP retry warning.
    - D1-10: `PackId`'s `Deserialize` is used only by tests now, but it is public facade surface.
    - D1-13: the contract's `uses` drops the vendor, and nothing reads it.
    - C2: two vendors' Packs with one Pack id cannot share a run.
    - C3: Workshop's chat always sees the two mcp tools.
    - Each is specified or accepted in the design record, or has no consumer.
  - Weak or speculative, 8: D1-3, D1-6, D1-9, D1-12, D1-14, D1-15, D1-16, and D1-17. These are structural leads with no demonstrated consequence. Existing tests already guard D1-9, D1-15, and D1-16.
  - False, 1: D1-11. The `plugins` Debug field and `plugin` log key that Step 2 left were fixed in `808028b3f`.
  - Unrelated pre-existing, 1: D1-7, an MCP session that ends still reports `ready`. That behavior was documented at the baseline.

</product-contract>
<implementation-contract>

## Technical Design

- `crates/harness-internal/runner/src/host.rs`:
  - Remove `services: HostServices` from `pub struct HostContext`, which leaves `built: BTreeMap<&'static str, Built>`.
  - `HostContext::new(packs: PackRegistry, services: HostServices) -> HostContext` keeps its public signature. It still hands `&services` to each `construct`, and ends with `HostContext { built }`. The `services` argument drops when `new` returns.
  - Change the struct doc's first sentence from "Every Pack a Host built and the Host-wide services their `construct` functions read." to "Every Pack a Host built." Keep the rest of the doc.
  - `impl fmt::Debug for HostContext`:
    - Remove `.field("services", &self.services)`.
    - Change the doc from "Shows the services and each built Pack's FQN and Pack id with whether it built, and never a Pack's config." to "Shows each built Pack's FQN and Pack id with whether it built, and never a Pack's config."
  - Keep the `new` doc's sentence "They reach only `construct`; a tool call reads the run's own services instead.", which stays true.
  - Leave the `HostServices` import in place, because `new` and `begin_run` still take it.
- Lifetime: the stored field owns nothing a Pack depends on.
  - Each Pack that uses a provider takes its own `Arc` at construct time: `plugin-web`'s `construct` takes the search provider and the runtime (`crates/plugin-web/src/web.rs` lines 83 and 86), and `plugin-mcp`'s takes the runtime handle (`crates/plugin-mcp/src/lib.rs` line 157).
  - Workshop provides only `tokio::runtime::Handle::try_current()` (`crates/workshop/server/src/agents.rs` line 181), a handle to the runtime the server already owns.
- No public interface, persisted or wire format, ownership, dependency direction, or trust boundary changes. `crates/runvil/public-api.txt` is unaffected, because `harness-runner` is not part of the `runvil` facade.

</implementation-contract>
<verification-contract>

## Testing Plan

- C1 is dead code, not incorrect behavior, so it needs no regression test. The compiler checks the removal: any remaining reader of `HostContext.services` fails to build.
- Focused: `cargo nextest run --locked -p harness-runner --all-features`. The existing `Debug` assertion in `crates/harness-internal/runner/src/host-tests.rs` (line 215, `let shown = format!("{host:?}")`) checks only FQNs, so it keeps passing unchanged.
- Integration:
  - `cargo nextest run --locked -p harness --all-features`.
  - `cargo nextest run --locked -p workshop-server`, whose host-setup tests build `HostContext::new` over the real Plugins and runtime handle. One of them is `every_run_holds_the_packs_the_three_plugins_add_and_the_builtin` in `crates/workshop/server/src/agents/tests.rs`.
- Architecture and exit:
  - `cargo fmt --all --check`.
  - With `$env:CARGO_BUILD_WARNINGS="deny"`: `cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features` and `cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`.
  - With `$env:RUSTDOCFLAGS="-D warnings"`: `cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`.
  - `git grep -n "self.services" -- crates/harness-internal/runner/src/host.rs` prints nothing.

</verification-contract>
<decision-record>

## Decision Record

- Reversible decisions and consequences:
  - Remove C1's field outright, not just its `Debug` line, and keep `HostContext::new`'s signature.
    - Consequence: no caller changes, and `HostContext`'s `Debug` stops printing services.
    - Confidence: high. The field is private, and its only reader is `Debug`.
- User-resolved architecture choices: none. No retained finding needs a hard-to-reverse remedy.
- Rejected alternatives:
  - Keep the field and only fix its doc. Rejected because it leaves dead state that the lint cannot see.
  - Drop the `services` parameter from `HostContext::new`. Rejected because every `construct` still reads Host-wide services, and the signature is public through `harness::pack`.
  - Add a compatibility alias for `plugins:` and the `plugins` global (D1-1). Rejected for three reasons:
    - The design record already rejected it: "the unknown-field error is enough".
    - Neither rename was corrective work. Both were the owner's planned vocabulary decisions.
    - An alias would spare no file in the repository.
- Assumptions:
  - No Host or test depends on `HostContext`'s `Debug` showing services. A search found the single `Debug` assertion, which checks FQNs.
- Risks:
  - A provider whose `Drop` has side effects now drops when `new` returns, unless a Pack took its own `Arc`. No such provider exists in the tree, and the Workshop host-setup tests cover the real providers.

### Deferred and Out of Scope

- D1-1: script-facing vocabulary churn. The unit was renamed with no alias on 2026-10-04 and again on 2026-10-19. Revisit when old prompts in other repositories need help migrating, which is the design record's trigger, or before the next script-facing rename, such as the toolset design's tool identity work.
- D1-10: `PackId`'s `Deserialize` impl has no non-test caller. Removing it would change the `runvil` facade. Revisit at the next facade-breaking change.
- D1-2 and D1-3: the vendor check and the punctuation-twin ids. Revisit in the dynamic-library step, where library Plugins can be untrusted.
- C2: two vendors' Packs with one Pack id in one run. Revisit in the toolset design, which owns the tool id grammar.
- D1-7: an MCP session that ends still reports `ready`. This is pre-existing and documented, and it is out of scope.
- The `runvil-docs` and `runvil-design` commits of the same plan were not analyzed.

</decision-record>
<project-survey>

## Project Survey

- Status: complete
- Build command: `cargo build --locked -p gateway` (plain `cargo build` builds only the default member, the gateway). The Workshop desktop app is `cargo build --locked -p workshop`, after `npm ci --prefix crates/workshop` and `npm ci --prefix crates/gateway/config-ui/ui`.
- Focused test command pattern: `cargo nextest run --locked -p <package> --all-features -E 'test(<test_name>)'`. For `workshop`, `workshop-server`, and `workshop-server-api`, drop `--all-features`. Native whisper tests are `#[ignore]`d and run with `--run-ignored only --test-threads 1`.
- Component test command pattern: `cargo nextest run --locked -p <package> --all-features` (no `--all-features` for the three workshop packages above). Structural checks for a boundary or wording change: `cargo test -p build-xtask` and `node --test crates/workshop/ui/test/docs-claims.mjs`.
- Full-suite test command: `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`, then `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`, then `node --test crates/workshop/ui/test/docs-claims.mjs`. CI also runs `npm test --workspaces --if-present` in `crates/workshop` and `npm test` in `crates/gateway/config-ui/ui`.
- Linter command: `CARGO_BUILD_WARNINGS=deny cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features` and `CARGO_BUILD_WARNINGS=deny cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`, plus the headless shape check `cargo check -p gateway --no-default-features`. CI adds `cargo deny check` and `cargo audit`. Never run a standalone `cargo check --workspace`.
- Formatter check command: `cargo fmt --all --check`.
- Docs command: `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`, then `RUSTDOCFLAGS="-D warnings" cargo doc -p runvil --no-deps` (CI also runs `cargo doc -p harness --no-deps`). Facade surface: `cargo +<pinned nightly> xtask api --check` with `CARGO_BUILD_WARNINGS` unset, nightly named in `crates/build-xtask/src/api/toolchain.rs`, listing committed in `crates/runvil/public-api.txt`. Guide site: `MDBOOK=<path> cargo xtask site`.
- Test placement and naming conventions:
  - Unit tests sit in a sibling `foo-tests.rs` wired from `foo.rs` by `#[cfg(test)] #[path = "foo-tests.rs"] mod tests;`. When a group reaches three files it moves into `foo/tests.rs` or a `foo/tests/` subdirectory. Small modules sometimes keep an inline `mod tests { }`. A few older files in `build-xtask` use `_tests.rs`.
  - Crate-private test helpers live in a `#[cfg(test)] mod test_support;` file (`test_support.rs`).
  - Each crate has one integration test binary at `tests/<target>/main.rs`, named `suite` or `it`, with one module file per area and a `support.rs`. Prompt fixtures for the facade are in `crates/runvil/tests/prompts/`.
  - Test functions are snake_case sentences that describe the behavior, such as `an_output_store_failure_renders_its_own_message_and_sources_the_engines_error`.
  - Engine tests never sleep, read the wall clock, wait with a timeout, or draw OS entropy. Random input comes from a fixed seed that the failure prints. Temporary directory names come from the test name, process id, and a counter.
  - Criterion benches live in `benches/` for `runvil-engine` and `runvil-lua`. UI tests are `.mjs` files run by `node --test` with jsdom under `crates/workshop/ui/test/`. Node tool scripts in `tools/` have a `.test.mjs` beside them.
- Directory map:
  - `crates/` top level is the public and shared layer: `runvil` (the Engine facade), `runvil-plugin` (the Pack and Plugin API), `harness`, `harness-gateway-client`, the Packs `plugin-mcp`, `plugin-web`, and `plugin-user-input`, `gateway-api-types` and `gateway-api-discovery` (gateway vocabulary and discovery), `shared-error-source` and `shared-loopback`, `shared-ui` (a TypeScript and CSS package, not a Rust crate), and build tooling `build-xtask` (structural checks and `cargo xtask`), `build-workshop` (`cargo workshop`), `build-ui`, `build-user-guide`, `build-llama-cuda`, and `build-ceiling`.
  - `crates/runvil-internal/` holds the Engine's private crates: `types`, `vfs`, `model-client`, `lua`, `parser`, and `engine`.
  - `crates/harness-internal/runner` holds the Harness's private `harness-runner`.
  - `crates/gateway/` holds the gateway family: `app` (the `runvil-gateway` binary), `cloud-providers`, `config`, `config-ui`, `local`, `logging`, `progress`, `protocol`, `routing`, `web-search`, and `stt/` (`api`, `engine`, `backend-whisper`, `whisper-ffi`).
  - `crates/workshop/` holds Workshop: `desktop` (the Tauri app, package `workshop`), `server`, `server-api`, feature and service crates (`agents`, `gateway`, `menu`, `protocol`, `registry`, `run-log`, `status`, `support`, `user-state`, `workspace`), and the npm workspace `ui`, `look`, and `platform`.
  - `guide/` holds the mdBook sources (`books/language`, `books/gateway`, `books/workshop`), site chrome, and the landing page.
  - `prompts/` holds sample prompt programs (Markdown with frontmatter). `tools/` holds Node scripts for staging the gateway sidecar and live TTS checks.
  - `vibe/` holds tracked, dated plan and run records named `YYYY-MM-DD-N-<slug>.md`, with monthly folders; `vibe/scratch/` is ignored.
  - `.github/workflows/` holds CI, release, site, nightly, Miri, and CUDA jobs. `.githooks/` runs fmt before commit and the wording scan, headless check, clippy, and deny before push. `.cargo/config.toml` sets rust-lld with static CRT on Windows and the `xtask` and `workshop` aliases. `.config/nextest.toml` holds nextest profiles and the heavy STT test group. `.cursor/rules/` holds the Workshop architecture and SPA rules.
  - Root config: `Cargo.toml` (workspace, shared dependencies, lints), `clippy.toml`, `deny.toml`, `dist-workspace.toml` (cargo-dist), `rust-toolchain.toml` (stable), `rustfmt.toml` (style edition 2024), and `gateway.local.example.toml`. `local/`, `cabinet/`, and `target/` are untracked.
- Component boundaries:
  - Engine: `runvil` is the facade over all six `runvil-internal` crates. Inside, `types` and `vfs` are leaves, `model-client` depends on `types`, `lua` on `types`, `model-client`, and `vfs`, `parser` on `types` and `lua`, and `engine` on all five. Engine crates never depend on `tokio`, `tokio-util`, `async-trait`, `reqwest`, `getrandom`, or `fastrand`.
  - Pack API: `runvil-plugin` depends on `runvil-types` and `runvil-vfs` directly. The Packs `plugin-mcp`, `plugin-web`, and `plugin-user-input` depend only on `runvil-plugin` among workspace crates.
  - Harness: `harness-runner` depends on `runvil` and `runvil-plugin`. `harness` depends on those plus `harness-runner`. `harness-gateway-client` depends on `runvil`, `harness`, and `plugin-web`.
  - Gateway family: `gateway/app` composes the family crates and reaches outside it only through `gateway-api-types`, `gateway-api-discovery`, `shared-*`, and `build-ceiling`. In STT, `api` depends on `engine` and `backend-whisper`, `backend-whisper` on `engine` and `whisper-ffi`, and `whisper-ffi` owns the subsystem's only `unsafe` code.
  - Workshop: dependencies flow server to features to services to vocabulary (`workshop-protocol`), and same-tier crates meet only through `workshop-protocol` types and `workshop-registry` slots. `workshop-server` depends on `runvil`, `runvil-plugin`, `harness`, `harness-gateway-client`, the three Packs, and the Workshop crates. `workshop-server-api` is the facade over `workshop-server`. The desktop app depends on `workshop-server-api`, `gateway-api-discovery`, and `build-ceiling`, never on `workshop-server`. Workshop has no Cargo dependency on the gateway family.
  - Families reach each other only through top-level crates. `cargo test -p build-xtask` enforces the product and container boundaries, the Workshop tier graph, the `## Invariants` markers, and lint inheritance.
- Conventions summary:
  - Five defined terms (Engine, Harness, Host, Pack, Plugin) are capitalized and have one meaning everywhere. Engine crates say "the caller" and never mention the Host. `docs-claims.mjs` enforces the wording.
  - Lints are strict and inherited from the workspace: clippy `all` and `pedantic` deny, `unwrap_used` and `expect_used` deny, `allow_attributes` deny (suppressions use `#[expect(..., reason = "...")]`), `missing_docs` warn, and `unsafe_code` deny outside its owned boundary. The gates turn warnings into errors.
  - Source directories are flat: one or two related files sit beside the parent as `foo-bar.rs` wired by `#[path]`, and a group of three or more becomes a `foo/` subdirectory.
  - Every crate's `lib.rs` opens with a `//!` doc carrying a `## Invariants` section. Workshop `lib.rs` files are facades only.
  - Comments explain non-obvious constraints, and each workaround cites its upstream issue URL. `Cargo.toml` entries carry comments explaining pins and feature choices, and pre-1.0 or format-coupled dependencies are pinned exactly.
  - Error and status messages are written for model consumption, naming required versus actual.
  - Behavior changes ship with tests in the same change. JSON that reaches a recorder round-trips exactly with sorted keys and finite numbers.
  - Edition 2024, stable toolchain, and a 1.95 floor for the Engine crates, which CI's msrv job builds.
  - Commit subjects are imperative, sentence-case summaries such as "Rename the Engine's Plugin unit to Pack", and a finished plan gets a "Close plan: <slug>" commit.

</project-survey>
<execution-plan>

## Execution Instructions

<step-1>

### Step 1: Remove HostContext's dead services field [completed]

- Component: `none`
- Tests:
  - `cargo nextest run --locked -p harness-runner --all-features`
  - `cargo nextest run --locked -p harness --all-features`
  - `cargo nextest run --locked -p workshop-server`
  - `cargo fmt --all --check`
  - `$env:CARGO_BUILD_WARNINGS="deny"; cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features; Remove-Item Env:CARGO_BUILD_WARNINGS`
  - `$env:CARGO_BUILD_WARNINGS="deny"; cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets; Remove-Item Env:CARGO_BUILD_WARNINGS`
  - `$env:RUSTDOCFLAGS="-D warnings"; cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api; Remove-Item Env:RUSTDOCFLAGS`
  - `git grep -n "self.services" -- crates/harness-internal/runner/src/host.rs`
- Starting point: `runvil` at `8faa39a76`, clean worktree. This is C1 from the debt review of the Pack and Plugin rename. `61cea9eb6` removed `HostContext::install`, the field's last real reader, and left the field behind.
- Artifact: `crates/harness-internal/runner/src/host.rs` only. Make these edits:
  - `pub struct HostContext`: remove `services: HostServices,` (line 53), which leaves only `built: BTreeMap<&'static str, Built>`.
  - The struct doc (lines 46-47): change the first sentence from "Every Pack a Host built and the Host-wide services their `construct` functions read." to "Every Pack a Host built." Keep the rest of the doc as it is.
  - `HostContext::new(packs: PackRegistry, services: HostServices) -> HostContext`: keep the public signature and keep handing `&services` to each `construct` (line 91). Change the final `HostContext { services, built }` (line 103) to `HostContext { built }`. The `services` argument now drops when `new` returns.
  - `impl fmt::Debug for HostContext`: remove `.field("services", &self.services)` (line 234). Change the doc (lines 225-226) from "Shows the services and each built Pack's FQN and Pack id with whether it built, and never a Pack's config." to "Shows each built Pack's FQN and Pack id with whether it built, and never a Pack's config." Rewrap it to the file's comment width.
- Leave alone:
  - The `new` doc, including "They reach only `construct`; a tool call reads the run's own services instead.", which stays true.
  - The `HostServices` import, because `new` and `begin_run` still take it.
  - `HostContext::begin_run`'s `services` parameter (line 206) and `HostRunContext`'s own `services` field. These hold the run's own services and are live.
  - The module doc at line 18.
- Decisions, already made:
  - Remove the field outright, not just its `Debug` line. Keeping the field and only fixing its doc was rejected, because it leaves dead state that the lint cannot see.
  - Keep `HostContext::new`'s `services` parameter. Dropping it was rejected, because every `construct` still reads Host-wide services and the signature is public through `harness::pack`.
  - Add no alias or compatibility path for `plugins:` or the `plugins` Lua global.
- Non-goals: no change to behavior, public interfaces, wire or persisted formats, or `Debug` output beyond dropping the services field. `crates/runvil/public-api.txt` stays unchanged, because `harness-runner` is not part of the `runvil` facade. Touch none of the rejected or deferred candidates (D1-1 through D1-17, C2, C3).
- Testing:
  - Add no new test. C1 is dead code, not incorrect behavior, and the compiler checks the removal: any remaining reader of `HostContext.services` fails to build.
  - Change no existing test. The only `Debug` assertion, `let shown = format!("{host:?}")` in `crates/harness-internal/runner/src/host-tests.rs` (line 215), checks only FQNs and keeps passing.
  - The `workshop-server` run covers the real Plugins and runtime handle through `HostContext::new`, including `every_run_holds_the_packs_the_three_plugins_add_and_the_builtin` in `crates/workshop/server/src/agents/tests.rs`.
  - Risk this guards: a provider whose `Drop` has side effects now drops when `new` returns unless a Pack took its own `Arc`. None exists in the tree. `plugin-web` takes its provider and runtime `Arc`s at construct time, `plugin-mcp` takes the runtime handle, and Workshop provides only `tokio::runtime::Handle::try_current()`.
- Acceptance:
  - `HostContext` holds only `built`.
  - Every nextest, fmt, clippy, and rustdoc command above passes with no test changes.
  - The `git grep` prints nothing. Its exit code 1 with empty output counts as a pass.
- Commit: one commit in `runvil` containing only this change, with an imperative sentence-case subject such as "Remove HostContext's dead services field" and a `Plan:` trailer naming this plan's record under `vibe/`. Push nothing.

</step-1>

</execution-plan>