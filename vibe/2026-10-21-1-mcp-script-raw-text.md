---
name: MCP script calls return raw text
overview: "plugin-mcp marks its output by caller: a call the section's Lua makes through tools.call gets the raw text back, and a call a model round makes keeps the untrusted envelope. Both of the Pack's tools follow the rule. runvil-plugin's TestCall gains a way to name the caller so the Pack's tests can exercise both paths."
todos:
  - id: s1-test-call-caller
    content: "S1: add TestCall::with_caller to runvil-plugin's test support, with a test that a call built with ToolCaller::Model lends a model origin"
    status: pending
  - id: s2-mcp-trust-by-caller
    content: "S2: plugin-mcp returns trusted output to ToolCaller::Script and untrusted output to every other caller, for both get_dynamic_tools and call_dynamic_tool; update the crate docs and tests"
    status: pending
isProject: false
---

# plugin-mcp: No Envelope on Lua Calls

<product-contract>

## Product Requirements

- Ticket: GH-164, "plugin-mcp: no envelope on Lua calls". The ticket's whole text: "Script-issued MCP calls return the raw text string, and the model path keeps its envelope."
- Repository: `runvil` (`/code/runvil`), branch `GH-164-plugin-mcp-remove-envelope`, at `31941a46`.
- Problem: `plugin-mcp` returns every output as `ToolOutput::untrusted`. The shared tool dispatch (`crates/runvil-internal/lua/src/dispatch.rs`, the `match output.trust()` after the call) nonce-wraps untrusted output before it reaches either the next model turn or the calling script. A section's Lua that calls `mcp/call_dynamic_tool` or `mcp/get_dynamic_tools` through `tools.call` therefore gets the envelope, not the server's text. The crate docs already promise that a script reads the listing with `json.decode`, which the envelope breaks.
- Goals
  - A call whose origin's caller is `ToolCaller::Script` gets the raw text: the exact string the call renders today before wrapping, with no envelope.
  - A call whose origin's caller is `ToolCaller::Model`, or any caller a future version adds, keeps the untrusted envelope exactly as today.
  - The rule covers both of the Pack's tools, `get_dynamic_tools` and `call_dynamic_tool`.
- Non-goals: no change to the Engine, the Harness, the dispatch's wrapping, the envelope format, other Packs, error mapping, or the text a call renders. No change to `ToolCallOrigin` or `ToolCaller`.
- Success criteria
  - Through the Pack's own tests: a script-origin call of either tool returns `OutputTrust::Trusted` with the same text as before; a model-origin call returns `OutputTrust::Untrusted` with the same text.
  - Errors, including an `isError` result, stay `ToolError`s for both callers.
  - Every touched test, lint, format, and docs gate passes.

## Functional Specification

- `mcp/call_dynamic_tool` called by a script: returns the joined text of the result (text blocks, embedded text resources, and placeholders joined with blank lines, or the `structuredContent` JSON), marked trusted, so the Lua caller receives that string verbatim.
- `mcp/get_dynamic_tools` called by a script: returns the listing JSON text, marked trusted, so `json.decode` on the returned string succeeds.
- Either tool called by a model round: returns the same text marked untrusted, so the dispatch nonce-wraps it as today.
- A caller that is neither `Script` nor `Model` (the enum is `#[non_exhaustive]`) is treated as a model call: untrusted.
- Failures are unchanged for both callers: argument refusals, unknown namespaces or tools, servers not ready, transport errors, and `isError` results are `ToolError`s with the same kinds and messages.

</product-contract>
<implementation-contract>

## Technical Design

- `runvil-plugin` (test support only, behind the `test-support` feature)
  - `crates/runvil-plugin/src/testing.rs`: add `TestCall::with_caller(self, caller: ToolCaller) -> TestCall`, a builder like `with_services`, which replaces the origin's caller and keeps its execution (`plugin-test`) and section (`Test`). `TestCall::new` keeps the `ToolCaller::Script` default. Update the struct doc, which says the origin names "the script of the section `Test`", to say the caller is the script unless `with_caller` names another.
  - `ToolCallOrigin`'s fields are public and `#[non_exhaustive]` blocks struct literals outside its crate, so rebuild the origin with `ToolCallOrigin::new("plugin-test", "Test", caller)`.
- `plugin-mcp`
  - Trust is decided once, in `Mcp::call` (`crates/plugin-mcp/src/dynamic.rs`), from `cx.origin().caller`. Add a private free function in `dynamic.rs`, `fn marked(caller: ToolCaller, text: String) -> ToolOutput`, that returns `ToolOutput::trusted(text)` for `ToolCaller::Script` and `ToolOutput::untrusted(text)` for every other caller (a `_` arm, because `ToolCaller` is `#[non_exhaustive]`).
  - The text path below `Mcp::call` returns plain text instead of `ToolOutput`: `result::render` and `result::output` return `Result<String, ToolError>`; `server::send`, `server::call`, and `AbortOnDrop`'s join handle carry `Result<String, ToolError>`; `Mcp::call_tool` returns `Result<String, ToolError>`. `Mcp::list` already returns text. `Mcp::call` maps both tools' `Ok` text through `marked`. Error construction is untouched. Doc comments that say "tool output" for these functions say "text" instead.
  - `crates/plugin-mcp/src/lib.rs` crate docs: replace the invariant "Every output is untrusted, because descriptions, schemas, and results all come from the servers." with one stating that output to a model round is untrusted, because descriptions, schemas, and results all come from the servers, and output to the section's own script is trusted, so the script receives the server's text without the envelope. Add one sentence to the description of the two tools saying the same thing for a reader of the tool list.
- Boundaries: `plugin-mcp` keeps depending only on `runvil-plugin` among workspace crates (`ToolCaller` is re-exported there). No public item of `plugin-mcp` changes; only `install` is public.

</implementation-contract>
<verification-contract>

## Testing Plan

- Step 1 (`runvil-plugin`): in `crates/runvil-plugin/tests/it/test_call.rs`, add a test that `TestCall::new(tool).with_caller(ToolCaller::Model)` lends a context whose `origin().caller` is `ToolCaller::Model`, whose `origin().section` is `Test`, and whose `tool()` is the given tool. It fails to compile before `with_caller` exists. The existing test that `TestCall::new` lends a script origin stays and pins the default.
- Step 2 (`plugin-mcp`)
  - `crates/plugin-mcp/tests/it/support.rs`: `invoke` builds its `TestCall` with `.with_caller(ToolCaller::Model)` and keeps asserting `OutputTrust::Untrusted`, with the assertion message changed to say model-round output is untrusted. Every existing test that goes through `invoke`, `list`, or `call` thereby keeps pinning the model path. Add a helper that calls a tool as the script (`TestCall::new`, whose default caller is `Script`) and returns the whole `ToolOutput`.
  - New integration tests (in `tests/it/calls.rs` for the call tool, in `tests/it/listing.rs` for the listing), names stating the behavior:
    - A script call of `call_dynamic_tool` to the fixture's `Echo` tool with `{ "q": "hello" }` returns `OutputTrust::Trusted` and the text `echo: {"q":"hello"}` exactly. Before the change it is `Untrusted`, so the test fails.
    - A script call of `get_dynamic_tools` with no arguments returns `OutputTrust::Trusted`, and its text parses as JSON equal to the model-path listing for the same Pack.
    - A script call of `call_dynamic_tool` to the fixture's `fail` tool is a `ToolErrorKind::Backend` error with the message `it failed`, the same as the model path.
  - `crates/plugin-mcp/src/result-tests.rs`: `render` and `output` now return text, so drop the trust assertions there (trust is no longer `result.rs`'s concern) and assert on the returned strings. Keep every text-rendering case.
  - The focused run must show the new script-path tests failing before the source change and passing after.
- Exit checks at the final step, full scope, from the project survey: format check, both clippy runs, the full test suites, the docs builds, `cargo test -p build-xtask`, and `node --test crates/workshop/ui/test/docs-claims.mjs` (the crate docs change touches an `## Invariants` section).

</verification-contract>
<decision-record>

## Decision Record

- User-resolved choices
  - Trust boundary: output to the section's own script is trusted, output to a model round stays untrusted. This is the ticket's requirement, settled by the operator. Consequence: a script that forwards MCP text into a prompt does so without the envelope, which is the script author's decision, just as with any other value the script builds.
  - Scope: both tools, `get_dynamic_tools` and `call_dynamic_tool`, follow the rule (the operator: "unwrap both"). Reason: a script that lists tools wants to decode the listing JSON, which the crate docs already promise and the envelope breaks.
- Reversible decisions and consequences
  - Decide trust in the Pack from `ToolContext::origin`, not in the Engine's dispatch. The Plugin contract already hands each call its origin "to apply different policy to the same tool depending on who called it", and the dispatch honors the output's own trust marking, so no Engine change is needed. Rejected: making the dispatch skip wrapping for every script call, because that changes every Pack's contract, not this ticket's.
  - An unknown future `ToolCaller` variant gets untrusted output, the safe default, matching the dispatch's own handling of an unknown `OutputTrust`.
  - Trust is decided at one point, `Mcp::call`, and the layers below return plain text. Rejected: passing the caller down into `server` and `result`, because it would thread a policy value through transport code that does not need it.
  - Tests reach the model path through a new `TestCall::with_caller` in `runvil-plugin`'s test support. Rejected: building a `ToolContext` by hand in `plugin-mcp`'s tests, because an `Access` needs `runvil-vfs` constructors that `runvil-plugin` does not re-export, and `plugin-mcp` may depend only on `runvil-plugin` among workspace crates.
- Assumptions and risks
  - Assumption: a trusted script result is not otherwise treated as model input; the dispatch hands the content to the script and records the result's trust in the tool-result report.
  - Risk: none to stored data or wire formats; the change affects only the trust marking of in-run output.

### Deferred and Out of Scope

- Any change to how other Packs (`plugin-web`) mark script-issued output - revisit with its own ticket if wanted.

</decision-record>
<project-survey>

## Project Survey

- Status: complete
- Build command: `cargo build --locked -p <crate>` (plain `cargo build` builds only `crates/gateway/app`, the sole default member; the desktop app is `cargo build -p workshop`).
- Focused test command pattern: `cargo nextest run --locked -p <crate> -E 'test(<name>)'` (nextest config in `.config/nextest.toml`).
- Component test command pattern: `cargo nextest run --locked -p <crate> --all-features` (e.g. `-p plugin-mcp`); workshop crates run without `--all-features`.
- Full-suite test command: `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`, then `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`; plus `cargo test -p build-xtask` (structural checks) and `node --test crates/workshop/ui/test/docs-claims.mjs` (wording scan).
- Linter command: `CARGO_BUILD_WARNINGS=deny cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features` and `CARGO_BUILD_WARNINGS=deny cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`; extra gate `cargo check -p gateway --no-default-features`.
- Formatter check command: `cargo fmt --all --check`.
- Docs command: `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api` and `RUSTDOCFLAGS="-D warnings" cargo doc -p runvil --no-deps`; facade surface `cargo +<nightly from crates/build-xtask/src/api/toolchain.rs> xtask api --check` with `CARGO_BUILD_WARNINGS` unset (checks `crates/runvil/public-api.txt`).
- Test placement and naming conventions: unit tests live in a kebab sibling `foo-tests.rs` wired from `foo.rs` via `#[path = "foo-tests.rs"] mod tests;` (e.g. `crates/plugin-mcp/src/{connect,entry,result,server}-tests.rs`); integration tests are a single target per crate under `tests/it/main.rs` or `tests/suite/main.rs` with topic modules (`calls.rs`, `listing.rs`) plus `support.rs`/`fixture.rs`; prompt fixtures under `tests/prompts/`. Engine tests: no tokio, sleeps, wall clock, or OS entropy; fixed seeds; temp dirs named from test name, pid, counter.
- Directory map: `crates/` top level = public/shared layer (`runvil` Engine facade, `runvil-plugin` Plugin contract, `harness`, `harness-gateway-client`, `plugin-mcp`/`plugin-web`/`plugin-user-input` Plugins, `gateway-api-types`/`gateway-api-discovery`, `shared-*`, `build-*` tooling incl. `build-xtask`); containers of private crates: `crates/runvil-internal/` (types, engine, lua, parser, vfs, model-client), `crates/harness-internal/` (runner, plugin-loader, test-plugin), `crates/gateway/` (app, config, routing, local, stt/*, web-search, ...), `crates/workshop/` (desktop, server, server-api, ui SPA, look CSS, and support crates); `crates/shared-ui` is a TS/CSS package; `guide/` user guide books; `prompts/` sample prompts; `tools/` Node scripts; `vibe/` dated plan docs; `.config/nextest.toml`.
- Component boundaries: Engine (`runvil` → `runvil-internal/*`) depends on no tokio/reqwest/async-trait/getrandom/fastrand and never names the Host; `runvil-plugin` defines Pack/tool contracts; Plugins (e.g. `plugin-mcp` → `runvil-plugin`, `rmcp`, `reqwest`, `tokio`) depend on the contract, not the Harness; `harness` → `harness-runner`, `harness-plugin-loader`, `runvil`, `runvil-plugin`; Hosts (Workshop) → Harness; families reach each other only through top-level crates; boundaries enforced by `cargo test -p build-xtask`.
- Conventions summary: edition 2024, rust-version 1.95, workspace-inherited deps and lints with comments explaining each dependency's purpose; flat source dirs with `parent-label.rs` siblings below three files; capitalized defined terms (Engine, Harness, Host, Pack, Plugin) in docs/comments; crate docs carry `## Invariants`; model-oriented error messages naming required vs actual; recorded JSON round-trips exactly with sorted keys; behavior changes ship with tests; comments only for non-obvious constraints, citing upstream issue URLs for workarounds.

</project-survey>
<execution-plan>

## Execution Instructions

<step-1>

### Step 1: TestCall::with_caller in runvil-plugin test support [completed]

- Component: none
- Tests: `cargo nextest run --locked -p runvil-plugin --all-features`

- Scope: `runvil-plugin` test support only, behind the `test-support` feature. No change to `ToolCallOrigin` or `ToolCaller`.
- `crates/runvil-plugin/src/testing.rs`:
  - Add `TestCall::with_caller(self, caller: ToolCaller) -> TestCall`, a builder shaped like `with_services`, which replaces the origin's caller and keeps its execution (`plugin-test`) and section (`Test`). Rebuild the origin with `ToolCallOrigin::new("plugin-test", "Test", caller)`, since `ToolCallOrigin` is `#[non_exhaustive]` and struct literals are blocked outside its crate.
  - `TestCall::new` keeps the `ToolCaller::Script` default.
  - Update the `TestCall` struct doc, which says the origin names "the script of the section `Test`", to say the caller is the script unless `with_caller` names another.
- `crates/runvil-plugin/tests/it/test_call.rs`: add a test (e.g. `a_test_call_with_a_caller_lends_that_caller`) that `TestCall::new(tool).with_caller(ToolCaller::Model)` lends a context whose `origin().caller` is `ToolCaller::Model`, whose `origin().section` is `Test`, and whose `tool()` is the given tool. It fails to compile before `with_caller` exists. Keep `a_test_call_lends_its_tool_a_script_origin_and_a_working_filesystem`, which pins the `Script` default.
- Commit: code and test together.

</step-1>

<step-2>

### Step 2: plugin-mcp marks output by caller for both tools

- Component: none
- Tests: `cargo nextest run --locked -p plugin-mcp --all-features`; exit checks: `cargo fmt --all --check`; `CARGO_BUILD_WARNINGS=deny cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`; `CARGO_BUILD_WARNINGS=deny cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`; `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`; `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`; `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`; `RUSTDOCFLAGS="-D warnings" cargo doc -p runvil --no-deps`; `cargo test -p build-xtask`; `node --test crates/workshop/ui/test/docs-claims.mjs`

- Behavior (ticket GH-164: "Script-issued MCP calls return the raw text string, and the model path keeps its envelope."):
  - `mcp/call_dynamic_tool` called with origin caller `ToolCaller::Script` returns the joined text of the result (text blocks, embedded text resources, and placeholders joined with blank lines, or the `structuredContent` JSON), marked trusted, exactly the string rendered today before wrapping.
  - `mcp/get_dynamic_tools` called by `ToolCaller::Script` returns the listing JSON text, marked trusted, so `json.decode` succeeds.
  - Either tool called by `ToolCaller::Model`, or any other caller (`ToolCaller` is `#[non_exhaustive]`), returns the same text marked untrusted, so the dispatch nonce-wraps it as today.
  - Failures unchanged for both callers: argument refusals, unknown namespaces or tools, servers not ready, transport errors, and `isError` results stay `ToolError`s with the same kinds and messages.
  - No change to the Engine, the Harness, the dispatch's wrapping, the envelope format, other Packs (`plugin-web` is out of scope), error mapping, or rendered text. `plugin-mcp` keeps depending only on `runvil-plugin` among workspace crates (`ToolCaller` is re-exported there); no public item changes (only `install` is public).
- Source, `crates/plugin-mcp/src/`:
  - `dynamic.rs`: add private `fn marked(caller: ToolCaller, text: String) -> ToolOutput` returning `ToolOutput::trusted(text)` for `ToolCaller::Script` and `ToolOutput::untrusted(text)` in a `_` arm. `Mcp::call` reads `cx.origin().caller` once and maps both tools' `Ok` text through `marked`. `Mcp::call_tool` returns `Result<String, ToolError>`; `Mcp::list` already returns text.
  - `result.rs`: `result::render` and `result::output` return `Result<String, ToolError>`.
  - `server.rs`: `server::send`, `server::call`, and `AbortOnDrop`'s join handle carry `Result<String, ToolError>`.
  - Error construction untouched. Doc comments that say "tool output" for these functions say "text" instead.
  - `lib.rs` crate docs: replace the `## Invariants` line "Every output is untrusted, because descriptions, schemas, and results all come from the servers." with one stating that output to a model round is untrusted, because descriptions, schemas, and results all come from the servers, and output to the section's own script is trusted, so the script receives the server's text without the envelope. Add one sentence to the description of the two tools saying the same for a reader of the tool list. Use capitalized defined terms (Pack, Engine, Harness) per the wording scan.
- Tests:
  - `crates/plugin-mcp/tests/it/support.rs`: `invoke` builds its `TestCall` with `.with_caller(ToolCaller::Model)` and keeps asserting `OutputTrust::Untrusted`, with the message changed to say model-round output is untrusted; existing tests through `invoke`, `list`, and `call` thereby keep pinning the model path. Add a helper that calls a tool as the script (`TestCall::new`, default caller `Script`) and returns the whole `ToolOutput`.
  - `tests/it/calls.rs`: a script call of `call_dynamic_tool` to the fixture's `Echo` tool with `{ "q": "hello" }` returns `OutputTrust::Trusted` and the text `echo: {"q":"hello"}` exactly. A script call of `call_dynamic_tool` to the fixture's `fail` tool is a `ToolErrorKind::Backend` error with message `it failed`, same as the model path.
  - `tests/it/listing.rs`: a script call of `get_dynamic_tools` with no arguments returns `OutputTrust::Trusted`, and its text parses as JSON equal to the model-path listing for the same Pack.
  - Test names state the behavior.
  - `src/result-tests.rs`: drop the trust assertions (trust is no longer `result.rs`'s concern), assert on returned strings, keep every text-rendering case.
  - Run the new script-path tests focused (`cargo nextest run --locked -p plugin-mcp --all-features -E 'test(<name>)'`) before the source change to show them failing (the `Echo` and listing tests report `Untrusted`), then after to show them passing.
- Commit: code, docs, and tests together.

</step-2>

</execution-plan>
