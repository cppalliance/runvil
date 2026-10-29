---
name: "Vibelight: runvil/crates/runvil-internal/model-client"
overview: Outline the model-client crate with vibelight, verify every slop candidate against the code and its docs, and leave a self-contained Cleanup Plan.
todos:
  - id: resolve
    content: Resolve repository root, vibelight.toml, and the outline index
    status: completed
  - id: outline
    content: Verify outline freshness; rerun vibelight because the crate moved past it
    status: completed
  - id: docs
    content: Collect doc roots that count as fact
    status: completed
  - id: analyze
    content: Dispatch analysis and drift subagents in parallel
    status: completed
  - id: verify
    content: Dispatch one verify subagent per findings file
    status: completed
  - id: plan
    content: Write the Cleanup Plan into the plan file and plan tool copy
    status: completed
  - id: refine
    content: Dispatch refiner and fold accepted changes back into both copies
    status: completed
isProject: false
---

# Vibelight: runvil/crates/runvil-internal/model-client

<product-contract>

## Product Requirements

### Scope

- **Crate:** `runvil-model-client` at `crates/runvil-internal/model-client`, outline 70 KB
- **Outlines:** current for the target crate; the tree moved to `9b4c21a39` and no crate file changed
- **Context:** 48 other outlines, not analyzed; the other crates' outlines are now stale
- **Excluded:** 7 crates, all build tooling or one test fixture
- **Doc roots:**
  - operator's own roots, none named, 2026-10-28
  - `crates/runvil-internal/model-client/README.md`
  - `README.md` and `AGENTS.md` at the repository root
  - `vibe/`, the repository's dated design and debt records
  - the `runvil-docs` sibling clone at `b6c0d90`, 33 markdown files
- **Limits:**
  - vibelight warned that 99 items are written more than one way
  - `guide/books/*/book.toml` name a `src` tree this repository lacks, so no book root
  - the `vibelight` on PATH is 0.2.0 and rejects this repository's `vibelight.toml`
  - `runvil-lua` and `runvil-engine` were rewritten after the scan, so their line numbers were rechecked by hand

Stats: 21 findings, all accepted, 18 cheap and 3 substantial, 2 candidates rejected; 1 finding already fixed; 20 remain in 9 work items; 0 operator decisions.

Goals: remove the duplicate state, the unreferenced constructors, and the doc comments that are untrue.
Non-goals: the mutex around `ModelSet`, the `models.use` merge rules, and any slop in sibling crates.
Success: every work item's proof command passes, and a fresh `vibelight` run shows the changes.

## Functional Specification

### Debt Inventory

State and duplication, one line each:

- `ModelBinding` stores the provider twice, and only convention keeps the copies equal (S1-1)
  - A catalog-only binding answers `provider() == None` while `render_info()` names the catalog's
  - Changes what a facade-visible getter reports, so it needs a test and an operator decision
- `CompletionOptions` redeclares three `ModelInvocation` settings with a second setter pair (S1-2)
- `ToolArguments` models a null payload its own doc forbids (S1-3)
  - The null arms are reachable only from a test literal that hand-builds a `ToolCall`

Vestiges and naming:

- `ModelView::default` shadows `Default::default`; `ModelSet` calls the same fact `default_alias` (S1-4)
- `ModelSet::from_parts` has eleven callers and every one is test code (S1-5)
- `TryFrom<f64> for Temperature` forwards to `new` and nothing anywhere calls it (S1-6)
- Five of the six `runvil_types` re-exports keep paths that nothing names resolving (S1-7)
- `CompletionOptions::model` is `pub(super)` while its three siblings are private (V1-1)
- Three wire fields stay crate-visible though only their own getters read them (V1-2)
- `bindings()` means a borrow on `ModelSet` and an owned clone on `ModelView` (V1-3)

Doc comments in the crate, grouped by file:

- `src/client/wire.rs`: `name` states a grammar the builder refuses, and `model` promises the response's name where decoding keeps the request's (S2-1, S2-2)
- `src/model.rs`: the module doc names `models.bind`, a global `runvil-lua` asserts is absent (S2-3)
- `src/model/options.rs`: `ModelView` claims an `Arc<dyn ModelView>` share and a structural freeze, and `ModelSet` says bindings come from live H1 (S2-4, V2-1)
- `src/model/error.rs`: the message-shape rule is told three times in three wordings, and the builder list omits the third builder (S1-8, S2-7)
- `src/detail.rs`: the module doc enumerates four of its six functions (S2-9)

Documentation drift in the guide, one line each:

- `11-conversations.md:1217` lists `thinking` as a `models.use` option; the parser takes two keys (S2-5)
- `10-models.md:1453` says infer metrics hold only provider figures; the client adds its own (S2-8)

Fixed since the scan: the guide's error-kind table gained its `InvalidParts` row, so that finding is done (S2-6).

Rejected: 2 candidates, both weak, because the wording they question misleads no reader.

</product-contract>
<implementation-contract>

## Technical Design

- **The provider read rule.** The catalog already carries a provider, so the field becomes the fallback a catalog-less binding needs rather than a second answer. One expression serves both the getter and the render, so they cannot disagree. Cost: `provider()` now reports a catalog's provider for a binding that never called `with_provider`.
- **One invocation inside `CompletionOptions`.** Holding `ModelInvocation` removes the hand copy in `completion_options` and the second setter family, while the four getters and four setters keep their exact signatures. `with_invocation` and `invocation()` are the two additions, because a test cannot otherwise observe what the setters stored. Cost: `with_temperature` still takes `f64` and validates, so the crate keeps two spellings of temperature validation by design.
- **Arguments as a map, not a value.** Storing `Map` in the field retires the null state instead of documenting it away, and the five view branches collapse to map calls. The validating constructor cannot unwrap, because the workspace denies `unwrap_used` and `expect_used`, so the object check returns the map it proved: `normalize::checked_call_arguments` takes the `Value` and hands back a `Map`, keeping today's refusal message word for word.
- **`to_json_string` has no infallible spelling to borrow.** `Value::to_string` is infallible, and `serde_json::to_string(&Map)` returns a `Result` even though a `Value` tree cannot hold a number that fails to serialize. The method matches on that result and falls back to `{}`, which is unreachable for a `Value`-built map and is not the empty-string failure `lua/src/projection.rs:307-311` warns about.

</implementation-contract>
<verification-contract>

## Testing Plan

Gate commands from the repository root, once each:

- `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
- `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`
- `cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
- `cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`
- `cargo fmt --all --check`
- `cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`, with `RUSTDOCFLAGS="-D warnings"`
- `RUSTDOCFLAGS="-D warnings" cargo doc -p runvil --no-deps`, which builds the facade's docs without `--all-features`
- `cargo +nightly-2026-09-05 xtask api --check`, with `CARGO_BUILD_WARNINGS` unset
- `cargo test -p build-xtask`
- `node --test crates/workshop/ui/test/docs-claims.mjs`
- `vibelight` at the repository root, then read the rewritten outlines in `.vibelight/`

New tests, both in `crates/runvil-internal/model-client/src/model/options-tests.rs`, which already imports `ModelInfo` and `ThinkingMode` from `super::*`:

- `provider_prefers_the_catalog_and_falls_back_to_the_field`: a binding built with `with_catalog_info` alone reports the catalog's provider, and one built with `with_provider` alone still reports its own.
  - Fails today: the catalog-only case answers `None`.
  - Both halves are required, because `a_binding_built_with_new_renders_a_never_thinking_info_from_its_own_parts` covers only `render_info` and never calls `provider()`.
- `completion_options_reports_the_invocation_it_holds`: after `with_temperature(0.2)` and `with_thinking(true)`, `invocation().temperature` and `invocation().thinking` hold those values and the getters report the same.
  - Pins the delegation the rewrite introduces; the two cannot drift once one field holds both.

Existing tests that already pin the behavior and must pass unedited:

- `model/tests.rs` read-back and `with_model` tests, `options-tests.rs` temperature range, `client/tests.rs` name validation.
- `error-tests.rs` `each_kind_has_its_fixed_phrase`, `a_phrased_failure_shows_only_the_fixed_phrase`, and `a_specific_extends_the_phrase_after_a_colon_and_never_adds_detail`.
- `normalize-tests.rs` `non_object_arguments_are_rejected`, which pins the refusal message the map rewrite must keep.
- `read-tests.rs` around line 170 pins that a decoded completion is labeled `routed-to`, the requested name, across a table of body models.
- `models-tests.rs` `the_models_namespace_has_no_bind` at line 73 and `models_use_rejects_invalid_options_without_selecting` at line 160, which pins the two-key rejection message at line 196.

</verification-contract>
<decision-record>

## Decision Record

- The guide edits land in the `runvil-docs` clone, which is its own repository and needs its own commit (S2-5, S2-8)
- The `InvalidParts` table row landed in `runvil-docs` commit `4f537d5` after the scan, so nothing is left to do for it (S2-6)
- The `models.bind` clause was logged as deferred by the 2026-10-26 engine pass, which said to revisit it in its own pass; that record is now closed and this is that pass (S2-3)
- Deleting `TryFrom<f64>` and the five re-exports narrows a facade-pinned surface, and Item 2 adds two methods to a published type, so `crates/runvil/public-api.txt` is re-blessed in both items; every crate here is `publish = false`, so each change is a bless and reverts in one line
- Rejected: rewriting both `Completion::model` sentences, because the first states the `with_model` rule correctly (S2-2)
- Rejected: naming all six `detail` functions, because a rule survives a seventh function and a list does not (S2-9)
- Rejected: keeping `ModelView::bindings` as a named seam for a Host-backed view, because no Engine path calls it and re-adding it is one signature (V1-3)
- Risk: the map rewrite must keep the refusal message, the crate-visible field that `detail.rs` reads, and the absence of `unwrap` and `expect`, which the workspace denies
- Rechecked on 2026-10-29 against `9b4c21a39`: the target crate is byte-identical since the scan, but `runvil-lua` and `runvil-engine` were rewritten in twelve commits, so every cross-crate citation here was re-derived by hand rather than trusted
- The engine record that deferred the `models.bind` clause is closed (`4eb444630`), and `runvil-docs` gained two commits, one of which fixed the guide's error-kind table outright

- Step granularity: keep the step count low, favoring fewer, larger commits over many small ones, so a reviewer can hold the whole run in a few commits (operator instruction, 2026-10-29)

### Open Questions

None. The one open question is settled: `ModelBinding::provider()` returns the catalog's provider, falling back to its own field, and `render_info` takes its provider from that same expression (option A). The catalog is the authority because the field's own doc already says its value comes from the catalog, and one expression for both readers makes the two unable to disagree. The Engine's redundant `with_provider` call goes.

### Deferred and Out of Scope

- The mutex around `ModelSet`, which exists for the one writer `models.default`; removing it needs `runvil-lua` to change too
- `models.use` option precedence, documented only where the merge happens and agreeing with the guide
- The `ToolArguments` "canonical JSON" wording, whose ordering the workspace already pins through `serde_json`
- The wire-name grammar stated at four guide sites, `12-tools.md:298`, `:1701`, `:1755`, `:2224`; fix them in one guide pass
- `ToolSchema`'s three `pub(crate)` fields, which stay crate-visible because `detail::tool_schema_new` builds the type from outside `client`; narrowing them needs the construction moved behind a `pub(crate)` constructor in `client::wire`
- Any slop in `runvil-lua`, `runvil-engine`, or the harness crates, which this pass did not analyze

</decision-record>
<project-survey>

## Project Survey

- Status: complete
- Build command: `cargo build --locked -p gateway`. `default-members` in the root `Cargo.toml` is `crates/gateway/app`, so a bare `cargo build` builds only the gateway. The desktop app is explicit: `cargo build -p workshop`.
- Focused test command pattern: `cargo nextest run --locked -p <crate> -E 'test(<substring>)'`, for example `cargo nextest run --locked -p runvil-model-client -E 'test(provider_prefers)'`. A bare trailing filter also works: `cargo nextest run --locked -p runvil-model-client provider`. `cargo-nextest 0.9.128` is installed and `cargo nextest list -p runvil-model-client --all-features` verifies against module paths such as `client::tests::<name>`.
- Component test command pattern: `cargo nextest run --locked -p <package>`. Engine work such as this plan narrows to `cargo nextest run --locked -p runvil-model-client -p runvil-engine -p runvil-lua`, and a change in an internal Engine crate also runs the facade suite `cargo nextest run --locked -p runvil --all-features`. Add `--all-features` for Engine, plugin, and Harness crates; drop it for `workshop`, `workshop-server`, and `workshop-server-api`.
- Full-suite test command: `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`, then `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`, then the Node suites and the structural checks below.
- Linter command: `CARGO_BUILD_WARNINGS=deny cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`, plus `CARGO_BUILD_WARNINGS=deny cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets` for the Workshop crates. Never run a standalone `cargo check --workspace` beside these; the one permitted exception is `cargo check -p gateway --no-default-features`, a headless build shape clippy `--all-features` does not cover. The TypeScript packages have no linter config at all, so the JS gate is `npm run typecheck --workspaces --if-present` (`tsc --noEmit`) run from `crates/workshop` and `npm run typecheck` from `crates/gateway/config-ui/ui`, beside `npm test --workspaces --if-present`.
- Formatter check command: `cargo fmt --all --check`. Verified clean at survey time.
- Docs command: `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`, plus the facade gate `RUSTDOCFLAGS="-D warnings" cargo doc -p runvil --no-deps` without `--all-features`, and for Engine work `RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`. Rustdoc lints are not covered by clippy. The guide site has its own build, `cargo xtask site`, which needs the `MDBOOK` environment variable naming an mdBook binary.
- Test placement and naming conventions: unit tests live beside their module as a `#[cfg(test)] mod tests;` declaration pointing at a flat kebab sibling through an explicit path attribute, as `src/normalize.rs` does with `#[path = "normalize-tests.rs"] mod tests;` and `src/model/options.rs` does with `#[path = "options-tests.rs"]`. A test group with three or more files becomes a `tests/` subdirectory instead. Integration suites go under the crate's `tests/` as a `main.rs` plus named module files, as `crates/harness/tests/suite/main.rs` wires `mod asker; mod broker; mod host; mod launch; mod support; mod vfs;`. Prompt fixtures are Markdown files under `tests/prompts/valid`, `tests/prompts/invalid`, and `tests/prompts/execution`. Test function names are snake_case full sentences naming the behavior and its expectation, with no `test_` prefix and no numbering: `provider_prefers_the_catalog_and_falls_back_to_the_field`, `a_role_round_trips_through_serde_as_its_lowercase_name`. Assertion messages explain why the expectation holds.
- Directory map: `crates/` holds every Cargo package. `crates/runvil` is the facade and `crates/runvil-internal/{engine,lua,model-client,parser,types,vfs}` the Engine crates behind it. `crates/harness`, `crates/harness-gateway-client`, and `crates/runvil-plugin` are the Harness and the Plugin contract, with `crates/harness-internal/{runner,plugin-loader,test-plugin}` nested beneath. `crates/plugin-{std,web,user-input,mcp}` are the shipped Plugins. `crates/gateway/*` is the gateway family, with `stt/` nesting `api`, `engine`, `backend-whisper`, and `whisper-ffi` one level deeper. `crates/workshop/*` is the Workshop Host family, `crates/workshop/ui` is its TypeScript SPA, and `crates/shared-ui` is the TypeScript+CSS package the gateway config UI consumes. `crates/build-*` are build tooling crates: `build-xtask` (aliased `xtask`) runs the API-surface and site checks, `build-ceiling` enforces compile-time file size, `build-ui`, `build-user-guide`, `build-workshop` (aliased `workshop`), and `build-llama-cuda`. Outside `crates/`, `guide/books/{gateway,language,workshop}` are the mdBook sources, `prompts/` holds sample prompt files, `tools/` holds Node scripts, `local/` is untracked scratch, `vibe/` holds dated plan records, `images/` is documentation art, and `.vibelight/` is generated HTML outlines excluded from analysis through `vibelight.toml`.
- Component boundaries: the Engine is `runvil` plus `runvil-*`, the Harness is `harness` and `harness-*`, and a Host runs prompts only through the Harness. `runvil-model-client` may depend on `runvil-types` and nothing else internal, and its `lib.rs` states so under `## Invariants`. `runvil-engine` sits above `runvil-{lua,model-client,parser,types,vfs}`, and the `runvil` facade sits above all six and re-exports each public item from its defining crate. `harness` depends downward on `runvil`, `runvil-plugin`, `harness-runner`, and `harness-plugin-loader`. `crates/gateway/app` depends only on `gateway-*`, `shared-*`, and platform crates and never on the Engine. Workshop tiers run vocabulary, then services, then features, then `workshop-server`, enforced by `cargo test -p build-xtask`. Dependency cycles are a design error, and Engine crates may never reach `tokio`, `tokio-util`, `async-trait`, `reqwest`, `getrandom`, or `fastrand`, directly or transitively.
- Conventions summary: every crate declares `[lints] workspace = true`, which denies `clippy::all`, `clippy::pedantic`, `unwrap_used`, `expect_used`, `todo`, `allow_attributes`, `disallowed_methods`, and `unsafe_code`, and warns on `missing_docs`. A suppression is written `#[expect(..., reason = "...")]`, never `#[allow]`. `allow-unwrap-in-tests` and `allow-expect-in-tests` in `clippy.toml` make `unwrap` and `expect` legal only in tests. Source directories are flat by default and a subdirectory needs three or more files, so a two-file test pair stays a kebab sibling with an explicit `#[path]` attribute. Every module and public item carries a `//!` or `///` doc comment written in prose, and Engine crate docs name "the caller" and never a Host application. Commit hooks run `cargo fmt --all --check` on commit and the wording scan, the headless gateway check, clippy, and `cargo deny check` on push. Behavior changes ship with tests in the same change.

</project-survey>
<execution-plan>

## Execution Instructions

The letters and numbers in parentheses, such as (S1-1), are tags from the analysis and are never needed to act.

Three components, three steps, one commit each. No step spans two components. The four-step cap folds several work items into one commit wherever they touch the same crate and share a cause; each step names the items it carries, and the coverage line below accounts for all nine. No step owns the plan's seed copy, `vibe/ACTIVE`, or the close commit, because the Run Sequence owns them.

### Component order and why

- `model-client-wire` first. It changes the type of `ToolCall::arguments` and the return type of `detail::tool_call_arguments`, and `runvil-engine` consumes both, so the cross-crate signature settles before model-side work starts.
- `model-client-model` second. It renames and deletes `ModelView` members that `runvil-lua` and `runvil-engine` call, and its single re-bless must see the final method set.
- `runvil-docs-guide` last. It is a separate repository needing its own commit, its site build runs from the `runvil` root, and nothing in it depends on the Rust edits.

Coverage: Step 1 carries Items 4, 5, and 8. Step 2 carries Items 1, 2, 3, 6, and 7. Step 3 carries Item 9.

<step-1>

### Step 1: store tool arguments as a map and narrow the wire fields [completed]

- Component: `model-client-wire`
- Tests: from the repository root, in order:
  - `cargo nextest run --locked -p runvil-model-client -p harness-gateway-client -p runvil-engine -p runvil-lua`
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`

Carries Item 4 (S1-3), Item 5 (V1-2, S2-1, S2-2), and Item 8 (S2-9): the arguments type change, the cheap visibility and wording sweep beside it, and the `detail` module doc rule. They share one commit because Items 4 and 5 rewrite the same `ToolCall`, `ToolArguments`, and `ArgumentNames` code, and Item 8's replacement rule has to name what `detail.rs` still holds after Item 4.

Two pieces, built sequentially in this one step: argument storage first, then visibility and doc wording. Sequential because the narrowed fields and their doc sentences must describe the final `Map` and `String` types, so writing them before the type change would mean writing them twice.

**Piece 1, argument storage.**

- `src/normalize.rs:40-46`: replace `check_call_arguments(arguments: &Value) -> Result<()>` with `pub(crate) fn checked_call_arguments(arguments: Value) -> Result<serde_json::Map<String, Value>>`. It takes the `Value` by value and returns the map it proved. The refusal keeps the message `tool call arguments were not a JSON object` word for word, so `normalize-tests.rs:40-53` (`non_object_arguments_are_rejected`) passes unedited. The constructor cannot `unwrap` or `expect` because the workspace denies both, so match `Value::Object(map)` and return `Ok(map)` on that arm. No clone is added anywhere.
- `src/client/wire-canned.rs:143`: the `check_call_arguments(&arguments)?;` statement becomes `arguments: checked_call_arguments(arguments)?` inside the `ToolCall` literal at line 144, and update the import list at line 17. Drop the now-unused local if it served only that call.
- `src/client/wire.rs:180`: `ToolCall::arguments` becomes `pub(crate) arguments: serde_json::Map<String, Value>`.
- `ToolArguments` at `src/client/wire.rs:213-217` holds `map: &'a serde_json::Map<String, Value>` and keeps `#[derive(Debug, Clone, Copy)]`. Then `to_json_string` (lines 221-223) becomes `serde_json::to_string(self.map).unwrap_or_else(|_| "{}".to_owned())` with a comment above it saying a `Value`-built map always serializes, so this arm is unreachable. Do not use `unwrap_or_default`, which would publish an empty arguments string, the failure `lua/src/projection.rs:307-311` warns about. `is_empty` becomes `self.map.is_empty()`, `contains` becomes `self.map.contains_key(key)`, and `names` becomes `ArgumentNames { keys: self.map.keys() }`.
- `ArgumentNames` at `src/client/wire.rs:254-260` holds `keys: serde_json::map::Keys<'a>`, and its field doc loses the `None`-for-a-null-payload sentence. `next` becomes `self.keys.next().map(String::as_str)` and `size_hint` becomes `self.keys.size_hint()`.
- `src/detail.rs:97-99`: `tool_call_arguments` returns an owned `Value` built as `Value::Object(call.arguments.clone())`. Its one caller, `crates/runvil-internal/engine/src/execute/scheduler/chat.rs:426`, passes `detail::tool_call_arguments(call).clone()` today; that `.clone()` becomes a move and must be dropped, because `ToolCallEvent::new`'s third parameter is already `serde_json::Value` (`crates/runvil-internal/types/src/metrics.rs:51`).
- `src/client/tests.rs:89-93`: build the call through `ToolCall::from_parts("call_1", "search", json!({"query": "rust", "limit": 5}))` instead of a struct literal. Delete the null half of `tool_arguments_view_exposes_no_raw_value` at lines 105-112, the `Value::Null` literal and its three assertions, because that state no longer exists. Keep the `null arguments` row at line 122 inside `from_parts_refuses_a_blank_id_a_blank_name_and_non_object_arguments`.
- Readers that need no edit: `src/detail.rs:98`, `src/client/wire.rs:203`, and the construction site in `wire-canned.rs`, which is a child module of `wire` and can reach the field. Every other `arguments` hit in `runvil-lua` is a `ToolCallRecord` or `ToolCallEvent` field of a different type, untouched here. Nothing outside the crate names the field, because it stays `pub(crate)`.

**Piece 2, visibility and doc wording.**

- Narrow three fields to private: `Completion.metadata_diagnostics` at `src/client/wire.rs:354`, and `ToolCall`'s `id` and `name` at lines 174 and 176. The first is `pub(crate)` today and the other two are `pub(super)`. Each is read only through its own getter, and the in-crate tests already read them that way.
- Confirm the visibility rule before editing, because `pub(crate)` and private are not interchangeable. Rust narrows a field only to the module that declares it, so private is reachable only inside `client::wire`. `wire-canned.rs` is that module's `#[path]` sibling, so its `Completion` literal at lines 51-57 and its `ToolCall` literal at line 144 still compile with no edit.
- Leave `Completion::result` (line 341) and `ToolCall::arguments` (line 180) at `pub(crate)`, because `src/detail.rs` reads both directly. Keep `ToolSchema`'s `name`, `description`, and `parameters` at lines 108-112 at `pub(crate)` too: `detail::tool_schema_new` at `src/detail.rs:87` builds them from outside `client`, so private there would not compile. This is why this step narrows three fields and not six.
- `ToolSchema::name`'s doc at lines 117-118 says the name "holds one or more characters, all from `[A-Za-z0-9_.-]`", but `detail::tool_schema_new` at `src/detail.rs:74-79` requires a leading ASCII letter, then only letters, digits, `_`, and `-`, capped at 64 bytes. Replace that second sentence with: "Returns the tool's name as sent to the model. The `crate::detail::tool_schema_new` builder is the only one, and it accepts a leading ASCII letter followed by up to 63 letters, digits, `_`, or `-`." Note that the name is written without square brackets: `ToolSchema::name` is facade-visible, and `xtask api --check` rejects a doc link from a facade-visible item into a crate the facade does not re-export, so a bracketed link there fails the gate.
- `Completion::model`'s doc at lines 393-395 ends with "A completion decoded from a response is built with the model the response body named, or an empty name when the body omits one." `crates/harness-gateway-client/src/wire/stream.rs:361-363` overwrites that with the requested model whenever the request body named one. Replace only that final sentence with: "A completion decoded from a response is labeled with the model the request body named, or, when the request body named none, with the model the response body named." Leave the preceding `with_model` sentence untouched; it is correct.

**Piece 3, the `detail` module rule.**

- Replace the four-item enumeration in the module doc at `src/detail.rs:3-8` with the rule: "Each function reaches a field or a construction step the facade-visible type keeps private. [`completion_into_result`] is the one that splits a completion into the result the tool loop consumes and the metadata the round reports." The enumeration names four of six functions and omits `completion_into_result` at line 103, which `engine/src/execute/turn.rs:107` uses to dismantle a whole completion.
- Keep the two accurate claims that sit around it: the `runvil` facade never re-exports this module (`crates/runvil/src/lib.rs:71-99` re-exports from `client` and `model` only), and only Engine crates reach it.

Acceptance: the refusal text `tool call arguments were not a JSON object` still fails `non_object_arguments_are_rejected`'s cases, so `normalize-tests.rs:40-53` must pass unedited; `from_parts` still refuses a null payload at `src/client/tests.rs:122`; no `unwrap` or `expect` is added outside tests; and the docs gate passes with `broken_intra_doc_links` denied, so any doc link into a removed item is repointed in this step.

</step-1>

<step-2>

### Step 2: one provider, one invocation, and the model module's untrue docs [completed]

- Component: `model-client-model`
- Tests: from the repository root, in order:
  - `cargo nextest run --locked -p runvil-model-client -p runvil-engine -p runvil-lua`
  - `cargo nextest run --locked -p runvil-model-client -p harness-gateway-client -p workshop-server`
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`
  - `cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`
  - `cargo fmt --all --check`
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`
  - `RUSTDOCFLAGS="-D warnings" cargo doc -p runvil --no-deps`
  - `cargo +nightly-2026-09-05 xtask api --check` with `CARGO_BUILD_WARNINGS` unset
  - `cargo test -p build-xtask`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`
  - `vibelight` at the repository root, then read the rewritten outlines in `.vibelight/`. The `vibelight` on PATH is 0.2.0 and rejects this repository's `vibelight.toml`, which the plan records under Limits, so a failure there is pre-existing: report it and do not chase it.

Carries Item 1 (S1-1), Item 2 (S1-2), Item 3 (S1-4, S1-5, S1-6, V1-1, V1-3, S2-4, V2-1), Item 6 (S1-8, S2-7), and Item 7 (S1-7, S2-3). One commit, because all five items edit the model module for one cause, the module states duplicate facts in code and in prose that the code contradicts, and because Items 2 and 3 both move the public listing, so one blessing has to cover the net change.

Two pieces, built sequentially: options state first (Items 1, 2, 3), then the untrue docs (Items 6, 7). Sequential because Item 3 rewrites the `ModelView` and `ModelSet` docs that sit beside the members it renames, and the blessing must run after every API change in the item.

New tests, both in `src/model/options-tests.rs`, which already imports `ModelInfo` and `ThinkingMode` from `super::*`:

- `provider_prefers_the_catalog_and_falls_back_to_the_field`: build one binding with `with_catalog_info` over a `ModelInfo` whose provider is `"acme"` and no `with_provider` call, and assert `provider() == Some("acme")`. That half fails today, because `provider()` at `src/model/options.rs:232` reads only the field. Build a second binding with `with_provider("acme")` and no catalog and assert the same value; that half passes today and must keep passing, and it is the half `options-tests.rs` already protects at lines 79-93 for `render_info` only. Both halves are required, because `a_binding_built_with_new_renders_a_never_thinking_info_from_its_own_parts` covers only `render_info` and never calls `provider()`.
- `completion_options_reports_the_invocation_it_holds`: after `with_temperature(0.2)` and `with_thinking(true)`, `invocation().temperature` and `invocation().thinking` hold those values and `temperature()` and `thinking()` report the same. This pins the delegation the rewrite introduces, so the two cannot drift once one field holds both.

**Item 1, one provider.** `ModelBinding` stores the provider twice, in `provider: Option<String>` at `src/model/options.rs:133` and in `catalog: Option<ModelInfo>` at line 136, and `with_provider` (line 166) and `with_catalog_info` (line 174) are independent setters.

- Change `provider()` at line 232 to return `self.catalog.as_ref().and_then(ModelInfo::provider).or(self.provider.as_deref())`. One expression now serves both readers, so they cannot disagree. Accept the stated cost: `provider()` reports a catalog's provider for a binding that never called `with_provider`.
- Change `render_info`'s tail so its fallback takes the provider from that same expression: replace the `match &self.provider` block at lines 252-255 with `match self.provider() { Some(provider) => info.with_provider(provider.to_owned()), None => info }`.
- Reword `with_provider`'s doc at line 164 to: "Sets the provider id for a binding built without catalog info. A held catalog's provider wins."
- In `crates/runvil-internal/engine/src/execute/context-bound.rs`, delete the now-redundant `if let Some(provider) = info.provider()` block at lines 77-79, because `with_catalog_info(info.clone())` at line 76 already carries that provider. That removal also retires `let mut binding` at line 62 in favor of `let binding`, since nothing reassigns it afterward. Leave `engine/src/execute/context-tests.rs:168` alone; it exercises the fallback through `RunContext`.
- No edit in Lua: `crates/runvil-internal/lua/src/models-userdata.rs:78` and `:123` read `binding.provider()` and keep working. `engine/src/execute/tests/model_and_reply-handles.rs:177` pins the handle's provider as the catalog's through the engine path and must still pass.

**Item 2, one invocation.** `CompletionOptions` declares `temperature`, `max_tokens`, and `thinking` as its own fields at `src/model/options.rs:280-284`, duplicating `ModelInvocation`'s fields at lines 78-87, and `ModelBinding::completion_options` at lines 261-268 copies the three by hand.

- Replace the three fields with a single private `invocation: ModelInvocation`. Keep `model: String`. Item 3 makes it private, and it is private by the end of this step.
- Keep `model()`, `temperature()`, `max_tokens()`, and `thinking()` as getters that read through the held invocation, and keep `with_temperature`, `with_max_tokens`, and `with_thinking` with their exact current signatures, delegating to that field. `with_temperature` still takes `f64` and returns `Result<CompletionOptions, TemperatureError>`, so the crate keeps two spellings of temperature validation by design.
- Add `pub fn with_invocation(mut self, invocation: ModelInvocation) -> Self` and `pub fn invocation(&self) -> &ModelInvocation`. The second is not optional bookkeeping: the new test reads the field it pins, and `CompletionOptions` is `#[non_exhaustive]` with private fields, so no test can reach the invocation otherwise.
- Rewrite `CompletionOptions::new` at lines 291-297 to build an empty `ModelInvocation`, and `ModelBinding::completion_options` to `CompletionOptions::new(self.id.name()).with_invocation(self.invocation.clone())`.
- Nothing outside the crate changes. `crates/harness-gateway-client/src/wire/request.rs:39,61,64,67` reads the four getters, `crates/workshop/server/src/agents/broker.rs:99` and `:114` use `model()` and `with_model()`, and all five `binding.completion_options()` callers keep their signature: `harness-gateway-client/src/wire/request-tests.rs:101`, `model-client/src/model/tests.rs:77`, `engine/src/test_support/scenario-driving.rs:72`, `harness-internal/runner/src/effect_loop.rs:347`, and `engine/benches/models_loop.rs:105`. The benches compile only under the all-targets clippy gate.

**Item 3, the cheap sweep.** Five deletions and renames, then two doc blocks.

- Rename the `ModelView` trait method `default` at line 445 to `default_alias`, matching `ModelSet::default_alias` at line 401, so the name no longer collides with the `Default` trait that both `ModelInvocation` (line 72) and `ModelSet` (line 369) derive. Change the `impl ModelView for Mutex<ModelSet>` arm at line 468 to call `ModelSet::default_alias`. All three call sites move in this step: `src/model/options-tests.rs:24` and `:33`, which spell the call as `ModelView::default_alias(&set)` so the trait method stays the one under test, and `crates/runvil-internal/lua/src/vm/state.rs:271`, where `bindings.default()?` on a `dyn ModelView` becomes `bindings.default_alias()?`.
- Delete `ModelSet::from_parts` at lines 379-384. All eleven callers are test fixtures or assertions: `engine/src/model/tests.rs:61`, `engine/src/lua/tests.rs:42`, `engine/src/lua/tests/globals.rs:33`, `engine/src/execute/tests/models_loop.rs:32`, `models_loop-author-shapes.rs:229`, `precheck_anchor.rs:313`, `rendered_tools.rs:244`, `scheduler.rs:25`, `scheduler/failures-script-tools.rs:234`, `lua/src/models-tests.rs:62`, and `model-client/src/model/tests.rs:101`. Rebuild each with `ModelSet::default()`, `push` for each binding, and `set_default_alias` where the fixture named a default. `model-client/src/model/tests.rs:101` is an equality assertion rather than a fixture, so compare against a second set built that way; `a_model_set_grows_by_push_and_takes_its_default_alias_by_setter` at line 83 already asserts that `push` plus `set_default_alias` produces what `from_parts` did, which is the equivalence the deletion relies on.
- Delete the `TryFrom<f64> for Temperature` impl at lines 45-51, leaving `Temperature::new` as the only way in. `lua/src/models.rs:110` decodes a temperature through `Temperature::new`, not the impl, so Lua behavior does not move, and `options-tests.rs:98-141` already pins the surviving constructor's range from both ends.
- Delete `bindings()` from the `ModelView` trait at lines 439-441 and from the impl at lines 464-466, leaving `binding()` and `default_alias()` as the trait's reads. Update `engine/src/error/tests.rs:324`, which calls `set.bindings()` on an `Arc<Mutex<ModelSet>>`, to reach the same lock failure through `set.binding("writer").expect_err("a poisoned set cannot be read")`, because the trait's `binding` still returns `Err(ModelSetError)` on a poisoned lock. Update `src/model/options-tests.rs:23` and `:32`, because `Mutex<ModelSet>` has no `bindings` of its own once the trait arm is gone: line 23 becomes `assert!(set.lock().is_err(), "a poisoned set cannot be read")` and line 32 becomes `assert!(set.lock().expect("a healthy set locks").bindings().is_empty())`. The `bindings()` calls at `src/model/tests.rs:85` and `:91` and `engine/src/execute/tests/precheck_anchor.rs:187` reach the inherent `ModelSet` method and need no edit.
- Make `CompletionOptions::model` private at line 278, and read it through `model()` at `src/model/tests.rs:78`, which asserts the field directly as `opts.model` today.
- Reword `ModelSet`'s type doc at lines 366-367: it says "the prompt-level bindings produced by live H1 execution plus the prompt-wide `default` alias"; replace "produced by live H1 execution" with "built at run construction from prepare's role bindings".
- Reword `ModelView`'s doc at lines 429-432. It claims the context shares the set "as `Arc<dyn ModelView>`", that "once that VM is dropped no write handle remains", and that "post-H1 frozenness is structural", none of which holds: the context holds `Arc<Mutex<ModelSet>>` at `engine/src/execute/context.rs:100`, every section VM gets a write-capable clone (`section_context-construct.rs:85`, `:170`, and `context.rs:285`), and `models.default` works from any section, as `lua/src/models.rs:228-237` says. Replace those four sentences with this paragraph, which already ends with the existing mutex-guard sentence, so nothing is duplicated: "The run holds one `ModelSet` behind a mutex and hands every section VM a handle to it. This trait exposes reads only, so a reader cannot change the set through it. The one write any Engine path performs is `models.default`, which sets the default once and refuses a second, different label. Every method locks briefly and returns an owned snapshot: a mutex guard cannot outlive the call."

**Item 6, the message rule stated once.** The rule is told three times, on `phrase` at `src/model/error.rs:52-64`, on `CompletionError` at lines 143-151, and on `new` at lines 164-175, and the three copies already differ in small ways.

- Add the rule once to the module doc at lines 1-2: "The message is the kind's fixed phrase. A failure built from an HTTP status appends ` (status N)`, and a 401 or 403 is `Unavailable` with `the model backend did not accept the credentials`. Only `MalformedResponse`, `EmptyReply`, `Unavailable`, and `InvalidParts` extend it with `: ` and text this repository's own code wrote. Provider text never enters the message; it goes in `detail`."
- Cut the three blocks to pointers. `phrase`'s doc becomes: "Returns the fixed phrase for this kind: one lowercase phrase written for a model to read. The module page gives the message shapes a caller builds from it." `CompletionError`'s doc keeps its field list and its "`Display` shows only the message" sentence, and replaces its four-sentence restatement with "See the module page for the message shapes." `new`'s doc becomes: "Builds a failure of `kind` with `message` as its display text. The caller passes the kind's fixed `phrase`; the extension rules live on the module page. Provider text goes to `with_detail`."
- Fix the builder list at lines 139-140, which names `new` and `context_overflow` where the public surface has three: `empty_reply` at line 199 is public and published at `crates/runvil/public-api.txt:574`. Replace the sentence with: "The caller builds one with [`new`](CompletionError::new), [`empty_reply`](CompletionError::empty_reply), or [`context_overflow`](CompletionError::context_overflow) and adds the extras with the `with_` methods. `empty_reply` is the builder that chooses between the bare phrase and the phrase extended with the reasoning-ignored text." `error-tests.rs:245` (`the_shared_empty_reply_owner_builds_the_phrase_and_the_reasoning_specific`) already pins that builder and must pass unedited.

**Item 7, five re-exports and the `models.bind` clause.**

- Replace the block at `src/model.rs:23-25` with `pub use runvil_types::models::ModelId;` plus the comment "`runvil-lua` names the model id through this path; the rest of the catalog vocabulary is imported from `runvil-types` directly." Only `ModelId` is ever named through that path: `lua/src/models.rs:17`, `lua/src/models_loop/tests.rs:9`, `lua/src/protocol/tests.rs:15`, and `lua/src/models-tests.rs:6`. The other five are imported from `runvil_types::models` directly at every use, for example `engine/src/model.rs:47`, and nothing in this crate imports them through `crate::model`.
- Repoint four doc links into the deleted block in the same change, because `broken_intra_doc_links` is denied: `src/lib.rs:9`, `:19`, and `:20`, and the unqualified `ModelCatalog` link at `src/model.rs:3`. Point them at `runvil_types::models::*`. The facade re-exports all six from `runvil_types` at `crates/runvil/src/lib.rs:93-98`, so those doc paths keep resolving at the facade.
- Fix the module doc's opening at `src/model.rs:4-8`: it says "H1 `models.bind` resolves a description against that catalog under hard constraints, freezes invocation parameters, and stores the result in the Engine's run-scoped model bindings", but no such global exists. `lua/src/models.rs:234-235` says "There is no `models.bind`: binding is the frontmatter's", and `the_models_namespace_has_no_bind` at `lua/src/models-tests.rs:73` asserts `models.bind == nil`. Replace that clause with: "The prompt's frontmatter declares each model role, prepare resolves every role against that catalog under the role's hard keywords and context minimum, and the run builds its binding set from those bindings before any Lua runs." Leave the `models.use` clause untouched, but leave nothing wrong beside it: the same doc assigns `models.default` to H1 at line 7, so reword that to "`models.default` supplies the prompt-wide default, conventionally from H1", matching `lua/src/models.rs:230-233`.

**Blessing, once for both API-moving items.** Run `cargo +nightly-2026-09-05 xtask api --bless` from the repository root with `CARGO_BUILD_WARNINGS` unset, after Item 2 and Item 3 and before committing. The expected listing difference is four lines: the two new `CompletionOptions` methods added to the nine-line block at `crates/runvil/public-api.txt:586-594`, and the two `TryFrom<f64> for Temperature` lines removed at 354-355. All nine surviving `CompletionOptions` signatures keep their exact text. If the five re-export deletions also remove listing lines, name them in the commit message, because the facade re-exports all six names from `runvil_types` and should keep every facade path. Any other change means a signature moved and must be understood before committing.

Acceptance: both new tests pass; the existing tests listed in the Testing Plan pass unedited, namely `model/tests.rs` read-back and `with_model`, `options-tests.rs` temperature range, `client/tests.rs` name validation, `error-tests.rs` `each_kind_has_its_fixed_phrase`, `a_phrased_failure_shows_only_the_fixed_phrase`, `a_specific_extends_the_phrase_after_a_colon_and_never_adds_detail`, `normalize-tests.rs` `non_object_arguments_are_rejected`, `read-tests.rs` around line 170, and `models-tests.rs:73` and `:160` with its two-key rejection message at line 196; `cargo +nightly-2026-09-05 xtask api --check` passes with `CARGO_BUILD_WARNINGS` unset; and the docs gates pass with `broken_intra_doc_links` denied.

</step-2>

<step-3>

### Step 3: correct the two guide chapters [completed]

- Component: `runvil-docs-guide`
- Tests: from the `runvil` repository root with `RUNVIL_DOCS` set to `c:\Users\Vinnie\cursor\runvil-docs`:
  - `cargo xtask site --books-only`

Carries Item 9 (S2-5, S2-8). One commit, in the `runvil-docs` clone, because that clone is its own repository; the plan's decision record requires the guide edits to land there under their own commit. Nothing in the `runvil` repository changes, so this step produces no commit there.

One piece. The two chapters are one piece of prose work with the guide's house rules as its only shared contract, and both edits are single-sentence corrections, so there is nothing to divide.

- These edits land in the `runvil-docs` clone, now at `b6c0d90`. Every chapter keeps one paragraph per line, uses no em dash and no double dash, and opens code fences with four backticks, per `runvil-docs/CONTRIBUTING.md`.
- In `src/language/11-conversations.md` line 1217, delete `, and \`thinking\`` so the sentence names `temperature` and `max_tokens` only, leaving "such as `temperature` and `max_tokens`, apply to every round of the loop". `models.use` accepts exactly those two keys and rejects every other, as `lua/src/models.rs:185-193` shows, and `thinking` is frozen from the role's hard keyword.
- In `src/language/10-models.md` line 1453, replace "The Engine adds no metrics of its own, so `metrics` holds only what the provider reported." with "The Engine adds nothing of its own, so every figure comes from one of four sources: the backend's `usage`, llama.cpp's `timings`, vLLM's `metrics`, and the client's own `client` timing." Those four are the sections `CallMetrics` declares at `crates/runvil-internal/types/src/metrics.rs:85-97`.
- Nothing else is needed in the guide. Its error-kind table already gained the `InvalidParts` row in commit `4f537d5`, which closed that finding.
- The build command is `cargo xtask site --books-only` run from the `runvil` root, which is what `runvil-docs/CONTRIBUTING.md:7` names; the `runvil-docs` clone has no `xtask` of its own. Do not add `SUMMARY.md`, `index.md`, or `src/introduction.md` to the clone, because the assembler writes those into the staged books only.

Acceptance: both sentences read as quoted above, the guide keeps one paragraph per line, and `cargo xtask site --books-only` builds every book with no link failure.

</step-3>

</execution-plan>
