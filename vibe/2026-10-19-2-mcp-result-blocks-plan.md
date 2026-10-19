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
  - Output shape for script-issued calls to plugin-mcp tools: the MCP wire JSON of the `CallToolResult`, converted to Lua the same way `json.decode` converts JSON. The fields are:
    - `content`: an array of blocks, each with its wire fields. A text block is `{type="text", text=…}`. A resource link is `{type="resource_link", uri=…, name=…, mimeType=…, …}`. Image and audio blocks carry `type`, `mimeType`, and base64 `data`. An embedded resource is `{type="resource", resource={uri=…, mimeType=…, text=… or blob=…}}`.
    - `structuredContent`: present when the server sent it.
    - `isError`: present when the server sent it.
    - `_meta`: present when the server sent it.
  - Field names follow the MCP wire form (camelCase), so they match the MCP specification.
  - JSON null becomes `json.null`, and JSON arrays carry the same array metatable that `json.decode` results carry.
  - Text inside blocks is the server's text, not wrapped and not encoded.
- States and validation:
  - Script-issued calls to tools whose output carries no structured payload keep today's behavior: a Plain binding returns a string, and a Structured binding returns parsed JSON.
- Errors and recovery:
  - A result with `isError: true` still raises the tool error in Lua, as today. The message is the flattened text, or `the MCP tool reported an error and gave no text`.
  - A structured payload larger than 8,388,608 bytes when serialized (the `json.decode` input cap) raises a Lua tool error naming the tool alias rather than converting. The model-facing text is unaffected.
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

The Plugin output type gains an optional structured payload next to its text. plugin-mcp fills it with the serialized `CallToolResult`. The Lua dispatch path returns that payload as a table to the calling script, while model-facing paths keep using the text. The answer record persists the payload so the record still holds everything a script could observe.

- Architecture:
  - Plugin-to-Engine output stays one in-process `ToolOutput` value (`crates/runvil-internal/types/src/tools/output.rs`). It is returned from `Plugin::call` (`crates/runvil-plugin/src/plugin.rs`).
  - Script-issued bound calls resume through `ToolCallOutcome::from_dispatch` (`crates/runvil-internal/lua/src/protocol/answer.rs`). Model-issued and batched calls (`CallSite::Model` / `Batch`) resume as `ToolCallOutcome::Plain` and never consult a structured payload (`crates/runvil-internal/engine/src/execute/scheduler/apply.rs`).
- Modules and interfaces:
  - `ToolOutput` gains an optional structured payload (`serde_json::Value`). It has a builder (for example `with_structured(value)`) and a getter. Text and trust keep their current constructors and meaning. `ToolOutput` is already `#[non_exhaustive]` with private fields, so existing Plugins compile unchanged.
  - Contract on the payload, documented on `ToolOutput`: a Plugin that attaches a payload to any output of a tool attaches one to every successful output of that tool. This keeps the Lua return type fixed per tool.
  - plugin-mcp `render` keeps building today's flattened text and trust, and also attaches `serde_json::to_value(&CallToolResult)` as the payload (rmcp `=3.5.1`, `crates/plugin-mcp/Cargo.toml`).
  - In the Lua crate, a script-issued bound call whose dispatched output carries a payload resumes with the payload converted to a Lua value. The conversion is the same `lua.to_value` that `json.decode` uses (`crates/runvil-internal/lua/src/json/decode.rs`), after the size check. This takes precedence over the binding's Plain or Structured kind.
- File and public API changes:
  - Public: the new `ToolOutput` builder and getter. `crates/runvil/public-api.txt` is regenerated through `xtask api`.
  - Documentation: the `tools.call` docs in `crates/runvil-internal/lua/src/tools.rs`, `crates/runvil-internal/lua/src/handles.rs`, and the coroutine shim; the plugin-mcp crate docs in `crates/plugin-mcp/src/lib.rs`; and the `Plugin::call` docs.
- Data, persistence, failure, security, and privacy constraints:
  - `ToolAnswerRecord` (built in `crates/runvil-internal/engine/src/execute/run/effect.rs`) gains the payload as an optional field. The field is omitted from JSON when absent, so records of other tools serialize byte-identically to today.
  - The record has no format version, and replay compatibility is handled by run flags (`crates/runvil-internal/types/src/replay.rs`). The new behavior is driven by the Plugin's output, not by an Engine behavior switch, so no new flag is added.
  - `Event::ToolResult` keeps carrying the flattened, wrapped text and gains no payload.
  - The model-facing nonce wrapping is unchanged (`crates/runvil-internal/lua/src/dispatch.rs`).

</implementation-contract>
<verification-contract>

## Testing Plan

Tests use fixture MCP servers and in-process Lua runs; no external network. They cover the payload's construction in plugin-mcp, its conversion in Lua, and the unchanged model-facing path.

- Unit:
  - plugin-mcp `result-tests.rs`: for a text block plus a resource_link block, the payload equals the wire JSON and the flattened text is unchanged.
  - plugin-mcp `result-tests.rs`: `structuredContent` appears in the payload when present.
  - plugin-mcp `result-tests.rs`: `isError: true` still returns a Backend `ToolError`.
  - Lua answer tests: a payload converts with `json.null` for nulls and the array metatable on arrays.
  - Lua answer tests: an oversized payload raises the size error.
  - Lua answer tests: an output without a payload keeps the Plain and Structured behavior.
- Integration and end-to-end:
  - A fixture MCP server returning the repro shape, a JSON text block plus two resource_link blocks. A Lua script runs `json.decode(tools.call(...).content[1].text)` and reads `content[2].uri`.
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
  - D5. The structured payload rides on `ToolOutput`, generic for any Plugin. plugin-mcp is the only producer in this plan. This keeps the Plugin boundary as one value and avoids a plugin-mcp special case in the Engine.
  - D6. The payload is chosen at call time from the output, not declared in `ToolInfo`, under the every-output contract on `ToolOutput`. This adds the least public API.
  - D7. `isError: true` still raises in Lua, matching today's behavior, so error handling in existing scripts keeps working.
  - D8. Binary block data (image and audio `data`, resource `blob`) passes through as base64. The 8,388,608-byte cap matches `json.decode`'s input cap.
  - D9. The user delegated every technical choice in this plan that the user did not state: the size cap, error behavior, field naming and null conversion, base64 passthrough, record persistence, the every-output contract, the unchanged `Event::ToolResult`, the test list, the operational limits, and the exit criteria. The user's words: "i don't know rust, do whatever is needed here".
- Rejected alternatives:
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

Four components in dependency order:

1. **Plugin output payload** comes first because every other component reads the payload it defines. It is useful alone as a generic Plugin API addition.
2. **plugin-mcp result payload** follows because it produces the payload, and it ships alone as a richer plugin-mcp output that no consumer yet reads.
3. **Engine payload handling** follows because it consumes the payload. Its two pieces are built sequentially: script resumption (Step 3) and then record persistence (Step 4). Neither piece depends on the other, but resumption carries the user-visible behavior, so it lands first, and splitting them keeps each step's tests focused.
4. **End-to-end verification** comes last because it needs components 1 through 3 together, through a real Harness run in the Workshop server, which is the one crate that may depend on both `harness` and `plugin-mcp` under the boundary checks (`crates/build-xtask/src/product.rs`: Harness crates must not depend on Plugin crates, and Plugin crates may depend only on `runvil-plugin`).

Every step keeps these constraints: model-facing MCP text stays byte-identical, non-MCP results stay unchanged on every path, and the defined terms (Engine, Harness, Host, Plugin) keep their `AGENTS.md` meanings, with Engine crates saying "the caller". Every test runs against fixtures only: no network beyond the loopback, and no call to `https://wg21.org/mailing/mcp`.

<step-1>

### Step 1: Optional structured payload on ToolOutput [completed]

- Component: Plugin output payload
- Tests: `cargo nextest run --locked -p runvil-types -p runvil-plugin --all-features`; `RUSTDOCFLAGS="-D warnings" cargo doc -p runvil --no-deps`; `cargo +<nightly from crates/build-xtask/src/api/toolchain.rs> xtask api --check` with `CARGO_BUILD_WARNINGS` unset

- In `crates/runvil-internal/types/src/tools/output.rs`, add a private field `structured: Option<serde_json::Value>` to `ToolOutput`. The struct keeps `#[non_exhaustive]`, private fields, and `Clone, Debug, Eq, PartialEq`. `trusted` and `untrusted` keep their signatures and leave the field `None`.
- Add `#[must_use] pub fn with_structured(self, value: serde_json::Value) -> ToolOutput` (builder) and `#[must_use] pub fn structured(&self) -> Option<&serde_json::Value>` (getter).
- Document on `ToolOutput`:
  - The payload is what a calling script receives in place of the text. Model-facing deliveries always use the text, and the trust rules apply to the text only.
  - The contract is that a Plugin attaching a payload to any output of a tool attaches one to every successful output of that tool, so the script-facing return type stays fixed per tool.
  - The payload is not wrapped, so a script that forwards its text to a model must wrap it with `untrusted(s)`.
  - Keep the existing statement that untrusted text is nonce-wrapped before reaching model input.
- Update the `Plugin::call` docs in `crates/runvil-plugin/src/plugin.rs` with one bullet: the every-successful-output payload contract, linking `ToolOutput::with_structured`.
- Regenerate `crates/runvil/public-api.txt` with `cargo +<pinned nightly> xtask api --bless`, so it adds exactly the two new `ToolOutput` functions.
- Unit tests in `crates/runvil-internal/types/src/tools/tests.rs`:
  - `trusted` and `untrusted` outputs have no payload.
  - `with_structured` keeps text and trust and returns the payload from `structured()`.

</step-1>

<step-2>

### Step 2: plugin-mcp attaches the serialized CallToolResult [completed]

- Component: plugin-mcp result payload
- Tests: `cargo nextest run --locked -p plugin-mcp --all-features`

- In `crates/plugin-mcp/src/result.rs`, `render` keeps computing `text_of(result)` and the `isError` branch exactly as today. A result with `is_error == Some(true)` still returns `ToolError::message(...)` with `ToolErrorKind::Backend`, and the message is the flattened text or `the MCP tool reported an error and gave no text`. On success it returns `ToolOutput::untrusted(text).with_structured(payload)`, where `payload` is `serde_json::to_value(result)` (rmcp `=3.5.1` serializes the MCP wire form: `content`, `structuredContent`, `isError`, `_meta`, camelCase block fields).
- If `to_value` fails, return a `ToolError` with `ToolErrorKind::Other` and the message `the MCP tool result could not be serialized: <error>`. Keep the serde error as the source.
- Update the crate docs in `crates/plugin-mcp/src/lib.rs` with the following:
  - The model receives the flattened, untrusted-wrapped text.
  - A script's `tools.call` receives the `CallToolResult` wire form as a table, whose block text is raw and must be wrapped with `untrusted(s)` before it is forwarded to a model.
  - `isError` results still raise.
- Unit tests in `crates/plugin-mcp/src/result-tests.rs`:
  - The repro shape is one text block `{"expires": "2026-01-01T00:00:00Z", "urls": ["https://example.test/p0260r20.pdf"]}` plus one `resource_link` block (`uri` `https://example.test/p0260r20.pdf`, `name` `P0260R20`, `mimeType` `application/pdf`). Assert that the payload equals the hand-written wire JSON (`{"content":[{"type":"text","text":…},{"type":"resource_link","uri":…,"name":…,"mimeType":…}]}`, plus any field rmcp always emits, pinned as observed). Also assert that `text()` equals the literal `<json text>\n\n[resource: https://example.test/p0260r20.pdf]` and that trust is `Untrusted`.
  - A result with `structuredContent` `{"a": 1}` has `payload["structuredContent"] == {"a": 1}`.
  - A result with `isError: true` and text `boom` returns `Err` with kind `Backend` and message `boom`. With no blocks, the message is `the MCP tool reported an error and gave no text`.
  - Existing flattening tests stay green unchanged.
- Keep the existing integration test `a_call_sends_the_original_tool_name_and_returns_untrusted_text` (`crates/plugin-mcp/tests/it/main.rs`) passing unchanged.

</step-2>

<step-3>

### Step 3: Script-issued calls resume with the payload as a Lua table [completed]

- Component: Engine payload handling
- Tests: `cargo nextest run --locked -p runvil-lua -p runvil-engine -p runvil --all-features`; `node --test crates/workshop/ui/test/docs-claims.mjs`

- `crates/runvil-internal/lua/src/dispatch.rs`: `ToolDispatch` gains `structured: Option<serde_json::Value>`, taken from `output.structured()` in `prepare_dispatch`, with a getter and an `into_parts(self) -> (String, Option<serde_json::Value>)`. The following stay byte-identical to today:
  - the content (trusted verbatim, untrusted `nonce.wrap(text)`)
  - the `emitter.tool_result` event content (`Event::ToolResult` gains no payload)
  - `prepare_model_dispatch`'s result
- `crates/runvil-internal/lua/src/protocol/answer.rs`: `ToolCallOutcome::from_dispatch` gains a `structured: Option<serde_json::Value>` parameter:
  - When `Some`, it takes precedence over the binding's `ToolOutputKind`. If `serde_json::to_vec(&value)` is longer than 8,388,608 bytes, it returns `Error::Tool` with the message `tool {alias:?} returned a structured result of {actual} bytes; the limit is 8388608 bytes`. Otherwise it returns `ToolCallOutcome::Structured(value)`.
  - When `None`, Plain and Structured behave exactly as today, including the invalid-JSON error for an untrusted output on a Structured binding.
  - Share the limit by making `INPUT_LIMIT` in `crates/runvil-internal/lua/src/json/decode.rs` `pub(crate)` rather than duplicating `8_388_608`.
- `crates/runvil-internal/engine/src/execute/scheduler/apply.rs`: the `CallSite::Script` arm passes the dispatch's payload into `from_dispatch`. The `CallSite::Model` and `CallSite::Batch` arms keep resuming `ToolCallOutcome::Plain(content)` and ignore the payload.
- The existing `Structured` render path (`lua.to_value`, `crates/runvil-internal/lua/src/protocol/render.rs`) is the conversion. It is the same as `json.decode`'s, with JSON null becoming `json.null` and arrays carrying the list metatable.
- Docs:
  - Update the `tools.call` docs in `crates/runvil-internal/lua/src/tools.rs`, `crates/runvil-internal/lua/src/handles.rs`, and the coroutine shim `crates/runvil-internal/lua/src/__impl_coro.lua`. A tool whose output carries a structured payload returns it as a table converted as `json.decode` converts JSON. Its text is raw untrusted data, so a script forwarding it to a model must wrap it with `untrusted(s)`. Results over 8,388,608 bytes serialized raise. Other tools keep returning a string (Plain) or parsed JSON (Structured).
  - Engine crate docs say "the caller" and never name the Host.
- Unit tests in `crates/runvil-internal/lua/src/protocol/tests/answer.rs`:
  - `from_dispatch` with a payload on a Plain binding and on a Structured binding returns `Structured(payload)`.
  - A payload `{"a": null, "list": [1, 2], "empty": []}` resumed into Lua yields `json.null` for `a`, and `list` and `empty` carry the same metatable `json.decode('[]')` carries.
  - A payload with one string of 8,388,608 bytes raises the size error naming the alias and the actual byte count.
  - Without a payload, the existing `from_dispatch_classifies_by_the_declared_output_kind` cases still hold: Plain `raw` gives `Plain("raw")`, and Structured `not json` gives the invalid-JSON error.
- Unit tests in `crates/runvil-internal/lua/src/dispatch-tests.rs`:
  - An untrusted output with a payload yields wrapped content identical to the same output without a payload, and the payload is unchanged.
  - The `ToolResult` event content equals the wrapped text.
- Facade suite tests in `crates/runvil/tests/suite/effect.rs`, which step a run and answer its tool effects with canned outputs through `crates/runvil/tests/suite/support.rs`, cover the call sites:
  - A model-issued call to a tool whose output carries a payload resumes `Plain` with the wrapped text.
  - A script-issued call to the same tool resumes `Structured(payload)`.

</step-3>

<step-4>

### Step 4: Persist the payload in ToolAnswerRecord [completed]

- Component: Engine payload handling
- Tests: `cargo nextest run --locked -p runvil-engine --all-features`; `cargo +<nightly from crates/build-xtask/src/api/toolchain.rs> xtask api --check` with `CARGO_BUILD_WARNINGS` unset

- In `crates/runvil-internal/engine/src/execute/run/effect.rs`, add the field to `ToolAnswerRecord`:
  - `#[serde(default, skip_serializing_if = "Option::is_none")] pub structured: Option<serde_json::Value>`, documented as the structured payload the tool attached, which a calling script received.
  - Add a `#[must_use] pub fn with_structured(self, value: serde_json::Value) -> ToolAnswerRecord` builder, and keep `new` unchanged.
  - `EffectAnswer::record` copies `output.structured().cloned()` into the record.
  - Add no replay flag in `crates/runvil-internal/types/src/replay.rs`.
- Regenerate `crates/runvil/public-api.txt` with `cargo +<pinned nightly> xtask api --bless` for the new public field and builder.
- Tests in `crates/runvil-internal/engine/src/execute/run/effect-tests.rs`:
  - The existing records without a payload keep serializing byte-identically: `ToolAnswerRecord::new("done", Trusted)` serializes to exactly `{"text":"done","trust":"trusted"}`, and the `<html>` untrusted case serializes as before. Pin both as string literals.
  - A record with a payload serializes `structured` with sorted object keys, and the record round-trips exactly: `to_value`, `to_string`, and `from_str` yield an identical value.
  - A JSON record without `structured` deserializes with `structured: None`.
  - The record of an `EffectAnswer::ToolCall(Ok(output.with_structured(v)))` carries `structured == Some(v)`.

</step-4>

<step-5>

### Step 5: End-to-end MCP result table through a Harness run

- Component: End-to-end verification
- Tests: `cargo nextest run --locked -p workshop-server --all-features -E 'test(mcp)'`; `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`; `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`; `CARGO_BUILD_WARNINGS=deny cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`; `CARGO_BUILD_WARNINGS=deny cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`; `cargo check -p gateway --no-default-features`; `cargo fmt --all --check`; `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`; `RUSTDOCFLAGS="-D warnings" cargo doc -p runvil --no-deps`; `cargo +<nightly from crates/build-xtask/src/api/toolchain.rs> xtask api --check` with `CARGO_BUILD_WARNINGS` unset; `cargo test -p build-xtask`; `node --test crates/workshop/ui/test/docs-claims.mjs`

- Add a minimal Streamable HTTP MCP fixture server for the Workshop server's agent tests, modeled on `crates/plugin-mcp/tests/it/fixture.rs`, using the existing `axum` and `tokio` dependencies and bound to `127.0.0.1:0`:
  - Put it beside `crates/workshop/server/src/agents/tests.rs` as a test-only module `agents/mcp_fixture.rs`, wired from `tests.rs`.
  - It answers `initialize`, `tools/list` (one tool `get_paper_download_url`), and `tools/call`. The call returns the repro shape: one `text` block holding the JSON `{"expires":"2026-01-01T00:00:00Z","urls":["https://example.test/a.pdf","https://example.test/b.pdf"]}` plus two `resource_link` blocks for those URLs (`name`, `mimeType` `application/pdf`).
- In `crates/workshop/server/src/agents/tests.rs`, install `plugin_mcp::PLUGIN` under the Plugin id `papers` with `{ "url": <fixture url> }` through `with_plugins`/`HostContext::install`, and run prompts through the existing `run_prompt`-style helper with a `MemoryRecorder`. The prompts declare `papers` under `plugins:`.
- Test `a_script_decodes_the_json_block_of_a_multi_block_mcp_result`:
  - The Lua section runs `local r = tools.call("papers/get_paper_download_url", { papers = "P0260R20" })`.
  - It returns `json.decode(r.content[1].text).urls[2] .. " " .. r.content[2].uri .. " " .. r.content[3].type`.
  - The run outcome's text is `https://example.test/b.pdf https://example.test/a.pdf resource_link`.
- Test `a_model_loop_call_to_an_mcp_tool_receives_the_flattened_wrapped_text`:
  - A scripted `InferenceBroker` lists one model. Its first round requests `papers/get_paper_download_url` with `{"papers":"P0260R20"}`, and its second round replies `done`. It captures the tool message it receives.
  - Assert the following about the captured content:
    - It is the untrusted envelope (`<untrusted_input_<nonce>>` … `</untrusted_input_<nonce>>` after the preface).
    - Its inner body equals exactly `<json text>\n\n[resource: https://example.test/a.pdf]\n\n[resource: https://example.test/b.pdf]` (no `<` occurs, so encoding leaves it unchanged).
    - It contains no `"content"` key.
- The run uses no external network and finishes within the helper's existing 10-second timeout.
- This step closes the Testing Plan exit criteria, so all gates listed in Tests pass.

</step-5>

</execution-plan>
