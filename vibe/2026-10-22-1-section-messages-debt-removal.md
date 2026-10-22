---
name: Section messages debt removal
overview: "Debt collected on runvil's section-messages work (upstream/master aca75013f to HEAD 1e808b41f, 9 commits), plus three items the user added: remove the orphaned OverflowReason items, make an uncaught direct compactors.fail end the run as ContextExhausted, and give the Workshop chat agent a summarizing compactor so it recovers from an overflow. Three runvil commits and one runvil-docs commit."
todos:
  - id: remove-overflow-items
    content: "runvil: delete OverflowReason::ALL and the unused std::error::Error impl, iterate the local two-reason array in the exhaustion_detail test, drop the impl's test, rewrite the OverflowReason type doc, and fix the 'only shipped policy' comment (D1-1, D1-21, C-1, D1-22)"
    status: completed
  - id: direct-fail-kind
    content: "runvil: classify a downcast Error::ContextExhausted in map_runtime_error_among so an uncaught direct compactors.fail ends the run as ContextExhausted; add the failing-first Engine test; update the program.rs, run.rs, and compactors.rs docs (M-1)"
    status: completed
  - id: chat-compactor
    content: "runvil: give chat.md a summarizing compactor with a bounded summary input, update its prose and the models_loop-author-shapes.rs copy, add the Engine recovery test and the Workshop chat-gate overflow test (D1-20)"
    status: completed
  - id: guide-update
    content: "runvil-docs: drop the direct-call workaround and update the run-error-kind passages (M-1), and show the chat agent's compactor (D1-20); run the guide checks and the site build"
    status: completed
  - id: verify
    content: Run each item's focused checks and the runvil exit gates after each runvil commit, and the guide checks after the runvil-docs commit
    status: completed
isProject: false
---

# Section messages debt removal

<product-contract>

## Product Requirements

- Scope and target work:
  - Repository `runvil`, baseline `aca75013f` (`upstream/master`, also the merge base), endpoint `1e808b41f` (`HEAD`), disposition at the endpoint (worktree clean and excluded).
  - Target: the 9 commits of the section-messages plan, `df3f01c78` through `1e808b41f`. Design record: [vibe/2026-10-21-1-section-messages.md](runvil/vibe/2026-10-21-1-section-messages.md).
  - On 2026-10-22, Vinnie expanded the scope beyond the debt the analysis kept to three more items: D1-22, M-1, and D1-20 with a summarizing policy. M-1 and D1-20 also reach the language guide in `runvil-docs` (`c:\Users\Vinnie\cursor\runvil-docs`, `src/language/`).
- Cleanup goals:
  - remove the `OverflowReason` items the target orphaned or left unused, and the stale prose beside them;
  - make an uncaught direct `compactors.fail(reason)` end the run the same way a compactor-raised one does;
  - make the Workshop chat agent recover from a context overflow instead of failing every later turn.
- Non-goals:
  - no change to the facade surface (`crates/runvil/public-api.txt`) and no new structural checks;
  - no change to the guide's teaching example of the summarizing compactor (`11-conversations.md`, lines 2416-2456);
  - no change to the chat-shape copies that test other behavior (`harness-stop.rs` `CHATS`, Workshop `agents.rs` `ECHO_MD`, `agents/revoke.rs` `ROOTS_MD`);
  - the precheck still ignores tool schemas and images (D1-2 stays deferred).
- Success criteria:
  - `OverflowReason::ALL` and the `std::error::Error` impl on `OverflowReason` no longer exist, and the two stale doc texts match the code;
  - an uncaught direct `compactors.fail('manual')` ends the run as `RunErrorKind::ContextExhausted` with `manual` in its message;
  - the shipped chat agent answers the turn after an overflow and every turn after it;
  - each new regression test fails at the endpoint and passes after its change, and every gate passes after each commit.

## Functional Specification

### Debt Inventory

- Debt added: none. No introduced or worsened debt survived the analysis and the independent challenge.
- Cheap fixes:
  - D1-1, dead code the target orphaned (`c952b5c78`). Before the target, `OverflowReason::ALL` ([crates/runvil-internal/lua/src/compactors.rs](runvil/crates/runvil-internal/lua/src/compactors.rs), lines 51-53) had two readers. At the endpoint its only reader is the test at [compactors-tests.rs](runvil/crates/runvil-internal/lua/src/compactors-tests.rs) line 153. It is not in `public-api.txt`. Impact: maintenance noise only.
  - Riding along with D1-1, sharing its cause and files:
    - D1-21: the `OverflowReason` type doc (`compactors.rs`, lines 36-39) says "The reason is the whole active compactor contract", which is false since `511386d85` and `c952b5c78`;
    - C-1: `compactors-tests.rs` line 69 says "`compactors.fail`, the only shipped policy", which is false since `compactors.clear` shipped in `511386d85`.
- Added by the user, beyond the debt the analysis kept:
  - D1-22, pre-existing: `impl std::error::Error for OverflowReason {}` (`compactors.rs`, line 89) has no reader except its own test, `an_overflow_reason_is_a_standard_error_whose_text_is_its_display`.
  - M-1, pre-existing and documented: a direct `compactors.fail(reason)` in a block, left uncaught, ends the run as kind `Lua`.
    - Cause: `compactors.fail` raises `Error::ContextExhausted` as an mlua external error (`compactors.rs`, lines 308-317). Only the loop and `models.compact` turn it into a structured table.
    - `block_failure` (`lua/src/vm/run.rs`, lines 185-219) classifies only stashed tables. Anything else maps through `LuaProgram::map_runtime_error_among` (`lua/src/program.rs`, line 196), which special-cases only `QuotaExceeded`.
    - The guide teaches a workaround (catch with `pcall`, then re-raise with `error(err)`).
    - Target behavior: the uncaught direct call ends the run as `ContextExhausted` with its reason, exactly as the workaround does today.
    - Side effects, all matching the workaround: an uncaught direct call in H1 ends as `ContextExhausted` instead of `RequirementsUnmet` (`step.rs` lines 186-197 remap only `Lua` and `LuaRuntime`), and a fanout arm or called section reports `context_exhausted` instead of `lua`.
  - D1-20, pre-existing: the Workshop chat agent ([crates/workshop/agents/agents/chat.md](runvil/crates/workshop/agents/agents/chat.md), lines 30-34) runs `pcall(function() return models.loop() end)` under the default `compactors.fail`.
    - After one overflow, the records are restored, the next turn appends another user record, and every later turn overflows again. The `pcall` swallows each failure.
    - Target behavior: the agent passes a summarizing compactor. When a turn overflows at the precheck or the provider, every record before the last two user turns is summarized by one `models.infer` call into one user record marked `meta = { summary = true }`, and the turn retries once.
    - The summary input is bounded. Each record contributes at most 1,500 characters of its encoded content, and the input totals at most 48,000 characters, filled from the newest summarized record backward. That keeps the infer prompt near 12,000 estimated tokens, inside the 32,768-token `min_context` that `chat.md` declares.
    - With exactly two user turns there is nothing before the second-to-last one to summarize, so the compactor calls `compactors.fail` and that turn fails as today. The next turn can summarize. With a single user turn, it summarizes that turn.
- Exposed pre-existing debt: none.
- Rejected candidates still out of scope (20):
  - residual but acceptable (9): D1-2, D1-3, D1-5, D1-6, D1-10, D1-11, D1-12, D1-13;
  - weak or speculative (5): D1-4, D1-7, D1-8, D1-14, D1-19;
  - false (5): D1-9, D1-15, D1-16, D1-17, D1-18.
  - Each is a deliberate design-record choice, a reserved deferral, a consequence shown only in principle, or ruled out by the code.

</product-contract>
<implementation-contract>

## Technical Design

- `OverflowReason` cleanup (D1-1, D1-21, C-1, D1-22), in [compactors.rs](runvil/crates/runvil-internal/lua/src/compactors.rs) and [compactors-tests.rs](runvil/crates/runvil-internal/lua/src/compactors-tests.rs):
  - delete `pub const ALL` and its doc (lines 51-53), and `impl std::error::Error for OverflowReason {}` (line 89);
  - rewrite the type doc (lines 36-39): the two overflow tags the loop passes to a compactor, which `exhaustion_detail` turns into a description. A compactor may also receive a caller's own reason through `models.compact`;
  - in the tests:
    - line 153 iterates `[OverflowReason::Precheck, OverflowReason::Provider]`, the idiom already at lines 86 and 169, adjusting any dereference;
    - delete `an_overflow_reason_is_a_standard_error_whose_text_is_its_display`;
    - line 69 names `compactors.fail` as the omitted-compactor default.
  - The exhaustive `match` in `OverflowReason::tag` and `Display` stays the check that forces a new variant to get a tag and a description.
- Direct `compactors.fail` classification (M-1):
  - In `map_runtime_error_among` ([program.rs](runvil/crates/runvil-internal/lua/src/program.rs), line 196), right after the `QuotaExceeded` case, return `Error::ContextExhausted { reason }` when `error.downcast_ref::<Error>()` yields `Error::ContextExhausted { reason }`. mlua 0.12.0's `downcast_ref` looks through the `CallbackError` that wraps a callback's `Err`.
  - Match only `ContextExhausted`, so `Store`, `Internal`, and `Lua` external errors keep their current kinds.
  - `block_failure` needs no edit: the new value passes through its `None => mapped` arm, and cancellation still wins because `map_chunk_failure` checks the cancel flag first.
  - Update the doc comments that state the old rule: `program.rs` lines 186-195 ("All remaining failures return `Error::LuaRuntime`"), `vm/run.rs` lines 173-184, and the `install_compactors` doc in `compactors.rs` (lines 291-302), which says only the loop normalizes the raise.
- Chat agent compactor (D1-20), in [chat.md](runvil/crates/workshop/agents/agents/chat.md):
  - the conversation block defines `compact` and calls `pcall(function() return models.loop(compact) end)`:

````lua
local function compact(reason)
  local first = 1
  while messages[first] and messages[first].role == "system" do
    first = first + 1
  end
  local cut, turns = #messages + 1, 0
  for i = #messages, first, -1 do
    if messages[i].role == "user" then
      turns = turns + 1
      if turns == 2 then cut = i break end
    end
  end
  if cut == first then return compactors.fail(reason) end
  local lines, room = {}, 48000
  for i = cut - 1, first, -1 do
    local line = messages[i].role .. ": " .. json.encode(messages[i].content):sub(1, 1500)
    if #line > room then break end
    room = room - #line
    table.insert(lines, 1, line)
  end
  local summary = models.infer("Summarize this conversation:\n" .. table.concat(lines, "\n"))
  messages:replace(first, cut - 1, {
    role = "user",
    content = "Summary of the earlier conversation: " .. summary,
    meta = { summary = true },
  })
end
````

  - The prose (lines 15-19) gains one sentence: when the conversation outgrows the model's context window, older turns are summarized so the conversation continues.
  - The chat-shape copy in `crates/runvil-internal/engine/src/execute/tests/models_loop-author-shapes.rs` (lines 35-59), whose doc says it guards the shipped chat loop, carries the same compactor. The Engine crate's wording rule bars "workshop" and "host" in that file.

</implementation-contract>
<verification-contract>

## Testing Plan

- D1-1, D1-21, C-1, D1-22:
  - `git grep -n -e "OverflowReason::ALL" -e "impl std::error::Error for OverflowReason" -e "whole active compactor contract" -e "only shipped policy" -- crates` prints nothing;
  - `cargo nextest run --locked -p runvil-lua --all-features -E 'test(/compactors/)'` passes, including `exhaustion_detail_is_an_overflow_tags_display_and_any_other_reason_itself`.
- M-1 regression test, new in `crates/runvil-internal/engine/src/execute/tests/models_loop_compactors.rs`, modelled on the loop-compactor test at lines 77-102:
  - `an_uncaught_direct_compactors_fail_ends_the_run_as_context_exhausted` runs one section with `compactors.fail('manual')` then `return 'unreachable'`;
  - it asserts `error.kind() == RunErrorKind::ContextExhausted`, that the message contains `manual`, that `inner()` matches `Error::ContextExhausted { reason }` with `reason == "manual"`, and that `gateway.call_count() == 0`;
  - fails at the endpoint: the kind is `Lua`, because the callback error maps through `map_runtime_error_among` to `Error::LuaRuntime`.
- D1-20 regression test, new Workshop chat-gate test (for example `crates/workshop/server/tests/it/chat_gate/compaction.rs`, declared beside the other gates):
  - It runs the real embedded `chat.md` with a test-local mock gateway that:
    - returns HTTP 400 with `context_length_exceeded` in the body for any chat request carrying more than 4 messages;
    - replies `summary` to a prompt starting `Summarize this conversation:`;
    - echoes otherwise.
  - Turn 1 sends 100,000 `x` characters, and turns 2 to 4 send short text.
  - It asserts that turns 3 and 4 each deliver a reply, after draining each overflowed round's `Model turn failed` frame with `recv_until` as GATE 6 does (`recovery.rs`, lines 78-86). That the summarize request stays under 4,000 characters, which pins the per-record cap. And that the retried request's first user message starts with `Summary of the earlier conversation: `.
  - Fails at the endpoint: turn 3 delivers no reply, because `compactors.fail` raises and the agent's `pcall` swallows it.
- D1-20 Engine test, in `models_loop-author-shapes.rs`:
  - the chat-shape copy with the compactor runs inputs `{ string.rep('x', 6000), 'q2', string.rep('y', 9000) }` against replies `a1`, `a2`, `summary`, `a3` under `loop_scenario`'s 4,096-token window;
  - turn 3 overflows the precheck and its retry succeeds;
  - it asserts that the last record reads `a3` and `messages[1].meta.summary` is true.
- Guide (`runvil-docs`), run in PowerShell:
  - `Get-ChildItem c:\Users\Vinnie\cursor\runvil-docs\src -Recurse -Filter *.md | Select-String -Pattern ([string][char]0x2014)` prints nothing;
  - ``git -C c:\Users\Vinnie\cursor\runvil-docs grep -n -F -e 'run error kind `Lua`, not `ContextExhausted`' -e "pcall(compactors.fail, 'precheck')" -- src`` prints nothing;
  - if a heading changes, every `#<old slug>` link is updated;
  - from `runvil` with `$env:RUNVIL_DOCS='c:\Users\Vinnie\cursor\runvil-docs'`: `cargo xtask site --books-only` builds and its link check passes.
- Exit gates after each `runvil` commit, run from `runvil` in PowerShell:
  - `cargo fmt --all --check`;
  - with `$env:CARGO_BUILD_WARNINGS='deny'`: `cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features` and `cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`;
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features` and `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`;
  - with `$env:RUSTDOCFLAGS='-D warnings'`: `cargo doc -p runvil-lua --no-deps --all-features` and `cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`;
  - `node --test crates/workshop/ui/test/docs-claims.mjs`;
  - with `CARGO_BUILD_WARNINGS` unset: `cargo +nightly-2026-09-05 xtask api --check`, unchanged because no facade item moves.

</verification-contract>
<decision-record>

## Decision Record

- Reversible decisions and consequences:
  - Delete `ALL` and the `Error` impl rather than gate them with `#[cfg(test)]`, and fold D1-21, C-1, and D1-22 into one commit, since all four sit in the same two files.
  - Classify M-1 in `map_runtime_error_among`, beside the `QuotaExceeded` downcast, with a match on `ContextExhausted` only. That covers every block kind and the `run_chunk` test path, and it leaves every other typed external error alone.
  - Bound the chat compactor's summary input at 1,500 characters per record and 48,000 in total, filled newest first. Without a bound, the infer prompt is close to the size of the list that just overflowed, so the summary itself would overflow in the case that triggers it. Consequence: when the summarized range is larger than the budget, its oldest records, including an earlier summary, drop out of the new summary.
  - Copy the compactor only into the chat-shape test that claims to guard the shipped loop (`models_loop-author-shapes.rs`). The other copies test stops, sockets, and roots, not the loop's history.
- User-resolved architecture choices (Vinnie, 2026-10-22):
  - fix D1-22, M-1, and D1-20 in this plan;
  - the chat agent summarizes older turns with one `models.infer` call and keeps the last two user turns;
  - M-1 changes the run error kind hosts see for an uncaught direct `compactors.fail`, from `Lua` (or `RequirementsUnmet` in H1) to `ContextExhausted`.
- Rejected alternatives:
  - raising a structured table from `compactors.fail` itself, which would change the typed raise the loop and `pcall` normalization downcast;
  - classifying every typed external error by kind, which would reclassify `Store` and `Internal` raises;
  - trimming or clearing in the chat agent, which the user declined in favor of summarizing;
  - an unbounded summary input.
- Assumptions and risks:
  - The analysis, the challenge, and these designs come from reading code, and nothing was run.
  - Each chat overflow costs one extra model call, and summary quality depends on the chat model.
  - The overflowed round's `Model turn failed` frame still reaches the operator before the retried reply.
  - The summary budget is sized for `chat.md`'s `min_context: 32768`. A larger window only makes it conservative.

### Deferred and Out of Scope

- The `Model turn failed` frame the operator sees for an overflowed round that compaction then recovers. Revisit if operators find it confusing.
- D1-8, a memory error in the `drive` trampoline frame outside any `pcall`, which can leave `loop_running` or `compacting` set for the rest of the section. Revisit when a run reports `LOOP_RUNNING` after a caught memory error, or when a heap-limit fault-injection harness exists.
- D1-2, D1-3, and D1-19, already reserved by the design record's Deferred list (precheck counting of tool schemas and images, and `Arc` reuse in `append` and `replace`).
- The chat-shape copies in `harness-stop.rs`, Workshop `agents.rs`, and `agents/revoke.rs`. Revisit if one of them starts testing the loop's history.
- The guide's teaching example of the summarizing compactor, which stays unbounded for clarity.

</decision-record>
<project-survey>

## Project Survey

- Status: complete
- Build command: `cargo build --locked -p gateway` (the workspace's only default member, so plain `cargo build` builds just the gateway). The desktop app is `cargo build --locked -p workshop`, which first needs `npm ci --prefix crates/workshop` and `npm ci --prefix crates/gateway/config-ui/ui` because the crates' build scripts bundle the UIs. Aliases in `.cargo/config.toml`: `cargo xtask` (runs `build-xtask`) and `cargo workshop` (runs the `build-workshop` orchestrator).
- Focused test command pattern: `cargo nextest run --locked -p <crate> --all-features -E 'test(/<test-name-regex>/)'`. Drop `--all-features` for `workshop`, `workshop-server`, and `workshop-server-api`. A single Workshop UI test file: `node --test crates/workshop/ui/test/<file>.mjs` (after `npm ci` in `crates/workshop`).
- Component test command pattern: `cargo nextest run --locked -p <crate> --all-features`, with the same `--all-features` exception for the three workshop crates. Workshop UI packages: `npm test --workspaces --if-present` in `crates/workshop`. Gateway config UI: `npm test` in `crates/gateway/config-ui/ui`.
- Full-suite test command: `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`, then `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`. The workspace run includes `build-xtask`, whose tests are the boundary and structural checks (product and container boundaries, Workshop tier graph, `## Invariants` marker, lint inheritance). Also run the wording and rulebook scan `node --test crates/workshop/ui/test/docs-claims.mjs`, which needs no `npm ci`.
- Linter command: with `CARGO_BUILD_WARNINGS=deny` (PowerShell: `$env:CARGO_BUILD_WARNINGS='deny'`), run `cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features` and `cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`, plus `cargo check -p gateway --no-default-features` for the headless build shape. Never run a standalone `cargo check --workspace` beside them.
- Formatter check command: `cargo fmt --all --check` (`rustfmt.toml` sets `style_edition = "2024"`; the pre-commit hook runs the same check).
- Docs command: with `RUSTDOCFLAGS="-D warnings"` (PowerShell: `$env:RUSTDOCFLAGS='-D warnings'`), run `cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api` and `cargo doc -p runvil --no-deps`. CI also runs `cargo doc -p harness --no-deps`, `cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`, and `cargo doc --locked --no-deps -p workshop-server --document-private-items`. When the `runvil` facade surface changes, run `cargo +<pinned nightly> xtask api --check` with `CARGO_BUILD_WARNINGS` unset; the nightly is named in `crates/build-xtask/src/api/toolchain.rs`, and the check compares against `crates/runvil/public-api.txt`.
- Test placement and naming conventions:
  - Unit tests: small ones sit inline in `#[cfg(test)] mod tests { ... }`. Larger ones move to a sibling file wired as `#[cfg(test)] #[path = "foo-tests.rs"] mod tests;`, or to `foo/tests.rs` or a `foo/tests/` directory once there are three or more files (for example `crates/runvil-internal/engine/src/execute/tests/`).
  - Integration tests: each crate has one test binary directory, `tests/it/` or `tests/suite/`, holding `main.rs`, one module file per behavior area, and a `support/` helper module (some crates use `tests/common/`). Fixtures live in `tests/fixtures/` or, for prompt files, `tests/prompts/` split into `valid/`, `invalid/`, and `execution/`.
  - Test function names are snake_case sentences stating the behavior, such as `one_parent_cancel_ends_every_run_holding_one_of_its_children`. The `runvil` facade suite returns `TestResult`; `unwrap` and `expect` are allowed in tests by `clippy.toml`.
  - Engine test helpers live in `src/test_support/`, and `build-xtask` checks that test support does not leak into non-test builds. Gateway STT crates use `src/test_fixtures/`.
  - Engine tests never sleep, read the wall clock, wait with a timeout, or draw OS entropy. Pseudo-random input uses a fixed seed that the failure prints. Temp directory names come from the test name, the process id, and a counter.
  - Benches are criterion targets with `harness = false`: `models_loop` in `runvil-engine` and `surface` in `runvil-lua`.
  - Workshop UI tests run under `node --test` with jsdom, in `crates/workshop/ui/test/*.mjs` and `crates/workshop/ui/src/**/*.test.mjs`.
  - Behavior changes ship with tests in the same change, and refactors preserve existing behavior tests.
- Directory map:
  - `crates/runvil`: the Engine facade crate that re-exports the private Engine crates; its public surface is pinned in `public-api.txt`.
  - `crates/runvil-internal/`: manifestless container for the private Engine crates `runvil-types`, `runvil-vfs`, `runvil-model-client`, `runvil-lua`, `runvil-parser`, and `runvil-engine`.
  - `crates/runvil-plugin`: the Pack and Plugin API.
  - `crates/harness`, `crates/harness-internal/` (`harness-runner`, `harness-plugin-loader`, `harness-test-plugin`), and `crates/harness-gateway-client`: the Harness and its gateway client.
  - `crates/plugin-web`, `crates/plugin-mcp`, `crates/plugin-user-input`: Plugins that install Packs.
  - `crates/gateway/`: the gateway family. `app` is the `gateway` crate and `runvil-gateway` binary; beside it are `config`, `config-ui`, `local`, `logging`, `progress`, `protocol`, `routing`, `web-search`, `cloud-providers`, and the STT subsystem under `stt/` (`api`, `engine`, `backend-whisper`, `whisper-ffi`). `crates/gateway-api-types` and `crates/gateway-api-discovery` are the public wire types and process discovery that outside crates use.
  - `crates/workshop/`: Workshop, the desktop Host. `desktop` is the Tauri `workshop` crate (`runvil-workshop` binary); `server`, `server-api`, `agents`, `gateway`, `menu`, `protocol`, `registry`, `run-log`, `status`, `support`, `user-state`, and `workspace` are its Rust crates; `ui`, `look`, and `platform` are its TypeScript npm workspaces.
  - `crates/shared-error-source`, `crates/shared-loopback`: small shared Rust utilities. `crates/shared-ui` is a TypeScript and CSS package for the gateway config UI, not a Rust crate.
  - `crates/build-*`: build tooling. `build-xtask` (structural checks, facade API check, site), `build-workshop` (desktop build orchestrator), `build-ui`, `build-user-guide`, `build-llama-cuda`, `build-ceiling`.
  - `guide/`: the documentation site, with `books/` for `language`, `workshop`, and `gateway`, plus `landing/` and `chrome/`.
  - `vibe/`: dated plan and debt documents (`YYYY-MM-DD-N-<slug>.md`, older ones filed in `YYYY-MM/` folders).
  - `prompts/`: example prompt files. `tools/`: Node scripts CI uses (gateway sidecar staging, live TTS check). `images/`: README art. `local/`: gitignored operator config and scratch.
  - Root config: `.github/workflows/` (CI, nightly, releases, site), `.githooks/` (pre-commit format check; pre-push wording scan, headless gateway check, clippy, cargo-deny), `.config/nextest.toml`, `.cargo/config.toml` (rust-lld on Windows, aliases), `clippy.toml`, `deny.toml`, `dist-workspace.toml`, `rust-toolchain.toml` (stable), `gateway.local.example.toml`.
- Component boundaries:
  - Engine, bottom up: `runvil-types` and `runvil-vfs` are leaves; `runvil-model-client` depends on types; `runvil-lua` on model-client, types, and vfs; `runvil-parser` on lua and types; `runvil-engine` on all five; `runvil` re-exports them. Engine crates never depend on the Harness, the gateway, Workshop, or `tokio`, `tokio-util`, `async-trait`, `reqwest`, `getrandom`, or `fastrand`, even transitively.
  - `runvil-plugin` depends on `runvil-types` and `runvil-vfs`. The Plugins `plugin-web`, `plugin-mcp`, and `plugin-user-input` depend only on `runvil-plugin`.
  - Harness: `harness-runner` depends on `runvil` and `runvil-plugin`; `harness-plugin-loader` on `runvil-plugin`; `harness` on runner, plugin-loader, `runvil`, and `runvil-plugin`. `harness-gateway-client` depends on `harness`, `plugin-web`, and `runvil`.
  - Gateway family: `gateway-api-types` is a leaf; `config` depends on api-types; `protocol` on api-types and config; `routing` on config and protocol; `local` on config, progress, protocol, and routing; `stt` on `stt-engine` and `backend-whisper`, which wraps `whisper-ffi`; the `gateway` app sits on top. The gateway depends on no Engine or Harness crate.
  - Workshop: `workshop-protocol` and `workshop-support` are leaves; `workshop-registry` depends on protocol; `menu`, `status`, `user-state`, `workspace`, and `workshop-gateway` sit on protocol, registry, and support; `workshop-agents` depends on `harness`, `runvil`, and `plugin-user-input`; `workshop-run-log` on `harness`; `workshop-server` aggregates the Harness, the Plugins, and every workshop crate; `workshop-server-api` wraps the server; the desktop `workshop` crate depends on `workshop-server-api` and `gateway-api-discovery`. Workshop reaches the gateway only through `gateway-api-discovery` and `gateway-api-types`.
  - `build-xtask` enforces these product, container, and Workshop tier boundaries. A dependency cycle means the design is wrong.
- Conventions summary:
  - Rust 2024 on stable, resolver 3, with an MSRV of 1.95 for the Engine crates. Dependencies are declared once in `[workspace.dependencies]`, and every non-obvious version or feature choice carries a comment explaining why.
  - Workspace lints: clippy `all` and `pedantic` denied, `unwrap_used`, `expect_used`, and `todo` denied outside tests, `#[allow]` denied in favor of `#[expect(..., reason = "...")]`, `unsafe_code` denied outside its owned boundary, `missing_docs` and `unreachable_pub` warned, rustdoc broken links denied. Process-global installers such as `set_hook` and `set_global_default` are disallowed except in a binary entry point.
  - Source directories are flat. One or two related files sit beside the parent as `foo-bar.rs` wired with `#[path = "foo-bar.rs"] mod bar;`; three or more become a `foo/` subdirectory, and groups convert back and forth as they cross that line.
  - Five capitalized defined terms (Engine, Harness, Host, Pack, Plugin) with strict usage; Engine crates call their driver "the caller" and never mention the Host. `docs-claims.mjs` enforces this in AGENTS.md files, `## Invariants` crate docs, and `.cursor/rules`.
  - Crate docs carry an `## Invariants` section. Comments explain only non-obvious constraints, and every workaround cites its upstream issue URL.
  - Error and status messages are written for a model to read: concise and self-contained, naming what is missing with required versus actual.
  - Cargo features gate only real constraints such as toolchains or heavy native builds. JSON that reaches a recorder round-trips exactly, with sorted keys and finite numbers.
  - Workshop UI CSS uses only `@workshop/look` and component token custom properties, never raw values; every persisted value goes through the `ui-storage` adapter into one of two allow-listed server buckets.
  - Commit subjects are short sentence-case imperatives (for example "Compact a section's messages in place on overflow or demand"), and a finished plan closes with "Close plan: <slug>".

</project-survey>
<execution-plan>

## Execution Instructions

Components, in dependency order:

1. `OverflowReason cleanup` (D1-1, D1-21, C-1, D1-22): first, because it needs nothing and shares `compactors.rs` with the next component, so the M-1 doc edit lands on the cleaned file.
2. `Direct fail classification` (M-1): second, after the cleanup it shares a file with. It needs no other component, and the guide documents it.
3. `Chat agent compaction` (D1-20): third. It needs neither earlier component, and the guide documents it.
4. `Language guide` (`runvil-docs`): last, because it documents the behavior components 2 and 3 ship.

Each component is one piece built as one sequential step: the piece's code and its tests change together, and one set of tests covers the whole piece, so a finer split would leave a commit whose tests cannot pass alone. Line numbers are at the endpoint `1e808b41f`; Step 1 shifts the later lines of `compactors.rs`.

<step-1>

### Step 1: Remove the unused OverflowReason items and their stale prose [completed]

- Component: `OverflowReason cleanup`
- Tests (run from `c:\Users\Vinnie\cursor\runvil` in PowerShell):
  - `git grep -n -e "OverflowReason::ALL" -e "impl std::error::Error for OverflowReason" -e "whole active compactor contract" -e "only shipped policy" -- crates` (passes when it prints nothing)
  - `cargo nextest run --locked -p runvil-lua --all-features -E 'test(/compactors/)'`
  - `cargo fmt --all --check`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc -p runvil-lua --no-deps --all-features`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`
  - `Remove-Item Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue; cargo +nightly-2026-09-05 xtask api --check` (passes unchanged, since no facade item moves)

- Commit in `runvil`: "Remove the unused OverflowReason items and their stale prose".
- Why: `OverflowReason::ALL` lost its two readers in `c952b5c78` and is read only by one test; `impl std::error::Error for OverflowReason {}` is read only by its own test; neither is in `crates/runvil/public-api.txt`. Delete both rather than gate them with `#[cfg(test)]`, and fold the two stale texts beside them into this commit, since all four sit in the same two files.
- In `crates/runvil-internal/lua/src/compactors.rs`:
  - delete `pub const ALL` with its doc (lines 51-53) and `impl std::error::Error for OverflowReason {}` (line 89);
  - rewrite the `OverflowReason` type doc (lines 36-39), whose "The reason is the whole active compactor contract" is false since `511386d85` and `c952b5c78`: it names the two overflow tags the loop passes to a compactor, which `exhaustion_detail` turns into a description, and says a compactor may also receive a caller's own reason through `models.compact`;
  - keep the exhaustive `match` in `OverflowReason::tag` and in its `Display` impl, the check that forces a new variant to get a tag and a description.
- In `crates/runvil-internal/lua/src/compactors-tests.rs`:
  - `exhaustion_detail_is_an_overflow_tags_display_and_any_other_reason_itself` (line 153) iterates `[OverflowReason::Precheck, OverflowReason::Provider]`, the idiom at lines 86 and 169, adjusting any dereference;
  - delete `an_overflow_reason_is_a_standard_error_whose_text_is_its_display` (lines 160-165);
  - the comment in `the_omitted_compactor_defaults_to_fail` (line 69) names `compactors.fail` as the omitted-compactor default instead of "the only shipped policy", false since `compactors.clear` shipped in `511386d85`.
- Acceptance: the `git grep` prints nothing, the `compactors` tests pass including `exhaustion_detail_is_an_overflow_tags_display_and_any_other_reason_itself`, `public-api.txt` is unchanged, and every listed gate passes.

</step-1>

<step-2>

### Step 2: End the run as context exhaustion on an uncaught direct compactors.fail [completed]

- Component: `Direct fail classification`
- Tests (run from `c:\Users\Vinnie\cursor\runvil` in PowerShell):
  - `cargo nextest run --locked -p runvil-engine --all-features -E 'test(/an_uncaught_direct_compactors_fail_ends_the_run_as_context_exhausted/)'`
  - `cargo fmt --all --check`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc -p runvil-lua --no-deps --all-features`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`
  - `Remove-Item Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue; cargo +nightly-2026-09-05 xtask api --check` (passes unchanged, since no facade item moves)

- Commit in `runvil`: "End the run as context exhaustion on an uncaught direct compactors.fail".
- Why: an uncaught direct `compactors.fail(reason)` in a block ends the run as kind `Lua`. The call raises `Error::ContextExhausted` as an mlua external error (`compactors.rs`, lines 308-317); `block_failure` (`crates/runvil-internal/lua/src/vm/run.rs`, lines 185-219) classifies only stashed error tables; anything else maps through `LuaProgram::map_runtime_error_among`, which special-cases only `QuotaExceeded`. Target: it ends as `RunErrorKind::ContextExhausted` with its reason, exactly as the `pcall`-then-`error(err)` workaround does today. Vinnie approved this change to the run error kind hosts see (2026-10-22).
- Write the test first, in `crates/runvil-internal/engine/src/execute/tests/models_loop_compactors.rs`, modelled on `an_uncaught_compactors_fail_with_a_callers_reason_ends_the_run_as_context_exhausted` (lines 77-102):
  - `an_uncaught_direct_compactors_fail_ends_the_run_as_context_exhausted` runs `loop_scenario` over a `one_section` holding `compactors.fail('manual')` then `return 'unreachable'`, against a `ScriptedChat` holding `text("unreachable")`;
  - it asserts `error.kind() == RunErrorKind::ContextExhausted`, that the error's message contains `manual`, that `error.inner()` matches `Error::ContextExhausted { reason, .. }` with `reason == "manual"`, and that `gateway.call_count() == 0`;
  - add the direct call to the module doc's list of what the file tests;
  - run it and confirm it fails with kind `Lua` before the fix, because the callback error maps to `Error::LuaRuntime`.
- Then, in `crates/runvil-internal/lua/src/program.rs`, `map_runtime_error_among` (line 196): right after the `QuotaExceeded` case, return `Error::ContextExhausted { reason }` (the reason cloned) when `error.downcast_ref::<Error>()` yields `Error::ContextExhausted { reason }`. mlua 0.12.0's `downcast_ref` looks through the `CallbackError` that wraps a callback's `Err`. Match only `ContextExhausted`, so `Store`, `Internal`, and `Lua` external errors keep their current kinds; classifying every typed external error by kind was rejected for that reason.
- `block_failure` needs no edit: the new value passes through its `None => mapped` arm, and cancellation still wins because `map_chunk_failure` checks the cancel flag first.
- Update the doc comments that state the old rule:
  - `program.rs` lines 186-195, the `map_runtime_error_among` doc ("All remaining failures return `Error::LuaRuntime`");
  - `vm/run.rs` lines 173-184, the `block_failure` doc;
  - the `install_compactors` doc in `compactors.rs` (lines 291-302 before Step 1), which says only the loop normalizes the raise.
- Accepted side effects, matching the workaround: an uncaught direct call in H1 ends as `ContextExhausted` instead of `RequirementsUnmet` (`step.rs` lines 186-197 remap only `Lua` and `LuaRuntime`), and a fanout arm or called section reports `context_exhausted` instead of `lua`. No existing test runs an uncaught direct call; if one turns up asserting the old kind, update it to the new one.
- Acceptance: the new test fails first with kind `Lua` and passes after the change, `public-api.txt` is unchanged, and every listed gate passes.

</step-2>

<step-3>

### Step 3: Summarize older chat turns when a turn overflows [completed]

- Component: `Chat agent compaction`
- Tests (run from `c:\Users\Vinnie\cursor\runvil` in PowerShell):
  - `cargo nextest run --locked -p workshop-server -E 'test(/gate_context_overflow_summarizes_older_turns/)'`
  - `cargo nextest run --locked -p runvil-engine --all-features -E 'test(/the_chat_agent_shape/)'`
  - `cargo fmt --all --check`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc -p runvil-lua --no-deps --all-features`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`
  - `Remove-Item Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue; cargo +nightly-2026-09-05 xtask api --check` (passes unchanged, since no facade item moves)

- Commit in `runvil`: "Summarize older chat turns when a turn overflows".
- Why: the Workshop chat agent (`crates/workshop/agents/agents/chat.md`, lines 30-34) runs `pcall(function() return models.loop() end)` under the default `compactors.fail`. After one overflow the records are restored, the next turn appends another user record, and every later turn overflows again while the `pcall` swallows each failure. Vinnie chose summarizing with one `models.infer` call that keeps the last two user turns, over trimming or clearing (2026-10-22).
- Write the Workshop chat-gate test first, in a new `crates/workshop/server/tests/it/chat_gate/compaction.rs` declared as `mod compaction;` beside the other gate modules in `chat_gate.rs`:
  - `gate_context_overflow_summarizes_older_turns_and_later_turns_answer`, with a doc comment in the style of the other gates, runs the real embedded `chat.md` through `connect_chat` and `launch_chat` over a test-local mock gateway built like `spawn_chat_server_with_selection` (typed catalog, menu selection, captured request bodies);
  - the mock returns HTTP 400 with `context_length_exceeded` in the body for any chat request carrying more than 4 messages. The compacted retry holds 4 records (the summary, the second-to-last user turn, its reply, and the last user turn), which the projection may send as 3 wire messages by merging the two leading user records. A round compacts at most once, so the limit must admit the retry either way, and 4 does while every uncompacted turn from turn 3 on exceeds it;
  - the mock streams the reply `summary`, built like `echo_stream`, to a request whose last user message starts with `Summarize this conversation:`, and otherwise echoes as `echo_stream` does;
  - turn 1 sends 100,000 `x` characters, and turns 2 to 4 send short text. The typed catalog's 200,000-token window keeps the precheck quiet, so only the mock's limit overflows: turn 3's request carries 5 messages and turn 4's carries 6;
  - for turns 3 and 4, drain the overflowed round's error frame whose message contains `Model turn failed` with `socket.recv_until`, as GATE 6 does (`recovery.rs`, lines 78-86), because `collect_turn` and `next_wait_token` panic on error frames. Then assert the turn delivers an `agent_message` event whose content is `echo:` plus that turn's text; the summarizing `models.infer` round may emit its own `summary` message first, so collect until the echo arrives;
  - assert every summarize request's message content stays under 4,000 characters, which pins the per-record cap (uncapped, turn 3's would exceed 200,000), and that the request sent right after each summarize request has a first user message starting with `Summary of the earlier conversation: `;
  - run it and confirm it fails before the fix: turn 3 delivers no reply, because `compactors.fail` raises and the agent's `pcall` swallows it.
- Then change `chat.md`:
  - the `## Conversation` block defines the local `compact(reason)` shown in Technical Design, copied as written, and calls `pcall(function() return models.loop(compact) end)`;
  - the prose (lines 15-19) gains one sentence: when the conversation outgrows the model's context window, older turns are summarized so the conversation continues.
- Behavior the compactor delivers:
  - when a turn overflows at the precheck or the provider, every record after the leading system block and before the second-to-last user record is summarized by one `models.infer` call into one user record whose content starts `Summary of the earlier conversation: ` and which carries `meta = { summary = true }`, and the turn retries once;
  - each summarized record contributes at most 1,500 characters of its JSON-encoded content, and the input totals at most 48,000 characters, filled from the newest summarized record backward. That keeps the infer prompt near 12,000 estimated tokens, inside the `min_context: 32768` that `chat.md` declares; an unbounded input was rejected because the summary request would overflow in the very case that triggers it. When the range exceeds the budget, its oldest records, an earlier summary included, drop out of the new summary;
  - when the second-to-last user record is the first record after the system block (exactly two user turns), there is nothing to summarize, so it calls `compactors.fail(reason)` and that turn fails as today; the next turn can summarize.
- Give the chat-shape copy in `crates/runvil-internal/engine/src/execute/tests/models_loop-author-shapes.rs` (lines 35-59, whose module doc says it guards the shipped chat agent's turn loop) the same `compact` and `models.loop(compact)`, through one Lua builder that takes the input list and serves both tests:
  - `the_chat_agent_shape_completes_both_turns` keeps its assertions, since no turn overflows and `compact` never runs;
  - a new `the_chat_agent_shape_summarizes_older_turns_when_a_turn_overflows` runs inputs `{ string.rep('x', 6000), 'q2', string.rep('y', 9000) }` against `text` replies `a1`, `a2`, `summary`, `a3` under `loop_scenario`'s 4,096-token window. Turn 3's estimate (about 3,771 tokens plus the 512-token reserve) overflows the precheck, and its retry (about 2,278 tokens) fits. It asserts that the last record reads `a3` and that `messages[1].meta.summary` is true;
  - the Engine crate's wording rule bars the words "workshop" and "host" anywhere in that file, comments included.
- Out of scope: the chat-shape copies in `harness-stop.rs` (`CHATS`), Workshop `agents.rs` (`ECHO_MD`), and `agents/revoke.rs` (`ROOTS_MD`), which test stops, sockets, and roots rather than the loop's history.
- Acceptance: the Workshop test fails first as described and passes after the change, both chat-shape Engine tests pass, the shipped agent answers the turn after an overflow and every turn after it, and every listed gate passes.

</step-3>

<step-4>

### Step 4: Document the direct compactors.fail run error and the chat agent's compactor [completed]

- Component: `Language guide`
- Tests (run in PowerShell):
  - `Get-ChildItem c:\Users\Vinnie\cursor\runvil-docs\src -Recurse -Filter *.md | Select-String -Pattern ([string][char]0x2014)` (passes when it prints nothing)
  - ``git -C c:\Users\Vinnie\cursor\runvil-docs grep -n -F -e 'run error kind `Lua`, not `ContextExhausted`' -e "pcall(compactors.fail, 'precheck')" -- src`` (passes when it prints nothing)
  - from `c:\Users\Vinnie\cursor\runvil`: `$env:RUNVIL_DOCS='c:\Users\Vinnie\cursor\runvil-docs'; cargo xtask site --books-only` (passes when it builds and its link check passes)

- Commit in `runvil-docs` (`c:\Users\Vinnie\cursor\runvil-docs`): "Document the direct compactors.fail run error and the chat agent's compactor".
- Depends on Steps 2 and 3, whose behavior it documents: an uncaught direct `compactors.fail(reason)` now ends the run as `ContextExhausted` with its reason, and the Workshop chat agent passes a summarizing compactor to `models.loop`.
- Run mechanics, because this step's commit lives in a second repository:
  - the code, review, fix, and commit-message dispatches name `c:\Users\Vinnie\cursor\runvil-docs` as the repository and the `runvil` vibe copy as the plan file, and the provisional `[WIP] Step 4:` commit and its final message live in `runvil-docs`;
  - the review's component base is the `runvil-docs` parent of this step's commit;
  - Verify runs with `c:\Users\Vinnie\cursor\runvil` as the repository, at scope `FULL` with diff base `none`, and the Tests commands above name `runvil-docs` explicitly;
  - the ` [completed]` marker on this heading lands in the `runvil` close commit, since this step has no `runvil` commit to amend. On resume, a `runvil-docs` HEAD subject starting with `[WIP] Step 4:` is this step's provisional commit, and a finalized step 4 commit in `runvil-docs` under an unmarked heading means only the mark and the close remain.
- In `src/language/11-conversations.md`:
  - delete the direct-call paragraph and its workaround block (lines 2499-2506, from "A direct call to `compactors.fail` outside any loop or compaction ends the run differently." through "This block ends the run with run error kind `ContextExhausted` and the reason `precheck`.");
  - in line 2497, extend "whether `models.loop` raised it or your Lua caught it with `pcall` and raised it again with `error(err)`" to name an uncaught direct `compactors.fail(reason)` call too, so the section still covers the direct call;
  - line 2349's last sentence ("Left uncaught, the two end the run differently, ...") says both raises end the run alike, as `ContextExhausted` with the reason, keeping its link to [When context exhaustion goes uncaught](#when-context-exhaustion-goes-uncaught);
  - line 251 shows the Workshop chat prompt calling `pcall(function() return models.loop(compact) end)`, where `compact` summarizes older turns when a turn overflows (link [Writing your own compactor](#writing-your-own-compactor)), and keeps its point that a failed loop ends only that turn;
  - keep line 2324's point that a failed compaction leaves `messages` as it was, so the chat agent loses only that turn;
  - leave the teaching example of the summarizing compactor (lines 2416-2456) unchanged; it stays unbounded for clarity.
- In `src/language/05-lua-environment.md` line 2812, except a `context_exhausted` raise from "An uncaught error from a built-in Lua function ends the run with run error kind `Lua`": an uncaught `compactors.fail` ends the run as `ContextExhausted`.
- In `src/language/16-limits-and-errors.md`, the `ContextExhausted` table row (line 809) and paragraph (line 868) name an uncaught direct `compactors.fail(reason)` call, with any reason, as a cause.
- In `src/language/17-quick-reference.md` line 313, replace "or a caught `context_exhausted` error value is raised again" with the uncaught direct `compactors.fail` call.
- House rules: a single dash (never an em dash or a double dash), four-backtick fences, one line per paragraph. If a heading changes, update every `#<old slug>` link.
- Acceptance: all three guide checks pass.

</step-4>

</execution-plan>
