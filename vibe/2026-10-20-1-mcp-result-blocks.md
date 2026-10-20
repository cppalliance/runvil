---
name: MCP result blocks for Lua
overview: "Give a Lua script the MCP CallToolResult of a plugin-mcp tool call as a table (content blocks, structuredContent, isError) instead of one flattened string, so a script can json.decode a JSON text block without string manipulation; the model-facing text stays as it is."
todos: []
isProject: false
---

# MCP Result Blocks for Lua

<product-contract>

## Product Requirements

A script that calls an MCP tool today receives one string: every content block rendered, joined with blank lines, and wrapped in the untrusted envelope. When a server returns a JSON text block next to other blocks, the script cannot decode the JSON without stripping the envelope and the extra block text by pattern. This plan makes script-issued calls to plugin-mcp tools return the server's `CallToolResult` as a Lua table, while every model-facing path keeps receiving the flattened, wrapped text.

- Problem and users:
  - Prompt authors who call MCP tools from Lua with `tools.call`.
  - Repro: server `https://wg21.org/mailing/mcp`, tool `get_paper_download_url` with `{ papers = "P0260R20" }`, returns one `text` block (JSON) plus one `resource_link` block per URL, which is valid MCP 2025-06-18. Lua receives `{"expires": …}\n\n[resource: https://…]` inside the envelope, and `json.decode` fails with `trailing characters at line 3 column 1`.
  - The flattening is in `crates/plugin-mcp/src/result.rs`: `block_text` renders each block (`resource_link` becomes `[resource: <uri>]`), `text_of` joins them with `"\n\n"`, and `render` returns `ToolOutput::untrusted(text)`.
  - The envelope breaks decoding independently of the extra blocks. The Engine encodes `<` as `&lt;` inside it (`crates/runvil-internal/types/src/untrusted.rs`), so even the pattern-stripping workaround yields altered JSON strings.
- Goals:
  - A script-issued call to a plugin-mcp tool returns a Lua table that mirrors the MCP `CallToolResult`, with one entry per content block, in server order.
  - A script can `json.decode(result.content[1].text)`, or read `result.structuredContent`, with no string manipulation.
- Non-goals:
  - Changing what the model receives for model-issued MCP calls.
  - Changing MCP servers or the MCP protocol; servers keep returning standard `CallToolResult` values.
  - Parsing, merging, or reinterpreting text inside content blocks.
- Success criteria:
  - The repro shape (a JSON text block plus resource_link blocks) decodes with `json.decode(result.content[1].text)` in a Lua test against a fixture MCP server.
- Constraints:
  - The model-facing text for MCP results stays byte-identical to today's.
  - Results of non-MCP tools stay unchanged on every path.
  - The four defined terms (Engine, Harness, Host, Plugin) keep their meanings from `AGENTS.md`.
- Open questions:
  - None

## Functional Specification

When a script calls an MCP tool, it receives a table shaped like the MCP wire form of `CallToolResult`. All other tools, and all model-facing deliveries, keep their current string form. Server errors still raise, and oversized results raise rather than truncate.

- Actors and workflows:
  - A prompt author's script calls `tools.call(<mcp tool>, args)` (by id, alias, or tool object) and indexes the returned table.
  - The model calls the same MCP tool through `models.loop`, and its tool message stays the flattened, wrapped text.
- Inputs and outputs:
  - Output shape for script-issued calls to plugin-mcp tools: the MCP wire JSON of the `CallToolResult`, converted to Lua the same way `json.decode` converts JSON. plugin-mcp hands the Engine this JSON as text, and the Engine decodes it once for the script, so the script never decodes the result itself. The fields are:
    - `content`: an array of blocks, each with its wire fields. A text block is `{type="text", text=…}`. A resource link is `{type="resource_link", uri=…, name=…, mimeType=…, …}`. Image and audio blocks carry `type`, `mimeType`, and base64 `data`. An embedded resource is `{type="resource", resource={uri=…, mimeType=…, text=… or blob=…}}`.
    - `structuredContent`: present when the server sent it.
    - `isError`: present when the server sent it.
    - `_meta`: present when the server sent it.
  - Field names follow the MCP wire form (camelCase), so they match the MCP specification.
  - JSON null becomes `json.null`, and JSON arrays carry the same array metatable that `json.decode` results carry.
  - Text inside blocks is the server's text, not wrapped and not encoded.
- States and validation:
  - Script-issued calls to tools whose output carries no structured JSON text keep today's behavior: a Plain binding returns a string, and a Structured binding returns parsed JSON. Only plugin-mcp attaches structured JSON text, so every other tool behaves exactly as before.
- Errors and recovery:
  - A result with `isError: true` still raises the tool error in Lua, as today. The message is the flattened text, or `the MCP tool reported an error and gave no text`.
  - Structured JSON text longer than 8,388,608 bytes (the `json.decode` input cap) raises a Lua tool error naming the tool alias rather than decoding. The model-facing text is unaffected.
  - Structured JSON text that does not parse raises a Lua tool error naming the tool alias and the parse error.
- Security and privacy behavior:
  - The table carries raw untrusted text. This is a deliberate exception to the rule that untrusted text is wrapped once on entry to the run (see D4).
  - The `tools.call` documentation states that a script forwarding table text to a model must wrap it with the existing `untrusted(s)` global.
  - The model-facing path still wraps every untrusted MCP result.
- Acceptance criteria:
  - A prompt can `json.decode` the text block of a multi-block MCP result without string manipulation.
  - For the same call, the model-facing text equals today's flattened, wrapped text.

</product-contract>
<implementation-contract>

## Technical Design

A tool result stays a string. The Plugin output type gains one optional second string next to its text: structured JSON text meant for a calling script. plugin-mcp fills it with the `CallToolResult` serialized as JSON. The Lua dispatch path decodes that text into a table for the calling script, while model-facing paths keep using the text. The answer record persists the JSON text so the record still holds everything a script could observe. Behavior changes only for MCP tools, because plugin-mcp is the only Plugin that attaches the second string.

- Architecture:
  - Plugin-to-Engine output stays one in-process `ToolOutput` value (`crates/runvil-internal/types/src/tools/output.rs`). It is returned from `Plugin::call` (`crates/runvil-plugin/src/plugin.rs`).
  - Script-issued bound calls resume through `ToolCallOutcome::from_dispatch` (`crates/runvil-internal/lua/src/protocol/answer.rs`). Model-issued and batched calls (`CallSite::Model` / `Batch`) resume as `ToolCallOutcome::Plain` and never consult a structured payload (`crates/runvil-internal/engine/src/execute/scheduler/apply.rs`).
- Modules and interfaces:
  - `ToolOutput` gains optional structured JSON text (`Option<String>`), never an in-memory value. It has a builder `with_structured_json(json: String)` and a getter `structured_json() -> Option<&str>`. Text and trust keep their current constructors and meaning. `ToolOutput` is already `#[non_exhaustive]` with private fields, so existing Plugins compile unchanged.
  - Contract on the structured JSON text, documented on `ToolOutput`: it is a complete JSON document, and a Plugin that attaches it to any output of a tool attaches it to every successful output of that tool. This keeps the Lua return type fixed per tool.
  - plugin-mcp `render` keeps building today's flattened text and trust, and also attaches `serde_json::to_string(&CallToolResult)` as the structured JSON text (rmcp `=3.5.1`, `crates/plugin-mcp/Cargo.toml`).
  - In the Lua crate, a script-issued bound call whose dispatched output carries structured JSON text resumes with that text decoded into a Lua value: first the 8,388,608-byte length check on the text, then a JSON parse, then the same `lua.to_value` conversion that `json.decode` uses (`crates/runvil-internal/lua/src/json/decode.rs`). This takes precedence over the binding's Plain or Structured kind.
- File and public API changes:
  - Public: the new `ToolOutput` builder and getter, and the new `ToolAnswerRecord` field and builder. All of them take or return strings. `crates/runvil/public-api.txt` is regenerated through `xtask api`.
  - Documentation: the `tools.call` docs in `crates/runvil-internal/lua/src/tools.rs`, `crates/runvil-internal/lua/src/handles.rs`, and the coroutine shim; the plugin-mcp crate docs in `crates/plugin-mcp/src/lib.rs`; and the `Plugin::call` docs.
- Data, persistence, failure, security, and privacy constraints:
  - `ToolAnswerRecord` (built in `crates/runvil-internal/engine/src/execute/run/effect.rs`) gains the structured JSON text as an optional string field, `structured_json`. The field is omitted from JSON when absent, so records of other tools serialize byte-identically to today.
  - The record has no format version, and replay compatibility is handled by run flags (`crates/runvil-internal/types/src/replay.rs`). The new behavior is driven by the Plugin's output, not by an Engine behavior switch, so no new flag is added.
  - `Event::ToolResult` keeps carrying the flattened, wrapped text and gains no payload.
  - The model-facing nonce wrapping is unchanged (`crates/runvil-internal/lua/src/dispatch.rs`).

</implementation-contract>
<verification-contract>

## Testing Plan

Tests use fixture MCP servers and in-process Lua runs; no external network. They cover the structured JSON text's construction in plugin-mcp, its decoding in Lua, and the unchanged model-facing path.

- Unit:
  - `ToolOutput` tests: `trusted` and `untrusted` outputs carry no structured JSON text, and `with_structured_json` keeps text and trust and returns the same string.
  - plugin-mcp `result-tests.rs`: for a text block plus a resource_link block, the structured JSON text parses to the wire JSON and the flattened text is unchanged.
  - plugin-mcp `result-tests.rs`: `structuredContent` appears in the structured JSON text when present.
  - plugin-mcp `result-tests.rs`: `isError: true` still returns a Backend `ToolError`.
  - Lua answer tests: structured JSON text decodes with `json.null` for nulls and the array metatable on arrays.
  - Lua answer tests: structured JSON text over 8,388,608 bytes raises the size error.
  - Lua answer tests: structured JSON text that does not parse raises the parse error naming the alias.
  - Lua answer tests: an output without structured JSON text keeps the Plain and Structured behavior.
  - Answer record tests: the `structured_json` field round-trips as a string and is omitted when absent.
- Integration and end-to-end:
  - A fixture MCP server returning the repro shape, a JSON text block plus two resource_link blocks. A Lua script runs `json.decode(tools.call(...).content[1].text)` and reads `content[2].uri`. These tests live in `crates/workshop/server`, the one crate the boundary checks allow to depend on both `harness` and `plugin-mcp`.
  - A `models.loop` call to the same tool yields a tool message equal to today's flattened, wrapped text.
- Regression, security, and performance:
  - Answer records for non-MCP tools serialize byte-identically to before.
  - The untrusted-envelope test for model-facing MCP results still passes.
- Operational limits:
  - Development machine only.
  - No downloads beyond package dependencies.
  - No command other than the project's build and test commands is expected to exceed 10 minutes.
  - Fixture data only; no calls to `https://wg21.org/mailing/mcp`.
- Exit criteria:
  - All tests above pass, along with the `AGENTS.md` verification commands (nextest, clippy, fmt, doc, `xtask api --check`, `build-xtask` tests).

</verification-contract>
<decision-record>

## Decision Record

- Decisions:
  - D1. Script-issued calls to plugin-mcp tools return the `CallToolResult` as a table, replacing the flattened string for scripts. The user's words: "pass Lua the MCP CallToolResult as a table instead of a flattened string … Then Lua always gets a list of blocks, the harness doesn't try to parse or merge text, and scripts decode a block's text (or just use structuredContent) when they expect JSON."
  - D2. MCP servers and the model-facing text are unchanged. The user's words: "MCP should keep returning whatever that standard is"; the agreed follow-up said what the model sees "can stay as it is today."
  - D3. The table uses MCP wire field names (`content`, `structuredContent`, `isError`, `mimeType`). This matches the user's proposed shape and the MCP specification, and it needs no mapping code because rmcp serializes to it.
  - D4. The table carries raw, unwrapped untrusted text. The acceptance criterion requires decoding without string manipulation, and the envelope's `&lt;` encoding alters JSON, so wrapped text cannot satisfy it. Scripts wrap with `untrusted(s)` before forwarding text to a model. The user delegated this call: "i don't know rust, do whatever is needed here".
  - D5. The structured JSON text rides on `ToolOutput` as an optional second string, available to any Plugin, with plugin-mcp as the only producer. A change confined to plugin-mcp cannot work: the Engine wraps every untrusted result for scripts in the untrusted envelope, whose `&lt;` encoding alters JSON, and a Plugin can avoid the envelope only by marking its output trusted, which would also send unwrapped server text to the model. Behavior changes only for MCP tools.
  - D6. The structured JSON text is chosen at call time from the output, not declared in `ToolInfo`, under the every-output contract on `ToolOutput`. This adds the least public API.
  - D10. A tool result stays a string: the second field is JSON text, never an in-memory value. This answers the PR #157 review by Vinnie Falco: "The tool's return value can only be a string. A JSON string, but still a string." The user chose this option: "json string then".
  - D11. The Engine decodes the structured JSON text for the script, so `tools.call` returns a table and the script decodes only the server's own JSON inside a block. The user rejected making scripts decode the result themselves: "why would i ever want to json.decode TICE???"
  - D7. `isError: true` still raises in Lua, matching today's behavior, so error handling in existing scripts keeps working.
  - D8. Binary block data (image and audio `data`, resource `blob`) passes through as base64. The 8,388,608-byte cap on the structured JSON text matches `json.decode`'s input cap.
  - D9. The user delegated every technical choice in this plan that the user did not state: the size cap, error behavior, field naming and null conversion, base64 passthrough, record persistence, the every-output contract, the unchanged `Event::ToolResult`, the test list, the operational limits, and the exit criteria. The user's words: "i don't know rust, do whatever is needed here".
- Rejected alternatives:
  - An arbitrary `serde_json::Value` payload on `ToolOutput`. The PR #157 review rejected it because it lets any tool return an in-memory value instead of a string. Replaced by D10.
  - An optional content-block list on `ToolOutput`. It matches the content-block result shape that several APIs use, but it adds a structured type to every Plugin's output and the review asked for a string. Revisit if non-MCP Plugins need to return blocks.
  - Returning the raw structured JSON text to the script and having it call `json.decode` on the result. It is closest to string-only on the script side but makes every script decode twice. Rejected by the user (D11).
  - Returning the serialized `CallToolResult` as the only text. The model would see the JSON instead of today's flattened text, breaking D2, and scripts would still receive it inside the untrusted envelope.
  - Wrapping each string inside the table in the untrusted envelope. It fails the acceptance criterion. Revisit if the Engine gains a way to mark model-bound text untrusted without wrapping it for the script.
  - A tainted-string userdata that re-wraps when sent to a model. Its scope far exceeds this ticket. Revisit if more raw-untrusted paths to scripts appear.
  - An opt-in function (for example `tools.call_result`) that leaves `tools.call` returning a string. The user chose the table as the result of `tools.call`. Revisit if existing prompts depend on the string form.
  - Declaring the result kind in `ToolInfo`. It adds public API for no current need. Revisit if a Plugin attaches payloads only to some outputs.
  - Rendering non-MCP-standard metadata outside the text body, from the ticket's third option. It is superseded by passing the whole result.
- Assumptions, risks, and notes:
  - Risk: the D4 exception lets a careless script forward attacker-influenced text to a model unwrapped. The mitigation is documentation plus the existing `untrusted(s)` global. The documented trust invariant covers model input only (`crates/runvil-internal/types/src/tools/output.rs`), while the vibe plans state wrap-once-on-entry for scripts as well.
  - Breaking change: scripts that treated an MCP result as a string (`tostring`, `..`) now get a table. No prompt, example, or test in this repository does so (`prompts/`, `crates/runvil/tests`, `crates/runvil/examples`, `crates/plugin-mcp/tests`).
  - Note: today an untrusted output on a Structured binding fails to parse because the envelope is not JSON (`crates/runvil-internal/lua/src/protocol/answer.rs`). This plan does not change that.

### Deferred and Out of Scope

- Deferred: giving scripts the content of `isError` results as a table. Revisit when a prompt needs to inspect error blocks.
- Out of scope: model-facing rendering of MCP results.
- Out of scope: MCP `outputSchema` handling in the tool catalog.

</decision-record>
<project-survey>

## Project Survey

- Status: complete
- Build command: `cargo build --locked -p <crate>` (plain `cargo build` builds only the default member `crates/gateway/app`; the desktop app is `cargo build -p workshop`).
- Focused test command pattern: `cargo nextest run --locked -p <crate> --all-features -E 'test(<name>)'`.
- Component test command pattern: `cargo nextest run --locked -p <crate> --all-features` (e.g. `-p plugin-mcp`, `-p harness`, `-p runvil-engine`).
- Full-suite test command: `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`; Workshop crates separately: `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`. Also `cargo test -p build-xtask` (boundary and structural checks) and `node --test crates/workshop/ui/test/docs-claims.mjs` (wording scan). Nextest config in `.config/nextest.toml`.
- Linter command: `CARGO_BUILD_WARNINGS=deny cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`, plus `cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`; extra shape gate `cargo check -p gateway --no-default-features`.
- Formatter check command: `cargo fmt --all --check` (config in `rustfmt.toml`).
- Docs command: `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`, plus `RUSTDOCFLAGS="-D warnings" cargo doc -p runvil --no-deps`; facade surface `cargo +<nightly from crates/build-xtask/src/api/toolchain.rs> xtask api --check` against `crates/runvil/public-api.txt`, with `CARGO_BUILD_WARNINGS` unset.
- Test placement and naming conventions: unit tests live in a sibling `foo-tests.rs` wired from `foo.rs` with `#[path = "foo-tests.rs"] mod tests;` (e.g. `crates/plugin-mcp/src/result.rs` → `result-tests.rs`); integration tests are a single binary per crate under `tests/<name>/main.rs` with sibling modules (`crates/plugin-mcp/tests/it/{main,fixture}.rs`, `crates/harness/tests/suite/*.rs`); test names are behavior sentences in snake_case; Engine tests are deterministic (no sleeps, wall clock, timeouts, or OS entropy; fixed printed seeds; temp dirs named from test name, pid, counter).
- Directory map:
  - `crates/`: top-level public and shared crates: `runvil` (Engine facade, `public-api.txt`), `runvil-plugin` (Plugin API), `harness`, `harness-gateway-client`, `plugin-mcp`, `plugin-web`, `plugin-user-input`, `gateway-api-types`, `gateway-api-discovery`, `shared-error-source`, `shared-loopback`, `shared-ui` (TS/CSS, not a crate), and `build-*` tooling (`build-xtask` holds structural checks and `xtask api`).
  - `crates/runvil-internal/`: private Engine crates `types` (incl. `untrusted.rs` envelope), `engine`, `lua`, `parser`, `vfs`, `model-client`.
  - `crates/harness-internal/runner`: private Harness runner (effect loop, performers, registry, recorder).
  - `crates/gateway/`: gateway family (`app`, `config`, `config-ui`, `local`, `logging`, `progress`, `protocol`, `routing`, `web-search`, `cloud-providers`, `stt/*`).
  - `crates/workshop/`: Workshop Host (`desktop`, `server`, `server-api`, `ui`, `look`, and support crates).
  - `guide/`: user guide books and site; `prompts/`: example prompt programs; `tools/`: Node scripts for gateway sidecar staging and TTS; `vibe/`: dated plans; `.github/workflows/`: CI (`ci.yml` mirrors the Verification gates).
- Component boundaries: Engine internals are reached only through the `runvil` facade; the Harness (`harness` → `harness-runner`) depends on the Engine and performs its effects; Plugins (`plugin-*`) depend only on `runvil-plugin`; Hosts (Workshop) depend on the Harness and may use Engine parser/types; product families (gateway, workshop, runvil-internal, harness-internal) reach each other only through top-level crates; Engine crates never depend on `tokio`, `reqwest`, `async-trait`, `getrandom`, or `fastrand`. `cargo test -p build-xtask` enforces these.
- Conventions summary: Rust 2024 edition, stable toolchain, MSRV 1.95 for Engine crates; flat source directories with kebab `parent-label.rs` siblings wired by `#[path]` until a group reaches three files; per-crate `## Invariants` docs and `AGENTS.md`/`README.md`; capitalized defined terms (Engine, Harness, Host, Plugin) with Engine crates saying "the caller"; model-oriented error messages naming required versus actual; recorder-bound JSON round-trips exactly with sorted keys; comments only for non-obvious constraints, workarounds cite upstream issue URLs; behavior changes ship with tests in the same change.

</project-survey>
<execution-plan>

## Execution Instructions

Components, in dependency order:

1. `structured-json-channel` (Steps 1-3): the Engine and Plugin API carry, record, and decode optional structured JSON text. It comes first because plugin-mcp and the end-to-end tests consume its API, and it is useful on its own to any Plugin. Its pieces are built sequentially: the `ToolOutput` field (Step 1) precedes the record field (Step 2) and the Lua decoding (Step 3), which both read it; the record precedes decoding so the record stays complete as soon as any script can observe the text.
2. `plugin-mcp-producer` (Step 4): plugin-mcp attaches the serialized `CallToolResult`. It follows the channel because it calls `with_structured_json`.
3. `end-to-end` (Step 5): Workshop server tests over a fixture MCP server plus the exit gates. It comes last because it exercises every earlier step together.

<step-1>

### Step 1: Structured JSON text on ToolOutput [completed]

- Component: `structured-json-channel`
- Tests: `cargo nextest run --locked -p runvil-types --all-features`, `cargo nextest run --locked -p runvil-plugin --all-features`, `RUSTDOCFLAGS="-D warnings" cargo doc -p runvil --no-deps`, and, with `CARGO_BUILD_WARNINGS` unset, `cargo +nightly-2026-09-05 xtask api --check`

- In `crates/runvil-internal/types/src/tools/output.rs`, add a private `structured_json: Option<String>` field to `ToolOutput` (already `#[non_exhaustive]` with private fields). `trusted` and `untrusted` set it to `None`.
- Add the builder `pub fn with_structured_json(self, json: String) -> ToolOutput` (keeps text and trust) and the getter `pub fn structured_json(&self) -> Option<&str>`. Never store an in-memory JSON value (D10).
- Document the contract on `ToolOutput`: the structured JSON text is a complete JSON document meant for a calling script; the model always receives `text()`; a Plugin that attaches it to any output of a tool attaches it to every successful output of that tool, which keeps the script-facing return type fixed per tool (D6). Note that the script receives the text decoded and unwrapped, so it is not covered by the untrusted envelope.
- Update the `Plugin::call` docs in `crates/runvil-plugin/src/plugin.rs` to describe the optional structured JSON text and the every-output contract.
- Unit tests in `crates/runvil-internal/types/src/tools/tests.rs`: `trusted` and `untrusted` outputs carry no structured JSON text; `with_structured_json` keeps text and trust and `structured_json()` returns the same string.
- Regenerate `crates/runvil/public-api.txt` through `cargo +nightly-2026-09-05 xtask api` (pinned in `crates/build-xtask/src/api/toolchain.rs`) so it lists the new builder and getter.
- Existing Plugins must compile unchanged.

</step-1>

<step-2>

### Step 2: Persist structured JSON text in ToolAnswerRecord [completed]

- Component: `structured-json-channel`
- Tests: `cargo nextest run --locked -p runvil-engine --all-features`, and, with `CARGO_BUILD_WARNINGS` unset, `cargo +nightly-2026-09-05 xtask api --check`

- In `crates/runvil-internal/engine/src/execute/run/effect.rs`, add `pub structured_json: Option<String>` to `ToolAnswerRecord` with `#[serde(default, skip_serializing_if = "Option::is_none")]`, so records of tools without it serialize byte-identically to today. `ToolAnswerRecord::new` sets it to `None`; add a builder `with_structured_json(self, json: impl Into<String>) -> ToolAnswerRecord`.
- In `EffectAnswer::record`, copy `output.structured_json()` into the record for `EffectAnswer::ToolCall(Ok(output))`.
- Add no replay run flag (`crates/runvil-internal/types/src/replay.rs` stays unchanged): the behavior is driven by the Plugin's output, not an Engine switch.
- `Event::ToolResult` keeps carrying the flattened, wrapped text and gains no payload.
- Tests in `crates/runvil-internal/engine/src/execute/run/effect-tests.rs`: a record with `structured_json` round-trips through `serde_json::to_string` / `from_str` as a string field; a record without it omits the key and serializes byte-identically to the existing `{"text":…,"trust":…}` form (non-MCP regression); `EffectAnswer::record` copies the structured JSON text from a `ToolOutput`.
- Recorder-bound JSON keeps canonical sorted keys.
- Regenerate `crates/runvil/public-api.txt` for the new field and builder.

</step-2>

<step-3>

### Step 3: Decode structured JSON text for script-issued calls

- Component: `structured-json-channel`
- Tests: `cargo nextest run --locked -p runvil-lua --all-features`, `cargo nextest run --locked -p runvil-engine --all-features`

- Thread the dispatched output's `structured_json()` from `prepare_dispatch` (`crates/runvil-internal/lua/src/dispatch.rs`) to `ToolCallOutcome::from_dispatch` (`crates/runvil-internal/lua/src/protocol/answer.rs`) for `CallSite::Script` in `crates/runvil-internal/engine/src/execute/scheduler/apply.rs`, for example by carrying it on `ToolDispatch` and adding an `Option<String>` parameter to `from_dispatch`. The model-facing nonce wrapping in `dispatch.rs` is unchanged, and `CallSite::Model` / `Batch` keep resuming as `ToolCallOutcome::Plain` with the flattened, wrapped text.
- When structured JSON text is present, it takes precedence over the binding's Plain or Structured kind: first reject text longer than 8,388,608 bytes (the `INPUT_LIMIT` of `crates/runvil-internal/lua/src/json/decode.rs`; share the constant rather than duplicating it) with an `Error::Tool` naming the alias, required limit, and actual length; then parse it, raising `Error::Tool` naming the alias and the parse error on failure (retain the parse error as the cause); then resume the script with the value converted by the same `lua.to_value` conversion `json.decode` uses, so JSON null becomes `json.null` and arrays carry the `json.decode` array metatable. The resumed value is the decoded table; text inside it is not wrapped or encoded (D4, D11).
- When structured JSON text is absent, keep today's behavior exactly: Plain returns the string, Structured returns parsed JSON (and still fails on wrapped untrusted text).
- A tool error (including MCP `isError: true`, which plugin-mcp already turns into a `ToolError`) still raises in Lua as today (D7).
- Update the `tools.call` docs in `crates/runvil-internal/lua/src/tools.rs`, `crates/runvil-internal/lua/src/handles.rs`, and the coroutine shim (`crates/runvil-internal/lua/src/coro.rs` or its Lua source): a tool that attaches structured JSON text returns a decoded table; that table carries raw untrusted text, and a script forwarding any of it to a model must wrap it with the existing `untrusted(s)` global. Engine crate docs say "the caller" and never mention the Host.
- Lua answer tests in `crates/runvil-internal/lua/src/protocol/tests/answer.rs`: structured JSON text decodes with `json.null` for nulls and the array metatable on arrays; text over 8,388,608 bytes raises the size error; text that does not parse raises the parse error naming the alias; an output without structured JSON text keeps the Plain and Structured behavior (keep `from_dispatch_classifies_by_the_declared_output_kind` passing).
- Engine scheduler test: a script-issued call whose untrusted output carries structured JSON text resumes with the decoded table, while a model-issued call to the same tool yields the wrapped text.

</step-3>

<step-4>

### Step 4: plugin-mcp attaches the CallToolResult

- Component: `plugin-mcp-producer`
- Tests: `cargo nextest run --locked -p plugin-mcp --all-features`

- In `crates/plugin-mcp/src/result.rs`, `render` keeps building today's flattened text (`block_text`, `text_of` joined with `"\n\n"`) and `ToolOutput::untrusted(text)`, and also attaches `serde_json::to_string(&CallToolResult)` via `with_structured_json` on every successful output (rmcp `=3.5.1`). Fields follow the MCP wire form: `content` (text, `resource_link` with `uri`/`name`/`mimeType`, image and audio with base64 `data`, `resource` with `uri`/`mimeType`/`text` or `blob`), and `structuredContent`, `isError`, `_meta` when the server sent them. Binary data passes through as base64 (D8). Do not parse, merge, or reinterpret block text.
- `isError: true` still returns a Backend `ToolError` with the flattened text, or `the MCP tool reported an error and gave no text`.
- Update the plugin-mcp crate docs in `crates/plugin-mcp/src/lib.rs` to state that scripts receive the `CallToolResult` as a table and the model receives the flattened, wrapped text.
- Tests in `crates/plugin-mcp/src/result-tests.rs`: for a text block plus a resource_link block, the structured JSON text parses to the wire JSON and the flattened text is byte-identical to today's; `structuredContent` appears in the structured JSON text when present; `isError: true` still returns a Backend `ToolError`.
- Keep the untrusted-envelope test in `crates/plugin-mcp/tests/it/main.rs` (`a_call_sends_the_original_tool_name_and_returns_untrusted_text`) passing.

</step-4>

<step-5>

### Step 5: End-to-end repro and exit gates

- Component: `end-to-end`
- Tests: `cargo nextest run --locked -p workshop-server -E 'test(mcp_result_blocks)'`, `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`, `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`, `CARGO_BUILD_WARNINGS=deny cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`, `CARGO_BUILD_WARNINGS=deny cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`, `cargo check -p gateway --no-default-features`, `cargo fmt --all --check`, `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`, `RUSTDOCFLAGS="-D warnings" cargo doc -p runvil --no-deps`, with `CARGO_BUILD_WARNINGS` unset `cargo +nightly-2026-09-05 xtask api --check`, `cargo test -p build-xtask`, `node --test crates/workshop/ui/test/docs-claims.mjs`

- Add `crates/workshop/server/tests/it/mcp_result_blocks.rs` (registered in `tests/it/main.rs`; split into a `mcp_result_blocks/` directory only if it reaches three files). `crates/workshop/server` is the one crate the boundary checks allow to depend on both `harness` and `plugin-mcp`; install plugin-mcp the way `crates/workshop/server/src/agents.rs` does.
- A local fixture MCP server (no external network; never call `https://wg21.org/mailing/mcp`; model on `crates/plugin-mcp/tests/it/fixture.rs`) exposes `get_paper_download_url` returning the repro shape: one `text` block of JSON (for example `{"expires": …, "urls": […]}` for `{ papers = "P0260R20" }`) plus two `resource_link` blocks.
- Test: a Lua script runs `json.decode(tools.call(<mcp tool>, { papers = "P0260R20" }).content[1].text)` without string manipulation and reads `content[2].uri`; assert both values.
- Test: a `models.loop` call to the same tool, driven by a scripted model reply, yields a tool message equal to today's flattened, wrapped text (`<json>\n\n[resource: <uri>]\n\n[resource: <uri>]` inside the untrusted envelope).
- All Testing Plan tests and the `AGENTS.md` verification commands listed above pass.

</step-5>

</execution-plan>
