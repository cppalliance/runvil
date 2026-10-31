---
name: model_only_call flag
overview: "`ToolInfo` gains a `model_only_call` flag, so a Pack can keep a tool in the catalog where Lua names and offers it, while a script's `tools.call` refuses it. The model still calls it, because the arguments a render shaped belong to the model that receives them."
todos:
  - id: flag
    content: "Add `model_only_call` to `ToolInfo`: field, builder, `false` in `new`, serde default, and the module doc exception"
    status: completed
  - id: binding
    content: Carry the flag on `ToolBinding` through `from_info`, and copy it in `scope::bind` so a render cannot clear it
    status: completed
  - id: refuse
    content: Refuse a script `tools.call` in `tool_route` with a new `Error::ModelOnlyToolCall`, classified as `RunErrorKind::Tool`
    status: completed
  - id: lua-surface
    content: Expose `model_only_call` on the Lua tool object, so a prompt reads it before offering the tool
    status: completed
  - id: tests
    content: Add the refusal, offerable, and model-still-calls tests, and update the exact-JSON tests the always-emitted key changes
    status: completed
  - id: gates
    content: Run the crate and workspace suites, clippy, fmt, rustdoc, build-xtask, the wording scan, and `xtask api --bless` for the facade listing
    status: completed
isProject: false
---

# The model_only_call flag

<product-contract>

## Product Requirements

A Pack may render a tool differently per model: different parameter names, a different schema, a different description. Lua has no way to read a schema, so no script can know what arguments such a tool takes. The tool must stay in the catalog anyway, because `tools.offer("std/replace")` has to name it. This plan adds the one flag that resolves the tension.

- Problem and users:
  - A model-shaped tool's arguments belong to the model the render served. A script that calls it writes JSON from a guess.
  - The users are Plugin authors, who need to declare the tool un-callable from Lua, and prompt authors, who need to read that before offering it.
- Goals:
  - Three facts stay independent for one tool: it is in the catalog, Lua may offer it, and Lua may or may not call it.
  - The flag lives on the catalog entry, where the Pack sets it, and a render cannot clear it.
  - The refusal is a normal Lua error the author can catch, not a run failure.
- Non-goals:
  - No tool becomes model-shaped here. Nothing in `plugin-std` sets the flag, and no definition gains a `render`.
  - No change to `Pack::render`, `RenderContext`, `PackInfo`, or the merge rules.
  - No change to the discovery tools or the Dynamic Set, which resolve as they do today.
  - No new Lua verb. The flag is a read-only field on a tool object already reachable through `tools.get`.
- Success criteria:
  - A script `tools.call` of a model-only tool fails with a typed error naming the id, and `pcall` catches it.
  - The same tool offered to the model is advertised and its call answers, which proves the flag gates the script alone.
  - `tools.offer` accepts the tool alone and with its Pack, so the flag never hides anything.
  - A render returning a catalog tool with the flag cleared still binds it model-only.
- Constraints:
  - The four flags on `ToolInfo` are independent questions about one tool, so they stay bools.
  - The key is always serialized, as `survives_stop` and `describes_tools` are, so a recorded run shows it.
  - Engine crate docs say "the caller", never the Host.
- Open questions: None.

## Functional Specification

A tool object in Lua gains a fifth field. A script that calls a model-only tool by id or by object gets an error it can catch. A model round is unaffected.

- Actors and workflows:
  - A Plugin author marks a tool with `ToolInfo::model_only_call(true)` in `Pack::tools`.
  - A prompt author reads `tools.get("std/replace").model_only_call` and decides whether to offer it.
  - A script calls `tools.call(id, args)` or `tools.call(object, args)`: both reach the same lookup, so both are refused.
  - A model round calls the tool by its wire name; the frozen sets resolve it and the call answers.
- Inputs and outputs:
  - The tool object's fields are `id`, `description`, `parameters`, `pack`, and `model_only_call`, all read-only, and no others.
  - The refusal's message is `tool "tools/replace" (tools/replace) is model-only: a render shapes its arguments for the model that receives them, so a script cannot call it; offer it with tools.offer and let the model call it`.
  - A Lua error table carries the kind tag `model_only_tool` and the `name` field.
- States and validation:
  - `ToolInfo::new` leaves the flag false, so every existing tool keeps its behavior.
  - A serialized `ToolInfo` without the key reads `false`, so a run log written before this change still loads.
  - `bind` copies the catalog's value onto a rendered tool with a catalog id, so a render that clears the flag changes nothing.
- Errors and recovery:
  - The refusal is a Lua error at the `tools.call` boundary, like an unbound id today.
  - An uncaught refusal ends the section as a tool failure under `RunErrorKind::Tool`.
- Security and privacy behavior:
  - No new data reaches the model or a script. The flag is the Pack's own declaration about its own tool.
  - The refusal names only the id and a name the caller already passed.
- Acceptance criteria:
  - `--workspace` and the workshop suites pass, with the crate suites for `runvil-types`, `runvil-lua`, and `runvil-engine`.
  - `cargo xtask api --check` passes, and the facade gains exactly the field and the builder.

</product-contract>
<implementation-contract>

## Technical Design

The change is one field carried through four layers that already carry the same three flags: `ToolInfo`, `ToolBinding`, the frozen-set merge, and the Lua tool object. The only new branch is in the script half of one lookup function.

- Architecture:
  - `ToolInfo` in `crates/runvil-internal/types/src/tools/info.rs` gains `pub model_only_call: bool` with `#[serde(default)]`, a builder, and `false` in `new`. The crate doc's claim that every catalog tool is callable from Lua now names the exception.
  - `ToolBinding` in `crates/runvil-internal/lua/src/bindings.rs` carries the flag and an accessor, copied in `from_info`.
  - `bind` in `crates/runvil-internal/engine/src/execute/scope.rs` copies the catalog's value onto a rendered tool, beside `structured_output`, `describes_tools`, and `survives_stop`.
  - `LuaToolHandle` in `crates/runvil-internal/lua/src/tools/userdata.rs` gains the field and a read-only getter.
- Modules and interfaces:
  - `Error::ModelOnlyToolCall { name, id }` in `crates/runvil-internal/engine/src/error.rs`, beside `UnboundToolCall`. It joins the `None` arm of `originating_error`.
  - `RunErrorKind::Tool` classifies it, in `crates/runvil-internal/engine/src/execute/error.rs`.
  - `runvil_lua::ErrorKind::ModelOnlyTool` renders it to Lua with the tag `model_only_tool`, joining `ALL`, `tag`, `from_tag`, and the two exhaustive matches in `crates/runvil-internal/lua/src/vm/run.rs` and `crates/runvil-internal/engine/src/error/convert.rs`.
  - `tool_route` in `crates/runvil-internal/engine/src/execute/scheduler/tool_call.rs` refuses in the script branch, after the catalog lookup, so the bare id and a tool object both pass through one check.
- File and public API changes:
  - `crates/runvil/public-api.txt` gains `ToolInfo::model_only_call` as a field and as a builder. `ToolInfo` is on the facade, so the listing is regenerated with `cargo +nightly-2026-09-05 xtask api --bless`.
  - The Lua error vocabulary gains one tag, which a prompt may branch on.
- Data, persistence, failure, security, and privacy constraints:
  - The key is always emitted, so a recorded `ToolInfo` shows the flag rather than leaving a reader to infer it. Three tests that pin exact JSON lines are updated, and the test that pinned a pre-`wire_name` line now asserts the absent flags read false instead.
  - No new dependency, no unsafe code, and no filesystem access.

</implementation-contract>
<verification-contract>

## Testing Plan

- Unit:
  - **Types:** the builder sets the flag and serde round-trips it; a line without the key reads false; the canonical form holds the key in sorted position.
  - **Binding:** `from_info` copies the flag, and a new info leaves it false.
  - **Lua surface:** the tool object reads `model_only_call` for a marked tool, and the field-set test still refuses a sixth field.
- Integration and end-to-end:
  - **Refusal** in `failures-script-tools.rs`: the id form fails with `ModelOnlyToolCall` naming the id, the object form fails the same way, and `pcall` catches it with the run continuing.
  - **Offer still works:** `tools.offer` accepts the model-only tool alone and through its Pack.
  - **Model path** in `offering.rs`: a section offers the tool, a scripted round calls it by wire name, and the `ToolCall` effect names the right id and alias.
- Regression, security, and performance:
  - The exact-JSON assertions in `types/src/tools/tests.rs` and `execute/run/effect-tests.rs` pin the serialized shape, so a later change to the key fails rather than drifting.
- Exit criteria, every command below passes:
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`, then `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`.
  - Both clippy runs with `CARGO_BUILD_WARNINGS=deny`, and `cargo fmt --all --check`.
  - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`, and the facade's own doc gate.
  - `cargo test -p build-xtask`, `node --test crates/workshop/ui/test/docs-claims.mjs`, and `cargo +nightly-2026-09-05 xtask api --check`.

</verification-contract>
<decision-record>

## Decision Record

- Decisions:
  - **A flag on the catalog entry, not a model-only tool minted inside a render.** Rationale: Lua must be able to name the tool to offer it, and a tool that exists only in a render's answer cannot be named at all. The catalog entry stays and the call is refused. Product owner: "Needs to be a flag on the Lua facing tool that says that the Lua is not allowed to call it."
  - **The render may not clear it.** Rationale: the catalog is the authority on what a Pack offers, and the other three flags already work this way. A render that could clear it would let a Pack grant itself script-callable status its own catalog denied.
  - **A typed error with its own Lua kind.** Rationale: an author needs to branch on "this is model-only" differently from "there is no such tool", and a distinct tag keeps the two apart in a caught error.
  - **Four bools, kept as bools.** Rationale: each answers a different question about one tool, so no state machine or enum expresses them. Clippy's `struct_excessive_bools` is expected with that reason rather than silenced.
  - **The key is always serialized.** Rationale: a recorded run should show the flag, and the cost is three exact-line tests.
- Rejected alternatives:
  - Minting a model-only tool inside `Pack::render`. Rejected: Lua cannot name it, so a prompt could never offer it individually.
  - One `callable: Callers` enum instead of a bool. Rejected: it reads as a state machine over exclusive states, and these flags are independent.
  - Failing the run instead of raising a catchable Lua error. Rejected: an author may want to offer the tool and let the model call it, which is the whole point.
- Assumptions, risks, and notes:
  - Nothing uses the flag yet. The first real consumer is `std/replace`, which is a separate change.
  - `ToolInfo` is on the facade, so this is a public-surface addition, though a purely additive one.

### Deferred and Out of Scope

- Deferred: `std/replace` marked model-only with a `render` that shapes its schema per model.
- Out of scope: any change to `Pack::render`, `RenderContext`, or `PackInfo`, and any change under `crates/plugin-std`.

</decision-record>
<project-survey>

## Project Survey

- Status: complete
- Build command: `cargo build -p runvil` (the workspace default members are `runvil`, `runvil-plugin`, `harness`, and `harness-gateway-client`).
- Focused test command pattern: `cargo nextest run --locked -p <crate> --all-features <test-name-filter>`; drop `--all-features` for `workshop`, `workshop-server`, and `workshop-server-api`.
- Component test command pattern: `cargo nextest run --locked -p <crate> --all-features`; structural checks: `cargo test -p build-xtask`.
- Full-suite test command: `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`, then `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`.
- Linter command: `CARGO_BUILD_WARNINGS=deny cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features` and the workshop run beside it; wording scan: `node --test crates/workshop/ui/test/docs-claims.mjs`.
- Formatter check command: `cargo fmt --all --check`.
- Docs command: `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`, plus `RUSTDOCFLAGS="-D warnings" cargo doc -p runvil --no-deps`; facade surface: `cargo +nightly-2026-09-05 xtask api --check` with `CARGO_BUILD_WARNINGS` unset.
- Test placement and naming conventions: unit tests sit beside their module in a `<stem>-tests.rs` sibling wired by `#[cfg(test)] #[path = "<stem>-tests.rs"] mod tests;`. Engine integration tests live under `execute/tests/`, one module per topic.
- Directory map: `crates/runvil-internal/` holds the Engine's private crates (`types`, `parser`, `lua`, `vfs`, `model-client`, `engine`) behind the `runvil` facade. `crates/runvil-plugin` is the Plugin API.
- Component boundaries: Engine crates depend on no gateway, workshop, or Harness crate, and `cargo test -p build-xtask` enforces it. `runvil-plugin` re-exports the `runvil-types` and `runvil-vfs` names a Plugin author needs, so a Plugin crate names no other runvil crate.
- Conventions summary: Rust 2024, workspace version 0.3.0, BSL-1.0; workspace lints inherited with pedantic denied and `unwrap_used`/`expect_used` denied outside tests; every Rust file at most 500 lines, enforced by each crate's `build.rs`; capitalized defined terms checked by `docs-claims.mjs`; model-readable error messages naming required versus actual; no doctests anywhere.

</project-survey>
<execution-plan>

## Execution Instructions

Each step landed as one commit-sized change, and every step's checks passed before the next began.

<step-1>
Add `model_only_call` to `ToolInfo` in `crates/runvil-internal/types/src/tools/info.rs`: the field with `#[serde(default)]`, the builder, `false` in `new`, and the crate doc's exception. Expect `clippy::struct_excessive_bools` at the fourth bool and record it with `#[expect]` and the reason that the flags are independent.
</step-1>

<step-2>
Carry the flag on `ToolBinding` in `crates/runvil-internal/lua/src/bindings.rs` through `from_info`, with an accessor, and copy it in `bind` in `crates/runvil-internal/engine/src/execute/scope.rs` so a render cannot clear the catalog's declaration.
</step-2>

<step-3>
Add `Error::ModelOnlyToolCall { name, id }` in `crates/runvil-internal/engine/src/error.rs`, classify it `RunErrorKind::Tool` in `execute/error.rs`, and add `runvil_lua::ErrorKind::ModelOnlyTool` with its tag `model_only_tool` across `error-value.rs`, `vm/run.rs`, and `error/convert.rs`.
</step-3>

<step-4>
Refuse in `tool_route`'s script branch after the catalog lookup, so the bare id and a tool object both pass through the one check. Use a `let ... else` on the lookup rather than `map(..).unwrap_or_else(..)`, which clippy refuses.
</step-4>

<step-5>
Expose the flag on the Lua tool object in `crates/runvil-internal/lua/src/tools/userdata.rs`, and update the module doc and the field-set test that pins exactly five fields.
</step-5>

<step-6>
Add the tests: the two refusal forms, the `pcall` catch, the two offer forms, the binding copy, the serde round trip, and a model round that advertises and calls the tool. Update the three exact-JSON assertions the always-emitted key changes.
</step-6>

<step-7>
Run every gate in the Testing Plan, blessing the facade listing with `cargo +nightly-2026-09-05 xtask api --bless` and confirming `--check` afterwards.
</step-7>

</execution-plan>
