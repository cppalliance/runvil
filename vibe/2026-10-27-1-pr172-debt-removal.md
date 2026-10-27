---
name: Research prompt tool names
overview: "Debt collection over runvil 951e4bb9b..1d6937459 (the 27 merged commits of pull request 172) kept one finding, C-1: the shipped research prompt tells the model to call tools by ids it never sees. At the user's request the plan also fixes six rejected candidates: one frozen-set enum instead of two, a guard on the discovery ids, an all-or-nothing admission pass, the dead counts parameter, a stale doc example, and the guide's run-limits claim."
todos:
  - id: c1-research-prompt
    content: "C-1: add the shipped.rs regression test (no user message the model reads names a tool id), confirm it fails, then rewrite prompts/research-person.md lines 32-33 to describe the tools by role"
    status: pending
  - id: set-enum-and-discovery-guard
    content: "D1-11, D1-19, D1-23: OfferMode gains the Static Set and Dynamic Set Display and replaces FrozenSet; a new runvil-lua scope-tests.rs ties every DiscoveryTool id to a reserved Pack"
    status: pending
  - id: admission-all-or-nothing
    content: "D1-24: regression tests that a failed admission pass leaves its queues and holders as it found them; run the pass on locals and write back only on success"
    status: pending
  - id: drop-dispatch-counts
    content: "D1-27: drop the always-None counts parameter, its increment, and both too_many_arguments expects from prepare_dispatch and prepare_model_dispatch"
    status: pending
  - id: display-chain-doc
    content: "D1-31: replace the stale Engine LuaRuntime example in display_chain.rs with an accurate one, or drop it"
    status: pending
  - id: guide-run-limits
    content: "D1-34: in runvil-docs, 16-limits-and-errors.md:125 says the Harness passes no limits today, so every run observes the defaults; commit there"
    status: pending
isProject: false
---

# Debt removal for 951e4bb9b..1d6937459: research prompt tool names and six follow-ups

<product-contract>

## Product Requirements

- Scope and target work
  - Repository: `c:\Users\Vinnie\cursor\runvil`. Guide repository for D1-34: `c:\Users\Vinnie\cursor\runvil-docs`, its own git repository.
  - Target: `951e4bb9b..1d6937459`, the 27 commits of pull request 172 as merged into `cppalliance/runvil` master. They come from three plans: `vibe/2026-10-25-1-tools-kernel-redo.md`, `vibe/2026-10-26-1-engine-slop-cleanup.md`, and `vibe/2026-10-26-2-engine-scheduler-split.md`.
  - Baseline: `951e4bb9b`, the upstream base. Endpoint and disposition ref: `1d6937459`. Worktree: excluded, and it was clean.
  - The merge rebased the commits, so hashes cited inside the three plans name pre-rebase commits.
  - The collection kept one finding, C-1. The user then chose six rejected candidates to fix as well: D1-11 and D1-19 (one cause), D1-23, D1-24, D1-27, D1-31, and D1-34. D1-27 and D1-34 predate the target, and D1-31 sits in a file the target didn't touch, so those three expand the scope beyond debt the target added.
- Cleanup goals and non-goals
  - Goals:
    - C-1: the shipped research prompt's model-facing prose names no tool by its id.
    - D1-11 and D1-19: one enum names a section's two frozen sets, and the engine's `error::sets` and `execute::scope` modules no longer import each other.
    - D1-23: a test ties every discovery tool id to a reserved Pack id.
    - D1-24: an admission pass that fails on an internal error leaves admission exactly as it found it.
    - D1-27: the Lua dispatch entry points lose the `counts` parameter that production never passes.
    - D1-31: `display_chain.rs`'s doc names an error variant that exists.
    - D1-34: the guide stops saying a given run's limits can differ from the defaults.
  - Non-goals:
    - No behavior change except C-1 (what the model is told) and D1-24 (the state admission holds after an internal error).
    - No change to `crates/runvil/public-api.txt`.
    - `tools.offer({"web/search", "web/fetch"})` on `prompts/research-person.md:21` stays unchanged, because Lua names tools by id and that line is correct.
    - No work on the MCP discovery overlap (D1-1) or any other rejected candidate.
    - No Harness setting for run limits; D1-34 corrects the guide instead.
- Success criteria
  - The C-1 and D1-24 regression tests fail at `1d6937459` and pass after their fixes.
  - The D1-23 guard passes.
  - Every merge error message keeps its exact text after D1-11 and D1-19.
  - `crates/runvil/public-api.txt` is unchanged, and the workspace suites, clippy with warnings denied, fmt, the rustdoc gates, `cargo test -p build-xtask`, and the docs-claims scan pass.

## Functional Specification

### Debt Inventory

- Debt added
  - C-1 (introduced): `prompts/research-person.md` tells the model to call tools by ids it never sees.
    - Evidence: commit `3599b20bf` ("Ask Packs to render their tools and route calls by Pack") changed lines 32-33 from `web_search` and `web_fetch` to `web/search` and `web/fetch`. Its stated reason was that "a render may rename a tool for each section's model".
    - What the model actually sees: `tool_schema` (`crates/runvil-internal/engine/src/execute/scope.rs:486-496`) builds each advertised schema from the binding's wire name, description, and schema only. `plugin-web` builds both tools with `ToolInfo::new` and sets no `wire_name` (`crates/plugin-web/src/web.rs:107-112`), and no shipped Pack overrides `Pack::render`. So the model's tool list names `web_search` and `web_fetch`, which `crates/runvil/tests/suite/shipped.rs:43-48` already pins. The fetch tool's own errors begin `web_fetch:` (`crates/plugin-web/src/fetch.rs:355`), so the model's context mixes both spellings.
    - Contract contradicted: the tools-kernel-redo plan's rule "A wire name is the name the model sees for a tool. The tool id is the stable name" (`vibe/2026-10-25-1-tools-kernel-redo.md:48`). No plan step called for this prompt edit.
    - Impact: a model that follows the prose literally calls `web/search`. Neither frozen set holds that name, so `tool_calls` (`crates/runvil-internal/engine/src/execute/scheduler/chat.rs:436-450`) returns `Error::OutOfScopeToolCall` and reports `TOOL_CALL_FAILED`. The prompt's Lua has no `pcall`, so the section fails. How often a model takes the prose literally depends on the model.
    - Reversal cost: a two-line prose edit.
    - Target state: the prose describes the two tools by their role, so it stays true under any render.
- Cheap fixes, chosen by the user from the rejected candidates
  - D1-11 and D1-19 (one cause): two enums name a section's two frozen sets.
    - Evidence: `FrozenSet` (`crates/runvil-internal/engine/src/execute/scope.rs:41-56`) and `OfferMode` (`crates/runvil-internal/lua/src/scope.rs:92-97`) have the same two variants, `Static` and `Dynamic`, and the same meaning. Commit `1fccbb70f` placed `FrozenSet` in `execute::scope` so that `error/sets.rs:8` could import it, while `execute/scope.rs:18` imports `crate::error::{SetRule, SetTool}`, so the two modules import each other.
    - Consequence: two enums to keep in step, and a module cycle inside the engine crate.
    - Target state: one enum, `OfferMode`, and no cycle.
  - D1-23: nothing ties the discovery tool ids to the reserved Pack list.
    - Evidence: `DiscoveryTool::id` spells `dynamic/search` and `dynamic/call` (`crates/runvil-internal/lua/src/scope.rs:210-213`). `RESERVED_PACK_IDS` spells `"dynamic"` (`crates/runvil-internal/types/src/packs.rs:31`). The types crate's test (`crates/runvil-internal/types/src/packs-tests.rs:70-86`) checks a literal `dynamic/search`, because that crate cannot depend on runvil-lua.
    - Consequence: renaming one spelling without the other would let a vendor Pack take the discovery namespace, with no test to catch it.
    - Target state: a runvil-lua test that each `DiscoveryTool` id's Pack is reserved.
  - D1-24: a failed admission pass loses admission's queued state.
    - Evidence: `Admission::plan` (`crates/runvil-internal/engine/src/execute/scheduler/admission.rs:161-200`) takes the resume queue and then the spawn queue with `std::mem::take`. It returns `Error::Internal` at `:175` or `:192` without writing back what it took, after earlier admissions in the same pass have already changed `holding` and `released`. `is_queued` (`:146`) reads only the spawn queue, and `end_backing` treats a task chain that isn't queued as admitted.
    - Consequence: after a `:192` error, a spawn that never started can report `TaskAbandoned` without `TaskStarted`, against the doc that a never-admitted task reports no terminal event. Both errors assert scheduler invariants, so only a scheduler bug reaches this, and the run ends on it.
    - Target state: a pass that fails leaves both queues, `holding`, and `released` exactly as they were before it started.
- Exposed and pre-existing debt, added to scope by the user
  - D1-27: the Lua dispatch entry points take a `counts` parameter production never passes.
    - Evidence: `prepare_dispatch` (`crates/runvil-internal/lua/src/dispatch.rs:127`) and `prepare_model_dispatch` (`:206`) take `counts: Option<&ToolCallCounts>` and increment it when it is `Some`. Their only production callers, `crates/runvil-internal/engine/src/execute/scheduler/apply.rs:306` and `:319`, pass `None`, as they did before the target. Unit tests pass `Some` (`crates/runvil-internal/lua/src/dispatch-tests.rs`, `crates/runvil-internal/lua/src/tools/tests-parameters.rs:156`). Each function carries `#[expect(clippy::too_many_arguments)]`, because the target's `vendor` parameter brought it to eight.
    - Target state: neither function takes `counts`, and neither needs the lint expectation.
  - D1-31: a doc names an error variant that no longer exists.
    - Evidence: `crates/harness-internal/runner/src/display_chain.rs:17` cites "the Engine's `LuaRuntime` variant" as an error that copies its source's text into its own message. Commit `1fccbb70f` merged that Engine variant into `Error::Lua`. runvil-lua still has its own `Error::LuaRuntime` (`crates/runvil-internal/lua/src/error.rs:97`).
    - Target state: the example names a variant that exists and does copy its source's text, or the doc gives no example.
  - D1-34: the guide says a run's limits can differ from the defaults.
    - Evidence: `runvil-docs/src/language/16-limits-and-errors.md:125` reads "The Harness that runs a prompt can replace any of the four defaults, so the limits a given run observes can differ from the ones in the table." `RunContext::with_limits` has one caller, `crates/runvil-internal/engine/src/test_support/scenario.rs:212`, so no Harness code sets limits and every run observes the defaults.
    - The guide's other run-limit sentences say who may change a limit (the Harness side, never the prompt), and they stay true: `16-limits-and-errors.md:140`, `:146-148`, `:196`, `:200`, `:248`, and `:254`; `05-lua-environment.md:2343`; `14-fanout.md:1196` and `:1281`; and `17-quick-reference.md:14` and `:275`.
    - Target state: `:125` says what every run observes today.
- Rejected candidate counts (35)
  - Residual-but-acceptable (12), each reserved by a plan or protected by an existing check:
    - D1-1: the Engine's `GetDynamicTools` and `CallDynamicTool` beside the MCP Pack's own discovery pair. The MCP pair predates the target (no change under `crates/plugin-mcp` in the diff), the two pairs share no contract, and the freeze that blocks retiring the MCP pair already existed at the baseline.
    - The others: the runvil-lua `handles` and `scope` module cycle, the pass-through `Pack::render` hook, the unchecked public `ToolInfo::wire_name` (which the merge validates), `RenderedTools` with public fields, the test-only `with_request_limits` callers, the `render_info` fallback, the derived status enums, the test-only chain bound, the defensive `HostRunContext::render` fallback, the Harness routing by Pack, and the deferred `RV_PACK_RENDER` constant.
  - Weak/speculative (8): no demonstrated consequence. These cover persisted records with no replay reader, an uncapped discovery listing, the `fmt::Debug` check's unmatched spellings, a `bool` H1 flag, an unused public builder, the H1 chain's unread `Walk`, the trusted discovery refusals, and the guide edits that live in another repository.
  - False (15): the four `Repairs:` claims hold and each has a regression test. Several candidates are moot or resolved at the endpoint, including the limits and positions that were each reshaped twice and are now settled. The others were refuted by reading the code: the origin model, the interrupt classification, the log producer deadline, task-id uniqueness, truncation at a character boundary, substitution rendering, and `sys` key omission.

</product-contract>
<implementation-contract>

## Technical Design

- C-1: only the model-facing prose of `prompts/research-person.md` (lines 32-33) changes, plus one integration test in `crates/runvil/tests/suite/shipped.rs`.
  - The contract the test pins: across every `EffectRecord::Chat` record of a run, no user message the model receives contains an offered tool's id (`web/search`, `web/fetch`). The model knows tools only by their wire names.
  - `EffectRecord::Chat` carries `messages: Vec<Value>`, the request's wire-form messages after the first `keep` (`crates/runvil-internal/engine/src/execute/run/effect-record.rs:64-80`).
- D1-11 and D1-19: `OfferMode` is the one frozen-set enum.
  - `OfferMode` (`crates/runvil-internal/lua/src/scope.rs:92`) gains `impl std::fmt::Display` writing `Static Set` and `Dynamic Set`. That's the exact text `FrozenSet`'s `Display` writes today (`crates/runvil-internal/engine/src/execute/scope.rs:48-55`), so every merge error message stays byte-identical.
  - `FrozenSet` goes. `execute/scope.rs` (`set_of`, `set_mut`, `lay_out`, the set checks, and the merge's pairing at `:459-460`) and its tests (`execute/scope-tests.rs`, `execute/scope-tests-render.rs`) use `OfferMode`, reached through `crate::lua`, which already re-exports it (`crates/runvil-internal/engine/src/lua.rs:8`).
  - `error/sets.rs` imports `runvil_lua::OfferMode` directly, as the error module already does for other runvil-lua types (`error/convert.rs:8`, `error/value.rs:15`). That leaves no module cycle: `lua.rs` imports only from runvil-lua.
  - runvil-lua is an internal crate, so no public API changes.
- D1-24: `Admission::plan` runs its pass against local copies of both queues and of the `holding` and `released` sets, and writes them back to `self` only when the pass succeeds.
  - Which passes fail, and their messages, stay the same. Only the state left after a failure changes.
  - This doesn't validate every entry up front: checking entries the pass would not admit would make some passes fail that succeed today.
- D1-27: `prepare_dispatch` and `prepare_model_dispatch` (`crates/runvil-internal/lua/src/dispatch.rs`) lose the `counts` parameter, the increment, the `# Errors` lines about the counts' own error, and both `#[expect(clippy::too_many_arguments)]` attributes. Seven parameters no longer trip the lint, and an unfulfilled `expect` would fail clippy.
  - `ToolCallCounts` itself stays, because the engine counts calls before dispatch.
- D1-31 and D1-34: documentation only.

</implementation-contract>
<verification-contract>

## Testing Plan

- C-1 regression: add a test to `crates/runvil/tests/suite/shipped.rs`, for example `the_shipped_research_prompt_names_no_tool_by_its_id_in_what_the_model_reads`.
  - It reuses the existing test's scenario: the two-tool `ToolCatalog`, the scripted model `calls("web_search", ...)` then `text("a factual summary")`, and `.args("Ada Lovelace")`. Extract that setup into a helper both tests share.
  - Assertion: for every `EffectRecord::Chat { messages, .. }` in `trace.effects`, and every message whose `"role"` is `"user"`, `serde_json::to_string(message)` contains neither `web/search` nor `web/fetch`.
  - Why it fails at `1d6937459`: the first round's user message is the section prose (`messages:user(prose)`), which contains `` `web/search` `` (`prompts/research-person.md:32`) and `` `web/fetch` `` (`:33`).
  - Only user messages are checked, so ids that tool results or call records carry can't make it fail by accident.
- D1-24 regressions, in `crates/runvil-internal/engine/src/execute/scheduler/admission-tests.rs`. It's a child module of `admission.rs`, so it can read the private `resuming` and `holding` fields.
  - New test, for example `a_pass_that_fails_on_an_internal_error_leaves_admission_as_it_found_it`:
    - Build `Tree::new()`, then `root = tree.root(2)`. A limit of 2 lets the pass admit the first entry and still reach the second.
    - `task = spawn(&mut admission, &mut tree, root)`, then `call = tree.call(root)`, then `admission.enqueue_spawn(call, SpawnRecord { input: None, section: "not a task".to_owned() })`.
    - Assert that `admission.plan(&tree.view())` matches `Err(Error::Internal { .. })`, which shows the pass reached the call chain. Then assert `admission.is_queued(task)` and `!admission.holding.contains(&task)`.
    - At `1d6937459` both later assertions fail. The pass admitted `task` into `holding` and took the spawn queue before it returned at `:192`.
  - Extend `a_resumption_whose_slots_belong_to_no_admitted_task_is_an_internal_error` (`:250`) with `assert!(admission.resuming.contains(&queued))` after the error. It fails at the endpoint because the pass took the resume queue before it returned at `:175`.
- D1-23 guard: a new `crates/runvil-internal/lua/src/scope-tests.rs`, wired from `scope.rs` with `#[cfg(test)] #[path = "scope-tests.rs"] mod tests;`.
  - For each `DiscoveryTool::ALL`, parse `tool.id()` into a `ToolId` and assert `id.pack().is_reserved()` (`crates/runvil-internal/types/src/tools/ids.rs:60`, `crates/runvil-internal/types/src/packs.rs:89`).
  - It passes at the endpoint, because it guards a future rename rather than a present failure.
- D1-11 and D1-19: the engine's `execute/scope-tests.rs` and `execute/scope-tests-render.rs` switch to `OfferMode`, and every merge-error message assertion keeps its expected text. The compiler's exhaustive matches cover the rest.
- D1-27: the unit tests that pass `Some(&counts)` (`crates/runvil-internal/lua/src/dispatch-tests.rs` and `crates/runvil-internal/lua/src/tools/tests-parameters.rs:156`) drop the argument. An assertion that only checks the dropped increment goes, and a test whose name says it counts, such as `prepare_dispatch_wraps_a_canned_untrusted_output_counts_it_and_reports_it`, is renamed.
- D1-31 and D1-34: checked by reading. The rustdoc gates confirm D1-31's doc still builds.
- Focused, from the runvil root in PowerShell:
  - `cargo nextest run --locked -p runvil --all-features shipped`
  - `cargo nextest run --locked -p runvil-engine --all-features admission scope`
  - `cargo nextest run --locked -p runvil-lua --all-features dispatch scope parameters`
- Exit, from the runvil root in PowerShell:
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `cargo fmt --all --check`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`
  - `cargo test -p build-xtask`, because a new test file is wired.
  - `node --test crates/workshop/ui/test/docs-claims.mjs`
  - `Remove-Item Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue; cargo +nightly-2026-09-05 xtask api --check`, which confirms the public API listing didn't change.

</verification-contract>
<decision-record>

## Decision Record

- Reversible decisions and consequences
  - C-1: describe the tools by role, not by any spelling, for example "Search the web with several targeted queries" and "Fetch the few best results". This keeps the prose true under any render, which is what `3599b20bf` intended. The model matches the tools by their descriptions instead of by name.
  - C-1: pin the contract with a test on what the run sends the model, the `Chat` effect's user messages, not a check on the prompt's source text. The test checks observable Engine output.
  - D1-11 and D1-19: keep `OfferMode`, the runvil-lua name, because offers are defined in that crate and the engine already imports it. Its new `Display` carries `FrozenSet`'s text over unchanged.
  - D1-11 and D1-19: `error/sets.rs` imports `runvil_lua::OfferMode` directly rather than through `crate::lua`, so the error module gains no new intra-crate import and no new cycle can form.
  - D1-24: write the pass's state back only on success, rather than validating every entry first. Validating up front would also check entries the pass would not admit, so it would change which passes fail.
  - D1-27: drop `counts` rather than wire it in. The engine already counts every call before dispatch, so the `Some` path had no production caller before or after the target.
  - D1-31: name an existing variant that copies its source's text, such as runvil-lua's `Error::LuaRuntime` if its message does, or drop the example clause.
  - D1-34: correct only `16-limits-and-errors.md:125`. The other run-limit sentences say who may change a limit, which stays true.
  - D1-34: the runvil-docs commit names runvil `1d6937459`, the commit whose code the corrected sentence describes, because D1-34 has no runvil change of its own and `1d6937459` is already upstream, so no rebase of this run's commits can change its hash. Neither repository is pushed; the operator pushes.
- User-resolved choices
  - The user chose to fix six rejected candidates alongside C-1: D1-11 and D1-19 together, D1-23, D1-24, D1-27, D1-31, and D1-34. None of the seven fixes needs a hard-to-reverse change: runvil-lua is internal, `public-api.txt` doesn't change, and D1-34 is prose.
- Rejected alternatives, assumptions, and risks
  - Rejected: restoring `web_search` and `web_fetch` in the C-1 prose. It's correct today but breaks again as soon as a render renames either tool.
  - Rejected: a text check over every file under `prompts/` for tool ids. It's a structural check on source shape, and the effect-level test protects the actual contract.
  - Rejected: making run limits a Harness setting for D1-34. It would add a public Harness setting, which is hard to reverse and a separate decision.
  - Rejected: rewriting every guide sentence that says the Harness can set a limit. They state authority, not what today's runs observe.
  - Rejected: keeping D1-1 as debt. See the inventory above.
  - Assumption: a regression test that already passes before its fix proves its finding false. In that case, keep the test and skip the change.
  - Risk (C-1): how often a model takes the prose literally is model-dependent. The failure path is certain from the code; its frequency is not.
  - Risk (D1-24): the new test's tree must let the pass reach the call chain. Its first assertion, that `plan` returns `Err(Error::Internal { .. })`, shows whether it does.
  - Analysis limits:
    - Eight large target commits got targeted rather than line-by-line review: `c0c76d748`, `1f78b71a4`, `e659e5531`, `518822e19`, `1fccbb70f`, `ce52c73e3`, `b66adf0a2`, and `687618e6f`.
    - No code was executed.
    - The gitignored `local/` prompts were not inspected. runvil-docs was read only for D1-34.

### Deferred and Out of Scope

- D1-1, the MCP discovery overlap. Revisit if MCP rendering is dropped rather than deferred, or if prompts start offering `mcp` tools beside dynamic offers. A merge-time notice is then the cheap guard. When MCP rendering is planned, `git grep get_dynamic_tools crates/plugin-mcp/src` returning nothing is a good acceptance line.
- Making run limits configurable through the Harness. Revisit when a Host needs non-default limits.
- D1-8: the guide edits that `518822e19`, `1fccbb70f`, and `db52cd6ec` deferred live in runvil-docs. This collection checked runvil-docs only for D1-34.
- Note for the next engine plan: two causes were each corrected twice within the target. The response limits were renamed in `518822e19`, then removed in `db52cd6ec`; the section positions were moved in `e659e5531`, then unified in `ce52c73e3`. Both are resolved at the endpoint, but in each case the first episode reshaped what the second removed.

</decision-record>
<project-survey>

## Project Survey

- Status: complete
- Build command: `cargo build --locked -p gateway` (the workspace's only default member, so plain `cargo build` builds just the gateway). The desktop app is opt-in: `cargo build --locked -p workshop`, or `cargo workshop [--release]`, which builds the gateway, stages it as the Tauri sidecar, builds Workshop, and removes the staged sidecar. CI runs `npm ci --prefix crates/workshop` and `npm ci --prefix crates/gateway/config-ui/ui` before every Rust build, lint, test, and docs job.
- Focused test command pattern: `cargo nextest run --locked -p <crate> --all-features <test-name-substring>`, or with a filter expression `cargo nextest run --locked -p <crate> --all-features -E 'test(/<regex>/)'`. Drop `--all-features` for `workshop`, `workshop-server`, and `workshop-server-api`. cargo-nextest 0.9.128 is installed.
- Component test command pattern: `cargo nextest run --locked -p <crate> --all-features` (crate names are the package names, such as `runvil-engine` for `crates/runvil-internal/engine` and `gateway` for `crates/gateway/app`). Workshop crates: `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`. Boundary and structural checks: `cargo test -p build-xtask`.
- Full-suite test command: `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`, then `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api` for the workshop crates. UI and Node suites are separate: `node --test crates/workshop/ui/test/docs-claims.mjs` (wording scan, no `npm ci` needed), and `npm test --workspaces --if-present` under `crates/workshop` after `npm ci`.
- Linter command: `cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features` and `cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`, both with `CARGO_BUILD_WARNINGS=deny` (PowerShell: `$env:CARGO_BUILD_WARNINGS="deny"`). Also required: `cargo check -p gateway --no-default-features` (headless build shape) and `node --test crates/workshop/ui/test/docs-claims.mjs`. Never run a standalone `cargo check --workspace` beside clippy. Supply-chain: `cargo deny check`.
- Formatter check command: `cargo fmt --all --check` (`rustfmt.toml` sets `style_edition = "2024"`; the pre-commit hook runs it).
- Docs command: `cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api` and `cargo doc -p runvil --no-deps` (facade, default features), both with `RUSTDOCFLAGS="-D warnings"` (PowerShell: `$env:RUSTDOCFLAGS="-D warnings"`). Facade surface check: `cargo +<pinned nightly> xtask api --check`, nightly named in `crates/build-xtask/src/api/toolchain.rs`, run with `CARGO_BUILD_WARNINGS` unset; it compares against the committed `crates/runvil/public-api.txt`.
- Test placement and naming conventions: Unit tests live in a sibling file `<stem>-tests.rs` wired as `#[cfg(test)] #[path = "<stem>-tests.rs"] mod tests;` (about 200 such files). When a module's tests reach three or more files they move to a `<module>/tests/` directory of topic files (for example `crates/runvil-internal/parser/src/tests/`). Tiny inline `mod tests { use super::*; }` blocks also appear. Test-only hooks on production types are `#[cfg(test)]` methods with a `_for_test` suffix. Integration tests are one binary per crate at `tests/it/main.rs` (`tests/suite/main.rs` in `runvil` and `harness`) with topic modules plus a `support` module, `tests/fixtures/` for data, and `tests/prompts/` for prompt fixtures. Test names are snake_case behavior statements with no `test_` prefix, such as `nil_argv_is_an_error` and `resolves_argv_dotted_path`. Engine tests never sleep, read the wall clock, use timeouts, or draw OS entropy; pseudo-random input uses a fixed seed the failure prints; temp directory names come from the test name, process id, and a counter. Behavior changes ship with tests in the same change.
- Directory map:
  - `crates/`: every Rust crate plus the TypeScript and CSS UI packages. Manifestless containers hold each family's private crates: `runvil-internal/` (Engine internals: `engine`, `lua`, `parser`, `types`, `vfs`, `model-client`), `harness-internal/` (`runner`, `plugin-loader`, `test-plugin`), `gateway/` (`app` is the `gateway` package, plus `config`, `config-ui`, `local`, `logging`, `progress`, `protocol`, `routing`, `web-search`, `cloud-providers`, and `stt/` one level deeper), and `workshop/` (`desktop` is the Tauri `workshop` package, plus `server`, `server-api`, `agents`, `workspace`, and other family crates, and the `ui/`, `look/`, `platform/` front-end packages with the npm workspace root). `shared-ui/` is the TypeScript and CSS package the gateway config UI consumes.
  - Top-level crates in `crates/`: `runvil` (Engine facade), `runvil-plugin` (Plugin contract), `harness`, `harness-gateway-client`, the Packs `plugin-web`, `plugin-mcp`, `plugin-user-input`, `gateway-api-types`, `gateway-api-discovery`, `shared-error-source`, `shared-loopback`, and build tooling `build-xtask` (structural checks and `xtask api`), `build-ui`, `build-workshop`, `build-user-guide`, `build-llama-cuda`, `build-ceiling`.
  - `guide/`: documentation site sources (`books`, `chrome`, `landing`).
  - `prompts/`: sample Runvil prompts such as `greet.md` and `hello.md`.
  - `tools/`: Node scripts for gateway sidecar staging and a live TTS check, each with a `.test.mjs`.
  - `images/`: README and site art.
  - `local/`: gitignored machine-local gateway config, env files, profiles, and prompts.
  - `vibe/`: dated planning and work notes, grouped by month.
  - `.github/workflows/`: CI, nightly, release, and STT and CUDA build jobs. `.githooks/`: pre-commit runs `cargo fmt`, pre-push runs the wording scan, headless gateway check, clippy, and `cargo deny`. `.config/nextest.toml`: timeouts and a `heavy` group for whisper suites. `.cargo/config.toml`: `rust-lld` and static CRT on Windows, `cargo xtask` and `cargo workshop` aliases. `.cursor/`: rules.
- Component boundaries:
  - Engine: `runvil` re-exports from `runvil-engine`, which depends on `runvil-parser`, `runvil-lua`, `runvil-model-client`, `runvil-types`, `runvil-vfs`. `runvil-parser` depends on `runvil-lua`; `runvil-lua` on `runvil-model-client`, `runvil-types`, `runvil-vfs`; `runvil-model-client` on `runvil-types`. `runvil-types` and `runvil-vfs` are leaves. Engine crates never depend on `tokio`, `tokio-util`, `async-trait`, `reqwest`, `getrandom`, or `fastrand`, even transitively.
  - Plugin contract and Packs: `runvil-plugin` depends on `runvil-types` and `runvil-vfs`. `plugin-web`, `plugin-mcp`, and `plugin-user-input` depend only on `runvil-plugin`.
  - Harness: `harness` depends on `harness-runner`, `harness-plugin-loader`, `runvil`, `runvil-plugin`. `harness-runner` depends on `runvil` and `runvil-plugin`; `harness-plugin-loader` on `runvil-plugin`. `harness-gateway-client` depends on `harness`, `plugin-web`, `runvil`.
  - Gateway: `gateway` depends on its family crates plus `gateway-api-types`, `gateway-api-discovery`, `shared-loopback`, and on no Engine or Harness crate. Long-running work reports through `gateway-progress`; outsiders read only the `Progress` type from `gateway-api-types`.
  - Workshop: `workshop` (desktop) depends on `workshop-server-api` and `gateway-api-discovery`. `workshop-server` depends on `harness`, `harness-gateway-client`, all three Packs, `runvil`, `runvil-plugin`, `shared-loopback`, and the workshop family crates. `workshop-agents` depends on `harness`, `plugin-user-input`, `runvil`.
  - `cargo test -p build-xtask` enforces product and container boundaries, the Workshop tier graph, the `## Invariants` marker, and lint inheritance.
- Conventions summary: Seven capitalized defined terms (Engine, Harness, Host, Pack, Plugin, Static Set, Dynamic Set) carry one meaning each; Engine crates say "the caller" and never mention the Host; `docs-claims.mjs` enforces the wording. Rust edition 2024 on pinned toolchain 1.99, Engine MSRV 1.95. Every crate inherits strict workspace lints: clippy `all` and `pedantic` denied, `unwrap_used` and `expect_used` denied, `allow_attributes` denied, `unsafe_code` denied, `missing_docs` and `unreachable_pub` warned. Crate docs carry a `## Invariants` section. Source directories stay flat: one or two related files are `foo-bar.rs` siblings wired with `#[path]`, and three or more become a `foo/` subdirectory. Comments explain only non-obvious constraints, and workarounds cite an upstream issue URL; `Cargo.toml` pins carry rationale comments. Error messages are written for a model reader, naming required versus actual. JSON reaching a recorder round-trips exactly, with sorted keys and `float_roundtrip`. Cargo features gate real build constraints, not product shape. Prefer reusing an existing facility over adding new machinery. Workshop UI CSS uses design tokens only, and persisted UI values go through the `ui-storage` adapter.

</project-survey>
<execution-plan>

## Execution Instructions

- Operator's parallel work: `README.md` and `images/workshop-window.jpg` in runvil belong to the operator, who edits them during this run. No step edits, reverts, stashes, or deletes them. The orchestrating session sweeps a modified `README.md` into the next runvil commit or amend, with the image only while `README.md` references it, so a step commit that holds them is in scope.
- Nothing is pushed to any remote. The operator pushes both repositories.

<step-1>

### Step 1: Describe the research prompt's tools by role [completed]

- Component: Research prompt
- Tests: run each from `c:\Users\Vinnie\cursor\runvil` in PowerShell, as written, after clearing `CARGO_BUILD_WARNINGS` and `RUSTDOCFLAGS` so each command sees only the variables it sets itself; each exits 0.
  - `cargo nextest run --locked -p runvil --all-features shipped`
  - `cargo fmt --all --check`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy -p runvil --all-targets --all-features`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`
- Commit: one runvil commit holding the test and the prompt edit, made after the Tests pass. Commit without asking, write the message from the staged diff, and don't push.

- Placement: first of five components. This is C-1, the one finding the debt collection kept and the only change to what a model is told. It touches only `prompts/research-person.md` and the `runvil` facade suite, shares no file with any other component, and nothing depends on it. Its one piece, the regression test and the prose edit, is built as this one step because the test is what proves the edit.
- Rules: paths are relative to the runvil root. Line numbers cite `1d6937459`, so find each site by name and treat the number as a hint. Comments state non-obvious constraints only and never narrate history. Prose and comments use a plain dash, never an em dash or a double dash. Never remove a file with the Delete tool.
- Finding: `prompts/research-person.md` lines 32-33 tell the model to run `web/search` queries and to use `web/fetch`. Those are tool ids, and the model only ever sees wire names. `tool_schema` (`crates/runvil-internal/engine/src/execute/scope.rs:486-496`) builds each advertised schema from the binding's wire name, description, and schema only; `plugin-web` sets no `wire_name` (`crates/plugin-web/src/web.rs:107-112`), and no shipped Pack overrides `Pack::render`. So the model's tool list names `web_search` and `web_fetch`, which the existing shipped test already pins. A model that follows the prose literally calls `web/search`, which neither frozen set holds, so `tool_calls` (`crates/runvil-internal/engine/src/execute/scheduler/chat.rs:436-450`) returns `Error::OutOfScopeToolCall`, reports `TOOL_CALL_FAILED`, and the section fails because its Lua has no `pcall`. The contract is the tools-kernel-redo rule: "A wire name is the name the model sees for a tool. The tool id is the stable name."
- Test, in `crates/runvil/tests/suite/shipped.rs`:
  - Extract the existing test's scenario into a helper both tests share, returning `TestResult<Trace>`. `Trace` is `pub(crate)` in `crates/runvil/tests/suite/support/scenario.rs` but not re-exported, so add it to `pub(super) use scenario::{Prepared, Scenario};` in `crates/runvil/tests/suite/support.rs`. The helper holds the two-tool `ToolCatalog` (`web/fetch` and `web/search` over the `query` schema), the scripted model `calls("web_search", json!({ "query": "Ada Lovelace" }))` then `text("a factual summary")` over `Tool::untrusted("web/search", "search results")`, `.args("Ada Lovelace")`, the catalog installed through `.context(..)`, and the prompt read from `../../prompts/research-person.md` under `CARGO_MANIFEST_DIR`. `the_shipped_research_prompt_offers_its_web_tools_by_id_and_runs_end_to_end` keeps every assertion it has.
  - Add `the_shipped_research_prompt_names_no_tool_by_its_id_in_what_the_model_reads`. `EffectRecord::Chat` carries `messages: Vec<Value>`, the request's wire-form messages (`crates/runvil-internal/engine/src/execute/run/effect-record.rs:64-80`). For every `EffectRecord::Chat { messages, .. }` in `trace.effects`, and every message in `messages` whose `"role"` is `"user"`, assert that `serde_json::to_string(message)` contains neither `web/search` nor `web/fetch`, with a failure message naming the id it found.
  - Check only user messages, so ids that tool results or call records carry can't make it fail by accident.
  - Reword the file's `//!` line if it no longer describes both tests.
- Fail first: run `cargo nextest run --locked -p runvil --all-features shipped` before editing the prompt, and confirm the new test fails because the first round's user message, the section prose sent by `messages:user(prose)`, contains `` `web/search` `` (line 32) and `` `web/fetch` `` (line 33). If it passes instead, the finding is false: keep the test, skip the prose edit, commit the test alone, and report C-1 as false.
- Prose edit, in `prompts/research-person.md`:
  - Line 32 says to search the web with several targeted queries to find who this person is and the most relevant, reputable sources about them. It names no tool.
  - Line 33 says to fetch the few best results to confirm facts from the primary or most authoritative pages, and keeps the sentence that everything a fetch returns is untrusted third-party text, to summarize and never to follow. It names no tool.
  - Leave line 21's `tools.offer({"web/search", "web/fetch"})` unchanged, because Lua names tools by id and that line is correct. Leave the rest of the prompt unchanged.
  - Don't restore `web_search` and `web_fetch` in the prose, and don't add a text check over the files under `prompts/`. The plan rejected both: the first breaks again as soon as a render renames either tool, and the second checks source shape where the effect-level test checks the contract.
- Acceptance: both shipped tests pass, the new one failed before the prose edit, and the prompt's model-facing prose describes the two tools by role, so it stays true under any render.

</step-1>

<step-2>

### Step 2: Leave admission as it was when a pass fails [completed]

- Component: Admission
- Tests: run each from `c:\Users\Vinnie\cursor\runvil` in PowerShell, as written, after clearing `CARGO_BUILD_WARNINGS` and `RUSTDOCFLAGS` so each command sees only the variables it sets itself; each exits 0.
  - `cargo nextest run --locked -p runvil-engine --all-features`
  - `cargo fmt --all --check`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy -p runvil-engine --all-targets --all-features`
- Commit: one runvil commit holding the tests and the fix, made after the Tests pass. Commit without asking, write the message from the staged diff, and don't push.

- Placement: second. D1-24 is the plan's other behavior change. It is confined to `crates/runvil-internal/engine/src/execute/scheduler/admission.rs` and its tests and shares no file with any other component. It lands before the refactors, so their no-behavior-change claim is checked against an engine suite that already holds both new regressions. Its one piece, the regression tests and the fix, is built as this one step because the tests are what prove the fix.
- Rules: paths are relative to the runvil root. Line numbers cite `1d6937459`, so find each site by name and treat the number as a hint. Engine crates' docs, comments, and tests say "the caller", never the Host. Engine tests never sleep, read the clock, or draw entropy, and `unwrap` and `expect` appear only in tests. Comments state non-obvious constraints only and never narrate history. Prose and comments use a plain dash, never an em dash or a double dash.
- Finding: `Admission::plan` (`admission.rs:161-200`) takes the resume queue and then the spawn queue with `std::mem::take`. It returns `Error::Internal` at `:175` ("a resumed chain's slots belong to an admitted task") or `:192` ("a queued chain backs a task") without writing back what it took, after earlier admissions in the same pass have already changed `holding` and `released`. `is_queued` (`:146`) reads only the spawn queue, and `end_backing` treats a task chain that isn't queued as admitted. So after a `:192` error, a spawn that never started can report `TaskAbandoned` without `TaskStarted`, against the doc that a never-admitted task reports no terminal event. Both errors assert scheduler invariants, so only a scheduler bug reaches this, and the run ends on it.
- Tests, in `crates/runvil-internal/engine/src/execute/scheduler/admission-tests.rs`. It is a child module of `admission.rs`, so it can read the private `resuming` and `holding` fields, and it already has the `Tree` builder and the `spawn` helper.
  - New `a_pass_that_fails_on_an_internal_error_leaves_admission_as_it_found_it`:
    - `let mut tree = Tree::new();`, `let mut admission = Admission::default();`, then `let root = tree.root(2);`. A limit of 2 lets the pass admit the first entry and still reach the second.
    - `let task = spawn(&mut admission, &mut tree, root);`, then `let call = tree.call(root);`, then `admission.enqueue_spawn(call, SpawnRecord { input: None, section: "not a task".to_owned() });`.
    - Assert `matches!(admission.plan(&tree.view()), Err(Error::Internal { .. }))`, which shows the pass reached the call chain. Then assert `admission.is_queued(task)` and `!admission.holding.contains(&task)`.
  - Extend `a_resumption_whose_slots_belong_to_no_admitted_task_is_an_internal_error` (`:250`) with `assert!(admission.resuming.contains(&queued));` after its error assertion.
  - Add the new rule to the file's `//!` list of rules.
- Fail first: run `cargo nextest run --locked -p runvil-engine --all-features admission` and confirm that both tests fail on their queue and holder assertions. At `1d6937459` the pass admits `task` into `holding` and takes the spawn queue before it returns at `:192`, and it takes the resume queue before it returns at `:175`. If the new test's first assertion fails instead, the tree doesn't let the pass reach the call chain: fix the tree, not the assertion. If both tests already pass, the finding is false: keep the tests, skip the fix, commit the tests alone, and report D1-24 as false.
- Fix, in `Admission::plan`:
  - Run the pass on local copies of `resuming`, `spawned`, `holding`, and `released`, and assign all four back to `self` only just before returning `Ok`. An `Err` return then leaves all four exactly as they were before the pass started.
  - Every read the pass makes through `slots_in_use`, `slot_holder`, and `is_queued` must see the same state it sees today, so the same entries are admitted in the same order. Where one of those helpers reads `self`, have it read the pass's working copy instead, for example by passing the copy in.
  - The early `Ok` return when both queues are empty stays ahead of any copy, so a pass with nothing queued copies nothing.
  - Which passes fail, and their messages, stay the same; only the state left after a failure changes. Don't validate every entry up front: checking entries the pass would not admit would make some passes fail that succeed today.
  - Add to the `# Errors` doc that a failed pass leaves admission as it found it.
- Acceptance: every admission test passes, the two new assertions failed before the fix, and the concurrency suites under `crates/runvil-internal/engine/src/execute/tests/scheduler/` still pass, which the full engine run in the Tests covers.

</step-2>

<step-3>

### Step 3: Make OfferMode the one frozen-set enum [completed]

- Component: Scope types
- Tests: run each from `c:\Users\Vinnie\cursor\runvil` in PowerShell, as written, after clearing `CARGO_BUILD_WARNINGS` and `RUSTDOCFLAGS` so each command sees only the variables it sets itself; each exits 0.
  - `cargo nextest run --locked -p runvil-engine --all-features`
  - `cargo nextest run --locked -p runvil-lua --all-features scope`
  - `if (Get-ChildItem crates/runvil-internal/engine -Recurse -Filter *.rs | Select-String -Pattern 'FrozenSet' -CaseSensitive -List) { throw 'the engine still names FrozenSet' }`
  - `cargo fmt --all --check`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy -p runvil-lua -p runvil-engine --all-targets --all-features`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`
- Commit: one runvil commit holding the enum change and the updated engine tests, made after the Tests pass. Commit without asking, write the message from the staged diff, and don't push.

- Placement: third. This component covers D1-11, D1-19, and D1-23 across runvil-lua's `scope` module and the engine's `execute::scope` and `error::sets` modules. It is a refactor plus a guard, independent of every other component, and follows the two behavior changes so the whole engine suite, new regressions included, confirms nothing observable changed. It has two pieces, this enum and the discovery-id guard in Step 4, built sequentially because both edit `crates/runvil-internal/lua/src/scope.rs`.
- Rules: paths are relative to the runvil root. Line numbers cite `1d6937459`, so find each site by name and treat the number as a hint. No behavior change: every merge error message keeps its exact text. runvil-lua is an internal crate, so no public API changes, and `crates/runvil/public-api.txt` stays as it is. Engine crates' docs, comments, and tests say "the caller", never the Host. Comments state non-obvious constraints only and never narrate history. Prose and comments use a plain dash, never an em dash or a double dash.
- Finding: `FrozenSet` (`crates/runvil-internal/engine/src/execute/scope.rs:41-56`) and `OfferMode` (`crates/runvil-internal/lua/src/scope.rs:92-97`) have the same two variants, `Static` and `Dynamic`, with the same meaning. `error/sets.rs:8` imports `FrozenSet` from `execute::scope`, while `execute/scope.rs:18` imports `crate::error::{SetRule, SetTool}`, so the two engine modules import each other. That leaves two enums to keep in step and a module cycle inside the engine crate.
- Change:
  - `crates/runvil-internal/lua/src/scope.rs`: add `impl std::fmt::Display for OfferMode` writing `Static Set` for `Static` and `Dynamic Set` for `Dynamic`. That is the exact text `FrozenSet`'s `Display` writes today (`execute/scope.rs:48-55`), so every merge error message that interpolates the set stays byte-identical.
  - `crates/runvil-internal/engine/src/execute/scope.rs`: delete `FrozenSet`, its doc, and its `Display`. Add `OfferMode` to the existing `use crate::lua::{..}` list, which `crates/runvil-internal/engine/src/lua.rs:8` already re-exports, and use it in `set_of`, `set_mut`, `lay_out`, the set checks, and the merge's pairing at `:459-460`.
  - `crates/runvil-internal/engine/src/error/sets.rs`: replace `use crate::execute::scope::FrozenSet;` with `use runvil_lua::OfferMode;`, as `error/convert.rs:8` and `error/value.rs:15` already import runvil-lua types, and type `SetTool::set` as `OfferMode`. Importing from runvil-lua directly, not through `crate::lua`, adds no intra-crate import, so no new cycle can form.
  - `crates/runvil-internal/engine/src/execute/scope-tests.rs` and `crates/runvil-internal/engine/src/execute/scope-tests-render.rs`: replace every `FrozenSet::Static` and `FrozenSet::Dynamic` with the matching `OfferMode` variant, including the parameter type of `site` in `scope-tests-render.rs`. Keep every expected message string exactly as it is.
- Keep the name `OfferMode`, because offers are defined in runvil-lua and the engine already imports it.
- Acceptance: nothing in the engine names `FrozenSet`, `error/sets.rs` no longer imports from `execute::scope`, and every merge-error message assertion in the engine's scope tests passes with its existing text. The compiler's exhaustive matches cover the rest.

</step-3>

<step-4>

### Step 4: Tie the discovery tool ids to a reserved Pack [completed]

- Component: Scope types
- Tests: run each from `c:\Users\Vinnie\cursor\runvil` in PowerShell, as written, after clearing `CARGO_BUILD_WARNINGS` and `RUSTDOCFLAGS` so each command sees only the variables it sets itself; each exits 0.
  - `cargo nextest run --locked -p runvil-lua --all-features scope`
  - `cargo test -p build-xtask`
  - `cargo fmt --all --check`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy -p runvil-lua --all-targets --all-features`
- Commit: one runvil commit holding the new test file and its wiring, made after the Tests pass. Commit without asking, write the message from the staged diff, and don't push.

- Placement: the second Scope types piece, built after Step 3 because both edit `crates/runvil-internal/lua/src/scope.rs`.
- Rules: paths are relative to the runvil root. Line numbers cite `1d6937459`, so find each site by name and treat the number as a hint. Unit tests go in the module's kebab-case sibling file and are named as snake_case sentences that state the behavior. `unwrap` and `expect` appear only in tests. Prose and comments use a plain dash, never an em dash or a double dash.
- Finding: `DiscoveryTool::id` spells `dynamic/search` and `dynamic/call` (`crates/runvil-internal/lua/src/scope.rs:210-213`), and `RESERVED_PACK_IDS` spells `"dynamic"` (`crates/runvil-internal/types/src/packs.rs:31`). The types crate's own test (`crates/runvil-internal/types/src/packs-tests.rs:70-86`) checks a literal `dynamic/search`, because that crate can't depend on runvil-lua. Renaming one spelling without the other would let a vendor Pack take the discovery namespace, with no test to catch it.
- Test: add `crates/runvil-internal/lua/src/scope-tests.rs`, opening with a `//!` line that names its concern, and wire it from the end of `scope.rs` with `#[cfg(test)] #[path = "scope-tests.rs"] mod tests;`. `scope.rs` has no test module today.
  - One test, for example `every_discovery_tool_id_sits_under_a_reserved_pack`: for each tool in `DiscoveryTool::ALL`, parse `tool.id()` with `ToolId::parse` (`crates/runvil-internal/types/src/tools/ids.rs:33`) and assert `id.pack().is_reserved()` (`ids.rs:60` and `crates/runvil-internal/types/src/packs.rs:89`), with a failure message naming the id.
- The test passes at once, because it guards a future rename rather than a present failure, so no fail-first run applies. `cargo test -p build-xtask` is in the Tests because a new test file is wired.
- Acceptance: the guard passes, and `scope.rs` changes only by the test-module wiring.

</step-4>

<step-5>

### Step 5: Drop the dead counts parameter from the dispatch bodies

- Component: Dispatch bodies
- Tests: run each from `c:\Users\Vinnie\cursor\runvil` in PowerShell, as written, after clearing `CARGO_BUILD_WARNINGS` and `RUSTDOCFLAGS` so each command sees only the variables it sets itself; each exits 0.
  - `cargo nextest run --locked -p runvil-lua --all-features dispatch parameters`
  - `cargo fmt --all --check`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy -p runvil-lua -p runvil-engine --all-targets --all-features`
- Commit: one runvil commit holding the signature change, both engine callers, and the updated tests, made after the Tests pass. Commit without asking, write the message from the staged diff, and don't push.

- Placement: fourth. D1-27 is a refactor of runvil-lua's dispatch API and its two engine callers, shares no file with the earlier components, and depends on none of them. It goes after them and before the documentation component, whose step runs the plan's exit commands and so must come after every runvil code change. Its one piece is this one step, because the signature change, its callers, and its tests only compile together.
- Rules: paths are relative to the runvil root. Line numbers cite `1d6937459`, so find each site by name and treat the number as a hint. No behavior change. Engine crates' docs, comments, and tests say "the caller", never the Host. Comments state non-obvious constraints only and never narrate history. Prose and comments use a plain dash, never an em dash or a double dash.
- Finding: `prepare_dispatch` (`crates/runvil-internal/lua/src/dispatch.rs:127`) and `prepare_model_dispatch` (`:206`) take `counts: Option<&ToolCallCounts>` and increment it when it is `Some`. Their only production callers, `crates/runvil-internal/engine/src/execute/scheduler/apply.rs:306` and `:319`, pass `None`, because the engine counts every call before dispatch. Each function carries `#[expect(clippy::too_many_arguments)]`, because its parameter list reached eight.
- Change, in `crates/runvil-internal/lua/src/dispatch.rs`:
  - Remove the `counts` parameter from both functions, the `if let Some(counts) = counts { .. }` increment in `prepare_dispatch`, the `counts` argument `prepare_model_dispatch` forwards, and the now-unused `use crate::scope::ToolCallCounts;`.
  - Remove both `#[expect(clippy::too_many_arguments, ..)]` attributes. Seven parameters no longer trip the lint, and an unfulfilled `expect` would fail clippy.
  - The module doc (`:3-5`) stops listing the per-VM call counts among what `prepare_dispatch` composes.
  - `prepare_dispatch`'s sequence paragraph (`:102-104`) starts at the succeeded/failed observation, and its `# Errors` keeps only the `Error::Tool` sentence.
  - In `prepare_model_dispatch`'s doc, "The counts increment and every other dispatch failure still propagate" becomes "Every other dispatch failure still propagates". Its `# Errors` section, which clippy's `missing_errors_doc` requires, says it propagates any error of `prepare_dispatch` other than the tool's own failure, which becomes the call's result.
  - Keep both return types, so behavior and callers' error handling don't change. Keep `ToolCallCounts` itself, because the engine still counts calls with it before dispatch.
- Callers and tests:
  - `crates/runvil-internal/engine/src/execute/scheduler/apply.rs`: drop the `None` counts argument from the `prepare_model_dispatch` call (`:306`) and the `prepare_dispatch` call (`:319`).
  - `crates/runvil-internal/lua/src/dispatch-tests.rs`: drop the counts argument from every call. In `prepare_dispatch_wraps_a_canned_untrusted_output_counts_it_and_reports_it` (`:16`), delete the `ToolCallCounts` local and the assertion that the tool's count reached 1, and rename the test `prepare_dispatch_wraps_a_canned_untrusted_output_and_reports_it`. Keep every other assertion. No other test in the file names counting.
  - `crates/runvil-internal/lua/src/tools/tests-parameters.rs:156`: drop the `None` counts argument, the fourth, from the `prepare_dispatch` call.
- Acceptance: neither function takes `counts` or carries a lint expectation, no test asserts the removed increment, and the focused runvil-lua run and clippy over both crates pass.

</step-5>

<step-6>

### Step 6: Correct display_chain's doc and the guide's run-limit claim, and run the exit gates

- Component: Documentation
- Tests: run each from `c:\Users\Vinnie\cursor\runvil` in PowerShell, as written and in this order, after clearing `CARGO_BUILD_WARNINGS` and `RUSTDOCFLAGS` so each command sees only the variables it sets itself; each exits 0.
  - `if (Select-String -Path crates/harness-internal/runner/src/display_chain.rs -Pattern 'LuaRuntime' -Quiet) { throw 'display_chain.rs still names LuaRuntime' }`
  - `if (Select-String -Path ../runvil-docs/src/language/16-limits-and-errors.md -Pattern 'can differ from the ones in the table|unless the Harness sets another' -Quiet) { throw 'the guide still says a run''s limits can differ from the defaults' }`
  - `if (-not (Select-String -Path ../runvil-docs/src/language/16-limits-and-errors.md -Pattern 'the Harness passes none today' -SimpleMatch -Quiet)) { throw 'the guide does not say the Harness passes no limits today' }`
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `cargo fmt --all --check`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`
  - `cargo test -p build-xtask`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`
  - `Remove-Item Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue; cargo +nightly-2026-09-05 xtask api --check`
- Commit: two commits, each made after the Tests pass and without asking, and neither pushed.
  - One runvil commit holding only `crates/harness-internal/runner/src/display_chain.rs`, its message written from the staged diff.
  - After that runvil commit is final, one commit in `c:\Users\Vinnie\cursor\runvil-docs`, its own git repository, holding only `src/language/16-limits-and-errors.md`. Its message is written from the staged diff by following `c:\Users\Vinnie\cursor\.cursor\rules\commit.mdc`, in that repository's style: a subject like `Match the guide to runvil <hash>'s ...` and a one-paragraph body ending `Matches runvil commit 1d6937459.` It names `1d6937459` because D1-34 has no runvil change of its own, and that commit holds the code the corrected sentence describes.

- Placement: the last component and the plan's last step. Documentation holds D1-31 and D1-34, prose only, in two repositories. They form one step because the guide edit has no runvil change of its own. This step's runvil change is the last one, so its Tests run the plan's exit commands over every runvil item, plus two checks on the guide, which those commands don't cover.
- Rules: `crates/` paths are relative to the runvil root, and paths starting with `src/` are relative to `c:\Users\Vinnie\cursor\runvil-docs`. Line numbers cite runvil `1d6937459` and the guide at `d81228d`, so find each site by name or text and treat the number as a hint. Comments state non-obvious constraints only and never narrate history. Prose and comments use a plain dash, never an em dash or a double dash.
- D1-31 finding, in runvil: `crates/harness-internal/runner/src/display_chain.rs:16-17` says "Some error variants copy their source's text into their own message, and the Engine's `LuaRuntime` variant is one example." Commit `1fccbb70f` merged that Engine variant into `Error::Lua`, so the example names a variant that no longer exists.
- D1-31 change: drop the example clause, so the sentence reads "Some error variants copy their source's text into their own message." Rewrap the doc paragraph to match its neighbors and change nothing else in the file.
  - Don't name runvil-lua's `Error::LuaRuntime` instead. Its message holds its cause's text, but runvil-lua and the engine both keep a cause off `source()` by design (`crates/runvil-internal/lua/src/error.rs:14-15` and `crates/runvil-internal/engine/src/error.rs:12-13`), so it isn't an example of a source `display_chain` walks.
- D1-34 finding, in runvil-docs: `src/language/16-limits-and-errors.md:125` reads "These built-in values are the default limits, the set every run starts from. The Harness that runs a prompt can replace any of the four defaults, so the limits a given run observes can differ from the ones in the table. One set of limits applies to the whole run, the [H1 pass](04-how-a-prompt-runs.md#the-h1-pass) and every section included, and it is the default set unless the Harness sets another." `RunContext::with_limits` has one caller, the runvil test helper `crates/runvil-internal/engine/src/test_support/scenario.rs:212`, so no Harness code sets limits and every run observes the defaults.
- D1-34 change: rewrite that paragraph to read exactly:
  > These built-in values are the default limits, the set every run starts from. The Engine accepts other limits from the code that starts a run, but the Harness passes none today, so every run observes these defaults. One set of limits applies to the whole run, the [H1 pass](04-how-a-prompt-runs.md#the-h1-pass) and every section included.
  - This keeps that these are the defaults every run starts from and that one set applies to the whole run. It drops the claim that a given run's limits can differ, and drops the closing clause "and it is the default set unless the Harness sets another", which the new middle sentence replaces.
- Leave every other run-limit sentence alone. They say who may change a limit, the Harness side and never the prompt, and that stays true: `16-limits-and-errors.md:140`, `:146-148`, `:196`, `:200`, `:248`, and `:254`; `05-lua-environment.md:2343`; `14-fanout.md:1196` and `:1281`; and `17-quick-reference.md:14` and `:275`. Don't add a Harness setting for run limits: the plan rejected that as a separate, hard-to-reverse public change.
- Acceptance: the runner doc names no variant, the guide paragraph says what every run observes today, no other guide line changes, and every command in the Tests passes. The cargo and node commands are the plan's exit gates, so passing them confirms the runvil success criteria: the workspace suites, clippy with warnings denied, fmt, both rustdoc gates, `cargo test -p build-xtask`, the docs-claims scan, and an unchanged `crates/runvil/public-api.txt`.

</step-6>

</execution-plan>
