---
name: Runvil json debt removal
overview: "Remove the debt that the json-global and plugin-rename commits (07e7e01b1..51c4b234e) added to runvil: frozen argv nulls that encode wrongly (the user picked option A, nested nulls read json.null), and unbounded Lua-to-JSON conversion in json.encode and var writes, fixed together with one size budget."
todos:
  - id: w1-argv-null
    content: "W1 (D1-2): write the three regression tests and confirm they fail at the endpoint; in argv.rs make nested JSON null freeze as json.null (top-level null stays nil); update args_surface.rs line 138"
    status: pending
  - id: w2-confirm
    content: "W2 step 1: write the budget, C-1 list, held-key, debug-mode, encode_lines, and var.x tests in tests/json/limits.rs and sys_and_var.rs; run at endpoint and record which fail (skip changes for any that pass)"
    status: pending
  - id: w2-budget
    content: "W2 steps 2-4: add walk_budget.rs and HeapCeiling app data in vm.rs; move Encoder and keys.rs to the budget with lazy value reads, held-key cap, too_large refusal, HashSet in resolve; add check_value before from_value in sys.rs line 92"
    status: pending
  - id: exit-checks
    content: Run the exit checks from AGENTS.md (nextest, clippy, fmt, build-xtask, doc, xtask api --check) and commit
    status: pending
isProject: false
---

# Runvil json debt removal

<product-contract>

## Product Requirements

- Scope and target work
  - Repository: `c:\Users\Vinnie\cursor\runvil`. The guide edits for these changes belong to the sibling repository `c:\Users\Vinnie\cursor\runvil-docs`, a separate git repository, so they are outside this run (see Deferred and Out of Scope).
  - Baseline: `07e7e01b131f0de5c27ae18d2f314d4a349f694c` (`upstream/master`, an ancestor of HEAD). Endpoint and disposition ref: `51c4b234e516c598db8a6af89d90e295771f61c5` (HEAD, clean worktree, identical to the endpoint).
  - Target commits: `4453bc29a` (json decode, null, array, var empty-list marker), `600a83428` (json encode, argv list marker), `fd91a35b1` (close json-global plan), `0988c99f8` (ToolInfo and ModelInfo rename), `29051b256` (PluginInfo, PluginRegistry, Builtin rename), `51c4b234e` (close rename plan).
  - Design records read as testimony only: `vibe/2026-10-18-1-json-global.md` and `vibe/2026-10-18-2-plugin-registry-rename.md`. No `vibe/ACTIVE` exists. Nothing was built or run during analysis, so every runtime magnitude below is inferred from code.
- Cleanup goals and non-goals
  - Goal: `json.encode(argv)` returns the stored data outside H1, as the json plan record acceptance criterion (line 132) requires.
  - Goal: no sandboxed Lua value can make `json.encode` or a `var` write abort the Host, panic inside mlua, or hold the VM thread past cancellation, while every value that fits the Lua heap still converts.
  - Non-goals: the guide edits in `runvil-docs`, the Plugin vocabulary recurrence, the six other `from_value` sites, `tools.calls`, replay flags, and cancel polling (see Deferred and Out of Scope).
- Success criteria
  - Every regression test below fails at the endpoint for the stated reason and passes after the change.
  - The existing tests named under Testing Plan still pass, and the exit checks pass.

## Functional Specification

### Debt Inventory

- Debt added (introduced)
  - D1-2, introduced by `600a83428`. `crates/runvil-internal/lua/src/argv.rs` `frozen_json_value` (lines 68-95) maps JSON null to `Value::Nil` (line 70, root dates to `2109f209e`), so a null list item leaves a hole and a null object field has no key. `600a83428` then made `json.encode` read frozen `argv` through to that data (`json/encode.rs` `resolve` 331-366, `json/keys.rs` `shape` and `missing_item` 43-86). Traced results for frozen argv: `{"ids":[1,null,3]}` raises `json.encode: value.ids is missing list item [2]`, `{"ids":[1,null]}` silently encodes `[1]`, `{"ids":[null]}` encodes `[]`, `{"a":null}` encodes `{}`. In H1, `install_writable` (argv.rs 45-51) uses `lua.to_value`, which keeps `json.null`, so the same args encode correctly there. Impact: valid stored data fails with a message that blames the data, or loses items silently. Reversal cost: medium (documented read behavior). Target state: nested JSON null reads `json.null` in frozen argv, matching H1 and `var`. User-resolved, option A.
  - D1-3, introduced by `600a83428`. `json/encode.rs` `Encoder::table` (197-228) tracks only the current path (push 219, pop 226, check 211), so a table reachable twice is walked twice and every string occurrence is copied (line 180). There is no node or byte count, the whole walk runs inside one Rust callback (the instruction hook in `hardening.rs`, interval 10,000, cannot fire inside it), and no `global_allocator` exists in `crates/`. `local t = {} for i = 1, 40 do t = { t, t } end return json.encode(t)` stays under the 127-level depth limit, has no cycle, and asks for about 2^41 tree nodes on the Rust heap, outside the 64 MiB Lua limit (`vm.rs` line 48). It contradicts the json plan record line 125 ("The byte cap and both depth limits bound the work a single call does") and `lua/src/lib.rs` lines 7-9 (cancel aborts even an unbounded loop). `resolve` also detects view loops with `Vec::contains` (lines 333-354), which is quadratic in view-chain length. Reversal cost: low.
  - C-1, introduced by `600a83428`, found by the challenger. `json/keys.rs` `entries` (91-101) collects every `(key, value)` pair of a table at once. In mlua 0.12 each live string, table, function, thread, or userdata `Value` holds one slot on the auxiliary reference stack, and `ref_stack_pop` (mlua-0.12.1 `src/state/extra.rs` 276-301, same code in the locked 0.12.0) panics with "cannot create a Lua reference, out of auxiliary stack space" when `lua_checkstack` cannot grow it; Lua 5.5 caps a stack at 1,000,000 slots. `local t = {} for i = 1, 1000000 do t[i] = 'a' end return json.encode(t)` fits the 64 MiB heap and should reach the panic. The panic escapes the `lua`-kind error contract (json plan record line 104). The Engine has no `catch_unwind` outside tests, so it unwinds into the Host. Not executed: step 1 of W2 confirms it.
- Cheap fix
  - D1-4, narrowed by the challenger to `crates/runvil-internal/lua/src/sys.rs`. Line 92, `lua.from_value::<Json>(value)` in the `var` `__newindex` closure, converts a shared-table value once per occurrence on the Rust heap before any Lua allocation, so `var.x = t` with the doubling table has the same process-level failure. The defect predates the target, but `4453bc29a` touched `sys.rs`, and the fix needs no hard-to-reverse change. `collection.rs` line 138 is a test helper and line 120 is not an independent exposure (its conversion already happened at `protocol/parse/tasks.rs` line 53), so `collection.rs` is not part of this fix.
- Exposed pre-existing debt, reported separately and not remediated
  - D1-5, exposed. The Plugin vocabulary has needed repository-wide renames twice under different plans: `77b210b2a` (2026-10-04, 132 files, Plan `vibe/2026-10-04-2-plugin-rename.md`) and the target (`0988c99f8`, 83 files, `29051b256`, 60 files). The Harness registry was `PluginRegistry`, removed by `a47d66c51`, `PackageRegistry` at `8ab4d8531`, then `PluginRegistry` again. The target left no residue (rename completeness searches print only the allowed `crates/workshop/server/build.rs` line). A third rename is already predicted by the toolset redesign (rename plan record Deferred line 705).
- Rejected candidate counts (16): residual-but-acceptable 10, weak/speculative 3, false 2, unrelated pre-existing 1.
  - residual-but-acceptable: D1-1 (`json.encode(tools.calls)` gives `{}`, which the published rule in guide `05-lua-environment.md` lines 1428-1438 specifies; `tools.rs` was not touched by the target), D1-6 (json surface growth, specified and tested), D1-7 (`json` reserved, collision message names it), D1-8 (shared `json` table, no new authority), D1-13 (emptied var list drops a string key, edge of the intended change), D1-14 (decode tree bounded by the 8 MiB input cap), D1-15 (integers above i64 decode to floats, specified), D1-16 (no deprecated aliases, explicit decision), D1-17 (install-once rule reserved for the plugin-mcp redesign), D1-18 (FQN stays a string, `PluginFqn` deferred).
  - weak/speculative: D1-9 (install order is code-ordered and tested), D1-10 (flag parameter on `Encoder::new`, no consequence), D1-12 (no replay `Flags` bit exists and no replay reader exists yet).
  - false: D1-11 (the var empty-array repair is complete, `an_empty_list_seeded_from_rust_reads_back_as_an_empty_array` pins it), D1-19 (the renames left no old names or behavior change).
  - unrelated pre-existing: D1-20 (`tools.calls` exposes a mutable metatable).

</product-contract>
<implementation-contract>

## Technical Design

- Frozen argv nulls (D1-2), `crates/runvil-internal/lua/src/argv.rs`
  - `install_frozen` keeps today's top-level rule: `None` or `Some(Json::Null)` installs nil, so `if argv then` stays the malformed-input check (guide `06-arguments.md` lines 765 and 1028; engine test `valid_json_scalars_make_argv_a_number_or_boolean_and_null_reads_as_nil`, `args_surface.rs` 68-84, pins it).
  - `frozen_json_value` takes `&Json` instead of `Option<&Json>` and drops its `None | Some(Json::Null)` nil arm. A nested null then reaches the scalar arm, `lua.to_value`, which yields `json.null` exactly as `install_writable` does. List data tables keep the array marker, so a list `[1,null,3]` has keys 1 to 3 and encodes whole.
  - Update the doc comment on `frozen_json_value` (lines 62-66, "absent or null reads as nil").
  - Public read behavior changes outside H1: `argv.a` for a JSON `{"a":null}` and `argv.ids[2]` for `[1,null,3]` now read `json.null` (truthy) instead of nil, and `pairs` and `#` see those slots.
- One bounded-conversion budget (D1-3, C-1, D1-4), crate `runvil-lua`
  - New module `lua/src/walk_budget.rs` (pub(crate), kept under the 500-line file limit that the crate build scripts enforce). `WalkBudget { nodes, bytes }` with `charge_node()` and `charge_bytes(n)` returning a `TooLarge` marker. `WalkBudget::for_vm(lua)` sets nodes to ceiling/8 and bytes to the ceiling, where the ceiling is the VM's Lua heap limit. Reasoning: an unshared value that fits the heap has at most about ceiling/9 values (an array slot costs at least about 9 bytes) and at most ceiling bytes of string, so only sharing-driven expansion beyond the heap is refused. At the default 64 MiB that is 8,388,608 values, which also covers the most values an 8 MiB `json.decode` input (cap `INPUT_LIMIT` in `json/decode.rs` line 15) can produce, so decode-then-encode stays whole.
  - mlua has no getter for the memory limit (`Lua::set_memory_limit` only returns the previous value), so store the ceiling as Lua app data: a `pub(crate) struct HeapCeiling(pub usize)` set in `vm.rs` next to `lua.set_memory_limit(DEFAULT_LUA_MEMORY_BYTES)` (line 259) and refreshed in `apply_lua_limits` (lines 341-345). `for_vm` falls back to `DEFAULT_LUA_MEMORY_BYTES` when no app data exists. `TestVm::limits` already goes through `apply_lua_limits` (`test_vm.rs` lines 322-325).
  - Also in the module: `HELD_KEY_LIMIT = 100_000`, the most keys held as Lua references along the live encode path (string, table, function, thread, userdata keys; integer, number, and boolean keys hold no slot). With the 1,000,000-slot stack cap, 10 times headroom covers the path's own live references.
  - `check_value(lua, &Value)`: a pre-walk for `from_value` callers. Recursive, lazy over `Table::pairs` (it never collects), counts every value visited including repeats of shared tables, adds string and key bytes, keeps a path vector of table pointers, and stops descending into a table already on the path or past 200 levels, leaving those cases to `from_value` exactly as today (cyclic and very deep values keep mlua's own behavior).
- Encoder, `lua/src/json/encode.rs` and `lua/src/json/keys.rs`
  - `Encoder` gains `budget: WalkBudget` and `held: usize`. Charge one node at the top of `value()` (scalars too), string byte length for each string value, key byte length for each object key, and one node per view hop in `resolve`.
  - `entries` returns sorted keys only. It drops each pair's value immediately (a dropped value frees its reference slot), counts reference-holding keys, and `Encoder::table` adds that count to `held` and subtracts it after the table encodes. `shape` takes the key slice. `list`, `object`, and `fallback` read each value with `data.raw_get(key.clone())` when its turn comes. Key visiting order and error precedence do not change, because keys were already checked before values. `debug_option` and `items` use the same `entries`.
  - New `Failure`-free path: `too_large()` returns the `lua`-kind refusal `{function}: value is too large to encode` (no path). It bypasses `fail()`, so debug mode raises it too. A placeholder per remaining value would not bound the work. The same message covers the node budget, the byte budget, and `HELD_KEY_LIMIT`. `encode_lines` uses one `Encoder` for all items, so its budget is shared and its prefix is `json.encode_lines:`.
  - `resolve` replaces `Vec::contains` over the view chain with a `HashSet` of pointers, removing the quadratic cost. Behavior of the `Loop` result is unchanged.
- `var` writes, `lua/src/sys.rs`
  - In the `guarded_table` `__newindex` closure, after the existing function, userdata, and thread check and before `lua.from_value::<Json>(value)` (line 92), run `WalkBudget::for_vm(lua).check_value(&value)`. On `TooLarge` return `mlua::Error::runtime(format!("{target} is too large to store"))`. mlua's own messages for every other failure are unchanged.

</implementation-contract>
<verification-contract>

## Testing Plan

- Order rule for every regression test: write it first, run it at the endpoint, and confirm it fails for the named reason. A test that already passes proves its finding false: keep the test and skip that change.
- D1-2 regression tests
  - `lua/src/tests/json/views.rs`: with `json!({"ids":[1,null,3],"tail":[1,null],"a":null})`, build one VM with `Argv::Frozen(Some(..))` and one with `Argv::Writable(Some(..))`, and assert `encoded(&vm, "json.encode(argv)")` equals `{"a":null,"ids":[1,null,3],"tail":[1,null]}` for both. At the endpoint the frozen call raises `json.encode: value.ids is missing list item [2]`, so `encoded` fails there. The writable half already passes and stays as the parity guard.
  - `lua/src/tests/argv.rs`: a frozen VM asserts `argv.a == json.null` and `argv.ids[2] == json.null`. At the endpoint both read nil.
  - `engine/src/execute/tests/suite/args_surface.rs` line 138: change `an_h1_argv_holding_json_null_passes_the_freeze_and_reads_the_field_as_nil` to assert `argv.a == json.null` and rename it. At the endpoint `argv.a` reads nil, so `assert(argv.a == json.null, ...)` fails.
  - Guards that must keep passing: `valid_json_scalars_make_argv_a_number_or_boolean_and_null_reads_as_nil` (top-level null stays nil), `frozen_nil_argv_reads_nil_and_rejects_assignment`, `frozen_argv_encodes_its_stored_data` (`views.rs` 29), `frozen_argv_holding_an_empty_list_encodes_it_as_an_empty_array` (`argv.rs` 87).
- D1-3, C-1, D1-4 regression tests, new file `lua/src/tests/json/limits.rs` (register with `mod limits;` in `tests/json.rs`) and one case in `lua/src/tests/sys_and_var.rs`; use the small-heap pattern from `tests/budgets.rs` line 73 (`TestVm::new().limits(4 * 1024 * 1024, DEFAULT_LUA_LOG_EVENTS).shims().build()`) so the endpoint finishes quickly instead of exhausting the machine
  - Doubling table: `t = {} for i = 1, 19 do t = { t, t } end`, then `caught(&vm, "json.encode(t)")` equals `lua_error("json.encode: value is too large to encode")`. The walk visits about 1 million tables against a budget of 524,288 values (4 MiB ceiling divided by 8). At the endpoint the call encodes them all and returns text, so `caught` fails with "required ... to raise".
  - Repeated string: `s = string.rep('x', 2^19)`, `t = { s } for i = 1, 5 do t = { t, t } end`. About 16 MiB of copies against a 4 MiB byte budget raises the same message. At the endpoint it returns text.
  - Debug mode raises too: `json.encode(t, { debug = true })` on the doubling table equals the same error. At the endpoint it returns text.
  - `json.encode_lines({ t })` raises `json.encode_lines: value is too large to encode`.
  - `var.x = t` with the doubling table: the caught `lua` error message is `var.x is too large to store`. At the endpoint the conversion builds the tree and then fails differently (Lua memory error from rebuilding), so the message assertion fails.
  - 1,000,000-item list at the default heap (`TestVm::new().shims().build()`): `local t = {} for i = 1, 1000000 do t[i] = 'a' end`, then assert `json.encode(t)` has length 4,000,001. At the endpoint this should panic in mlua (C-1); if it passes, C-1 is false for lists and the lazy-value change in `entries` is skipped.
  - 150,000-key string object at the default heap: `for i = 1, 150000 do t['k' .. i] = true end` raises `json.encode: value is too large to encode` (above `HELD_KEY_LIMIT`). At the endpoint it encodes, so the assertion fails.
  - Guards that must keep passing: `a_table_that_contains_itself_raises_but_a_shared_table_encodes` (`errors.rs` 208, a shared table in two places still encodes), `a_loop_of_127_tables_repeating_at_level_128_reports_the_cycle` (`errors.rs` 227), `a_var_value_127_levels_deep_encodes_because_views_add_no_level` (`views.rs` 47), the view-loop tests in `views.rs`, and the existing `var` cases in `sys_and_var.rs` (a value 127 levels deep and a shared small value still write).
- Reused checks: `TestVm::limits`, `caught`, `lua_error`, `encoded` in `lua/src/tests/json.rs`. No new structural ratchet, snapshot, line ceiling, or source-shape check is added.
- Exit checks (commands and flags from `AGENTS.md` lines 52-58 and the json plan record Testing Plan lines 220-224): focused `cargo nextest run` on `runvil-lua` for `json`, `argv`, and `sys_and_var`; full `runvil-lua`, `runvil-engine`, and the `runvil-parser`, `runvil`, `harness-runner`, `harness` suites; `cargo clippy` on `runvil-lua` and `runvil-engine` with all targets and features and warnings denied; `cargo fmt --all --check`; `cargo test -p build-xtask` (it enforces the 500-line file limit); the rustdoc commands with warnings denied; and `cargo xtask api --check` to confirm the facade surface is unchanged.

</verification-contract>
<decision-record>

## Decision Record

- Reversible decisions and consequences (confidence on each recommendation is stated)
  - One budget serves D1-3, C-1, and D1-4, because all three are the same cause: Lua-to-JSON conversion inside one Rust call with no bound, outside the instruction hook and outside the Lua heap limit. Confidence high: the challenger and the main-context read agree on the mechanism.
  - Budget size: ceiling/8 values and ceiling bytes. Consequence: only sharing-driven expansion past the heap is refused; Host memory for a budget-sized encode can still be several times the Lua heap (the json plan record line 279 already accepts that kind of cost). Confidence medium: the constants are estimates, not measurements, and nothing was executed.
  - `HELD_KEY_LIMIT = 100_000`. Consequence: one live path holding more than 100,000 string-like keys is refused. Integer-keyed lists are exempt because they hold no reference slots. Confidence medium: the headroom comes from reading mlua and Lua source, not from a run.
  - Debug mode raises the too-large error. Consequence: the guide's "Debug mode never fails on the value" gains one stated exception. Alternative of a `[too large]` placeholder was rejected because each remaining value would still be visited. Confidence high.
  - Error text: `{function}: value is too large to encode` for encode and `{target} is too large to store` for `var`. Reversible and additive; the guide follow-up documents it in the refusal table.
  - `resolve` uses a `HashSet` instead of a linear `Vec::contains`. Cheap, behavior-neutral, covered by the existing view-loop tests, with no wall-clock test.
  - The pre-walk leaves cyclic and over-200-level values to `from_value`, so their messages stay mlua's. Consequence: a cyclic value with fan-out can report the too-large message instead of the recursion message. Acceptable and rare.
- User-resolved architecture choices
  - D1-2: option A, store `json.null` for nested nulls in frozen argv. The user chose it because there are almost no other users. Consequence: prompts that read `argv.field` as nil for a passed null now see a truthy `json.null`, and the later switch back would be a second break.
- Rejected alternatives, assumptions, and risks
  - D1-2 option B (record each list's original length and null object keys so reads stay nil) and option C (document the limitation and point authors to `json.decode(args)`) were offered and not chosen.
  - Refusing any table met twice was rejected: it breaks the documented, test-pinned rule that a shared table encodes (`errors.rs` 208).
  - Fixing only `sys.rs` is what this plan does for D1-4, but the same failure stays reachable through the other `from_value` sites until the user widens the scope. Risk accepted by default because the user did not opt those in.
  - Risk: C-1's panic path into the Host is inferred (no `catch_unwind` outside tests, VM runs on the caller's thread). The order rule in the Testing Plan turns that inference into a checked fact before any change lands.
  - Risk: `HeapCeiling` app data is new VM state. A VM built without it falls back to the 64 MiB default, so no caller can run unbounded.

### Deferred and Out of Scope

- Guide edits in the sibling repository `c:\Users\Vinnie\cursor\runvil-docs` (a separate git repository, so the run's provisional-commit cycle cannot cover them). Revisit when this plan closes, as one commit in `runvil-docs`, then build the guide with `cargo xtask site --books-only` from `runvil` with `RUNVIL_DOCS` set to the docs root. The edits, under `src/language`:
  - `06-arguments.md`: rewrite line 843 (outside H1 a nested JSON `null` reads `json.null`, including a list element) and line 1184 (after `argv = { a = json.null }` a later section reads `argv.a` as `json.null`). Say in the "Missing fields and null values" section (line 805) that an absent field reads nil and a passed null reads `json.null`, and that a top-level `null` still makes `argv` nil. Check the `json.null` section of `05-lua-environment.md` (line 1299) and `07-substitution.md` for sentences that claim argv nulls read nil. Substitution of `{{ argv.note }}` with `{"note":null}` stays `NullValue` because `subst.rs` reads the Rust JSON, not the Lua value.
  - `05-lua-environment.md`: add the new message to the refusal table (line 1469 area) and a paragraph under "Size and nesting limits" (line 1582) giving the three limits in author terms (values counted once per place they appear, string bytes, and keys per path), add the debug-mode exception beside "Debug mode never fails on the value" (line 1520), and note the `var` write refusal. Add a fixed row to `17-quick-reference.md`'s limits table.
- Other `from_value` sites that take author tables, same cause as D1-4: `protocol/parse/tools.rs` line 64, `protocol/parse/tasks.rs` line 53, `messages.rs` line 238, `prose.rs` line 149, `vm/state.rs` lines 47 and 67, `protocol/parse.rs` line 61, plus the hash-part `Vec` in `collection_members`. Revisit when the user expands the scope; `WalkBudget::check_value` is written to be called at each.
- D1-5 Plugin vocabulary recurrence (exposed). Revisit when the toolset redesign names things, and settle the "Plugin id", "tool id", and "wire name" definitions in `AGENTS.md` before code names them (rename plan record Deferred line 705). The `PluginFqn` newtype is already deferred at line 702. Any deprecated alias needs a stated removal condition (D1-16 tension).
- `json.encode(tools.calls)` encoding `{}` (D1-1). Revisit only if a prompt needs the counts; the optional fix is a string `__metatable` on `tools.calls` (`tools.rs` 80-118).
- Cancel polling inside the encoder. Revisit if a budget-sized encode is measured above about one second of cancel latency. The cancel handle already lives in `runvil-lua` (`hardening.rs`).
- A replay `Flags` bit for the var `{}` to `[]` change (D1-12). Revisit when a replay reader exists (`types/src/replay.rs` lines 96-103).

</decision-record>
<project-survey>

## Project Survey

- Status: complete
- Build command: `cargo build --locked -p gateway` (the workspace default member; the desktop app is `cargo build --locked -p workshop`; headless gateway is `cargo build --locked -p gateway --no-default-features`).
- Focused test command pattern: `cargo nextest run --locked -p <crate> <test-name-substring>` (nextest 0.9.x is installed; the substring matches test paths such as `tests::name` or `suite::module::name`). Node tests: `node --test <file>.mjs`.
- Component test command pattern: `cargo nextest run --locked -p <crate>`; the three workshop crates run separately as `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`; structural and boundary checks are `cargo test -p build-xtask`; Workshop UI tests are `npm test --workspaces --if-present` run in `crates/workshop` after `npm ci --prefix crates/workshop`.
- Full-suite test command: `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`, then the workshop crates separately as above, then `node --test crates/workshop/ui/test/docs-claims.mjs` for the wording and rulebook scan (no `npm ci` needed).
- Linter command: `cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features` and `cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`, both with `CARGO_BUILD_WARNINGS=deny` (PowerShell: `$env:CARGO_BUILD_WARNINGS="deny"`); never add a standalone `cargo check --workspace`, and the one extra check is `cargo check -p gateway --no-default-features`.
- Formatter check command: `cargo fmt --all --check` (rustfmt `style_edition = "2024"`; the pre-commit hook runs it).
- Docs command: `cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api` with `RUSTDOCFLAGS="-D warnings"` (PowerShell: `$env:RUSTDOCFLAGS="-D warnings"`); the facade also needs `cargo doc -p runvil --no-deps` without `--all-features`; the facade surface gate is `cargo +<pinned nightly> xtask api --check` with `CARGO_BUILD_WARNINGS` unset, where the nightly is named in `crates/build-xtask/src/api/toolchain.rs`.
- Test placement and naming conventions: unit tests live in a sibling file named `<stem>-tests.rs` (or `tests.rs` inside a module directory), wired by `#[cfg(test)] #[path = "<stem>-tests.rs"] mod tests;`; shared helpers sit in `test_support.rs` or `tests/.../support.rs`; integration tests are one binary per crate under `tests/suite/main.rs` (`runvil`, `harness`) or `tests/it/main.rs` (`runvil-plugin`, `plugin-mcp`, `plugin-user-input`, `harness-runner`), one `mod` per topic file; shipped-prompt fixtures sit in `crates/runvil/tests/prompts/`; benches sit in `benches/`; Node tests sit in `crates/workshop/ui/test/*.mjs` and `src/**/*.test.mjs`; Engine tests never sleep, read the clock, time out, or use OS entropy, and random input comes from a fixed seed that the failure prints; temp directory names come from the test name, process id, and a counter.
- Directory map: `crates/` holds the public and shared crates (`runvil` facade, `runvil-plugin`, `harness`, `harness-gateway-client`, `plugin-web`, `plugin-mcp`, `plugin-user-input`, `gateway-api-types`, `gateway-api-discovery`, `shared-error-source`, `shared-loopback`, `build-*` tooling) beside manifestless family containers: `runvil-internal/` (engine, lua, model-client, parser, types, vfs), `harness-internal/` (runner), `gateway/` (app, cloud-providers, config, config-ui, local, logging, progress, protocol, routing, web-search, `stt/*`), and `workshop/` (desktop, server, server-api, agents, gateway, menu, protocol, registry, run-log, status, support, user-state, workspace, plus the TypeScript packages `ui`, `look`, `platform`); `crates/shared-ui` is a TypeScript and CSS package, not a Rust crate; `guide/` holds the user-guide book sources (gateway, language, workshop) with page chrome and landing styles, rendered into `target/site`; `prompts/` holds example prompts; `tools/` holds Node scripts for the gateway sidecar and TTS; `vibe/` holds dated working notes; `local/` holds local profiles and config; `cabinet/` is scratch and trash staging; `.github/workflows` holds CI; `.githooks` holds pre-commit (fmt) and pre-push (docs-claims, headless check, clippy, deny); `.cursor/rules` holds the Workshop rules; `target/` is build output.
- Component boundaries: the Engine is `runvil-types` and `runvil-vfs` (leaves), `runvil-model-client` (types), `runvil-lua` (model-client, types, vfs), `runvil-parser` (lua, types), and `runvil-engine` (all of those), re-exported through the `runvil` facade; `runvil-plugin` depends on types and vfs, and each `plugin-*` crate depends only on `runvil-plugin`; `harness-runner` depends on `runvil` and `runvil-plugin`, the `harness` facade on the runner, and `harness-gateway-client` on `harness`, `plugin-web`, and `runvil`; the Gateway builds up from leaf `gateway-api-types` through config, protocol, routing, local, progress, and stt to the `gateway` app; Workshop crates are tiered (vocabulary, then domain services, then features, then the server), with `workshop-server` using the Harness family, plugins, and every lower tier, and the desktop `workshop` crate using `workshop-server-api`; a family reaches another family only through a crate at the top level of `crates/`, and `build-xtask` enforces these boundaries.
- Conventions summary: Rust 2024 on stable (Engine floor 1.95); every crate inherits workspace lints (clippy `all` and `pedantic` at deny, no `unwrap` or `expect` outside tests, `allow_attributes` denied, unsafe denied, `missing_docs` warn) and its `build.rs` runs `build_ceiling::check()`, which enforces a 500-line file limit; source directories are flat, and a subdirectory needs at least three files or becomes kebab-named siblings with `#[path]`; Engine crates are sans-I/O and never depend on `tokio`, `reqwest`, or similar; the words Engine, Harness, Host, and Plugin are capitalized with one meaning each and `docs-claims.mjs` enforces them; recorder JSON round-trips exactly (`serde_json` with `float_roundtrip`, sorted keys, finite numbers); error messages are written for model readers; behavior changes ship with tests; doctests and `doc(hidden)` are banned; the `runvil` facade surface is pinned in `crates/runvil/public-api.txt`; `workshop-*`, `harness-*`, and `plugin-*` crates carry an `## Invariants` crate-doc marker; comments explain non-obvious constraints and cite upstream issue URLs for workarounds; Workshop UI CSS uses design tokens only and persists state through `ui-storage`.

</project-survey>
<execution-plan>

## Execution Instructions

- Path FULL. Highest-level objective: `json.encode` returns the stored data of frozen `argv`, and no sandboxed Lua value can abort the Host, panic inside mlua, or hold the VM thread past cancellation through `json.encode` or a `var` write, while every value that fits the Lua heap still converts.
- Components in dependency order. Component 1, Frozen argv nulls (Step 1), goes first because it touches only `argv.rs` and its tests, shares no code with Component 2, and ships on its own as the user-resolved read-behavior change. Component 2, Bounded conversion (Steps 2-4), goes second because it is the larger body of work and has no dependency on Component 1. The two could swap places without breaking either.
- Pieces and construction. Component 1 is one piece built in one step. Component 2 has three pieces built sequentially: Encode work budget (Step 2), Held-key cap with lazy value reads (Step 3), and Var write refusal (Step 4). Sequential, not joint, because Step 3 reuses `walk_budget.rs`, `too_large()`, and the `Encoder` fields that Step 2 adds, and Step 4 reuses `WalkBudget::for_vm` and `TooLarge` from Step 2, while each piece passes its own tests alone. Each new item is added in the first step that uses it, so no commit leaves dead code for the denied clippy lints to reject.
- Every step restates the order rule, the file scope, and the commit rule, because step agents read only their own step. The exit checks from the Testing Plan run as the Tests of Step 4, the last step. The close commit, the plan seed copy, and `vibe/ACTIVE` belong to the Run Sequence, not to any step.

<step-1>

### Step 1: Frozen argv reads nested JSON null as json.null [completed]

- Component: Frozen argv nulls
- Tests:
  - `cargo nextest run --locked -p runvil-lua json::views`
  - `cargo nextest run --locked -p runvil-lua tests::argv`
  - `cargo nextest run --locked -p runvil-engine args_surface`
  - `cargo nextest run --locked -p runvil-lua`
  - `cargo nextest run --locked -p runvil-engine`
  - `cargo fmt --all --check`
  - `$env:CARGO_BUILD_WARNINGS="deny"; cargo clippy -p runvil-lua -p runvil-engine --all-targets --all-features`

Goal (D1-2, user-resolved option A): outside H1, a nested JSON null in frozen `argv` reads `json.null` (as in H1 and in `var`), so `json.encode(argv)` returns the stored data. A top-level null or absent argv still installs nil. Scope: modify files only inside `c:\Users\Vinnie\cursor\runvil`. Do not edit the sibling `runvil-docs` repository.

Order rule: write the three regression tests first, run each at the current tree, and confirm it fails for the stated reason. A test that already passes proves its finding false: keep the test and skip that change.

1. `crates/runvil-internal/lua/src/tests/json/views.rs`, new test `frozen_and_writable_argv_encode_nested_nulls_alike`. With `json!({"ids":[1,null,3],"tail":[1,null],"a":null})`, build one VM with `Argv::Frozen(Some(..))` and one with `Argv::Writable(Some(..))` through the `TestVm` builder's `argv`, and assert `encoded(&vm, "json.encode(argv)")` equals `{"a":null,"ids":[1,null,3],"tail":[1,null]}` for both. At the current tree the frozen call raises `json.encode: value.ids is missing list item [2]`, so `encoded` fails there. The writable half already passes and stays as the parity guard.
2. `crates/runvil-internal/lua/src/tests/argv.rs`, new test `frozen_argv_reads_nested_nulls_as_json_null`. A frozen VM holding `{"a":null,"ids":[1,null,3]}` asserts `argv.a == json.null` and `argv.ids[2] == json.null`. At the current tree both read nil.
3. `crates/runvil-internal/engine/src/execute/tests/suite/args_surface.rs` line 138: rename `an_h1_argv_holding_json_null_passes_the_freeze_and_reads_the_field_as_nil` to `an_h1_argv_holding_json_null_passes_the_freeze_and_reads_the_field_as_json_null` and change it to assert `argv.a == json.null`, with a failure message naming the nil read. At the current tree `argv.a` reads nil, so `assert(argv.a == json.null, ...)` fails.
4. `crates/runvil-internal/lua/src/argv.rs`: change `frozen_json_value` (line 68) to take `&Json` instead of `Option<&Json>` and drop its `None | Some(Json::Null)` nil arm, so a nested null reaches the scalar arm `lua.to_value`, which yields `json.null` exactly as `install_writable` does. The recursive calls pass `value` instead of `Some(value)`. `install_frozen` (line 59) keeps today's top-level rule: `None` or `Some(Json::Null)` installs nil through `crate::globals::freeze_argv(lua, Value::Nil)`, and any other value goes through `frozen_json_value`, so `if argv then` stays the malformed-input check. List data tables keep the array marker, so a list `[1,null,3]` has keys 1 to 3 and encodes whole. Update the doc comment on `frozen_json_value` (lines 62-66, "absent or null reads as nil") to say a nested null reads `json.null` and only a top-level absent or null argv reads nil.
5. Public read behavior changes outside H1: `argv.a` for a JSON `{"a":null}` and `argv.ids[2]` for `[1,null,3]` now read `json.null` (truthy) instead of nil, and `pairs` and `#` see those slots. This is the intended change.

Acceptance: the three new or renamed tests pass, and these guards keep passing: `valid_json_scalars_make_argv_a_number_or_boolean_and_null_reads_as_nil` (`args_surface.rs` 68-84, top-level null stays nil), `frozen_nil_argv_reads_nil_and_rejects_assignment` (`tests/argv.rs` 100), `frozen_argv_encodes_its_stored_data` (`tests/json/views.rs` 29), `frozen_argv_holding_an_empty_list_encodes_it_as_an_empty_array` (`tests/argv.rs` 87). Commit: one commit holding this step's code and tests, in the repository's usual style per `AGENTS.md`.

</step-1>
<step-2>

### Step 2: Budget the encoder's node and byte work [completed]

- Component: Bounded conversion
- Tests:
  - `cargo nextest run --locked -p runvil-lua json`
  - `cargo nextest run --locked -p runvil-lua budgets`
  - `cargo nextest run --locked -p runvil-lua`
  - `cargo fmt --all --check`
  - `$env:CARGO_BUILD_WARNINGS="deny"; cargo clippy -p runvil-lua --all-targets --all-features`

Goal (D1-3): one `json.encode` or `json.encode_lines` call cannot expand shared tables or repeated strings past the Lua heap. Today `Encoder::table` (`crates/runvil-internal/lua/src/json/encode.rs` lines 197-228) tracks only the current path, so a table reachable twice is walked twice and every string occurrence is copied (line 180). There is no node or byte count, and the whole walk runs inside one Rust callback, where the instruction hook (`hardening.rs`, interval 10,000) cannot fire. `local t = {} for i = 1, 40 do t = { t, t } end return json.encode(t)` stays under the 127-level depth limit, has no cycle, and asks for about 2^41 tree nodes on the Rust heap, outside the 64 MiB Lua limit. Scope: modify files only inside `c:\Users\Vinnie\cursor\runvil`; do not edit `runvil-docs` and do not touch the other `from_value` sites.

Order rule: write the four tests first, run each at the current tree, and confirm it fails for the stated reason. A test that already passes proves its finding false: keep the test and skip that change.

1. New file `crates/runvil-internal/lua/src/tests/json/limits.rs`, registered with `mod limits;` beside the other `mod` lines in `crates/runvil-internal/lua/src/tests/json.rs`. Every test builds its VM with the small-heap pattern from `tests/budgets.rs` line 73, `TestVm::new().limits(4 * 1024 * 1024, DEFAULT_LUA_LOG_EVENTS).shims().build()`, so the current tree finishes quickly instead of exhausting the machine. Reuse `caught`, `lua_error`, and `encoded` from `tests/json.rs`. Define the doubling table with `vm.eval::<()>("t = {} for i = 1, 19 do t = { t, t } end")`.
   - Doubling table: `caught(&vm, "json.encode(t)")` equals `lua_error("json.encode: value is too large to encode")`. The walk visits about 1 million tables against a budget of 524,288 values (4 MiB ceiling divided by 8). At the current tree the call encodes them all and returns text, so `caught` fails with "required ... to raise".
   - Repeated string: `s = string.rep('x', 2^19)`, then `t = { s } for i = 1, 5 do t = { t, t } end`. About 16 MiB of copies against a 4 MiB byte budget raises the same message. At the current tree it returns text.
   - Debug mode raises too: `caught(&vm, "json.encode(t, { debug = true })")` on the doubling table equals the same error. At the current tree it returns text.
   - `caught(&vm, "json.encode_lines({ t })")` on the doubling table equals `lua_error("json.encode_lines: value is too large to encode")`. At the current tree it returns text.
2. New module `crates/runvil-internal/lua/src/walk_budget.rs`, registered as `mod walk_budget;` in `crates/runvil-internal/lua/src/lib.rs`, `pub(crate)` items only, under the 500-line file limit that the crate build script enforces.
   - `pub(crate) struct TooLarge;` as the refusal marker.
   - `WalkBudget { nodes, bytes }` with `charge_node(&mut self) -> Result<(), TooLarge>` and `charge_bytes(&mut self, n: usize) -> Result<(), TooLarge>`. Each fails once its counter is exhausted and never wraps.
   - `WalkBudget::for_vm(lua: &Lua) -> WalkBudget` sets nodes to ceiling/8 and bytes to the ceiling, where the ceiling is the VM's Lua heap limit. Reasoning: an unshared value that fits the heap has at most about ceiling/9 values (an array slot costs at least about 9 bytes) and at most ceiling bytes of string, so only sharing-driven expansion beyond the heap is refused. At the default 64 MiB that is 8,388,608 values, which also covers the most values an 8 MiB `json.decode` input (`INPUT_LIMIT` in `json/decode.rs` line 15) can produce, so decode-then-encode stays whole.
   - `pub(crate) struct HeapCeiling(pub usize);`. mlua has no getter for the memory limit (`Lua::set_memory_limit` only returns the previous value), so the ceiling lives in Lua app data. `for_vm` reads it and falls back to `DEFAULT_LUA_MEMORY_BYTES` when no app data exists, so no caller can run unbounded.
3. `crates/runvil-internal/lua/src/vm.rs`: store `HeapCeiling(DEFAULT_LUA_MEMORY_BYTES)` with `lua.set_app_data` next to `lua.set_memory_limit(DEFAULT_LUA_MEMORY_BYTES)` (line 259), and refresh it with the new `memory_bytes` in `apply_lua_limits` (lines 341-345). `TestVm::limits` already goes through `apply_lua_limits` (`test_vm.rs` lines 322-325), so the 4 MiB test VM gets a 4 MiB ceiling.
4. `crates/runvil-internal/lua/src/json/encode.rs`: `Encoder` gains `budget: WalkBudget`, built with `WalkBudget::for_vm`. Charge one node at the top of `value()` (scalars too), the string byte length for each string value, the key byte length for each object key, and one node per view hop in `resolve`. Add `too_large()`, which returns the `lua`-kind refusal `{function}: value is too large to encode`, using the same function prefix as the other errors (`json.encode` or `json.encode_lines`) and no path. It bypasses `fail()`, so debug mode raises it too: a placeholder per remaining value would not bound the work. Every `TooLarge` from a charge becomes this refusal and aborts the call. `encode_lines` already uses one `Encoder` for all items, so its budget is shared and its prefix is `json.encode_lines:`; keep it that way.
5. Same file, `resolve` (lines 331-366): replace the `Vec::contains` loop check over the view chain (lines 333-354, quadratic in chain length) with a `HashSet` of pointers. The `Loop` result and its behavior do not change.
6. Do not add `held`, `HELD_KEY_LIMIT`, or `check_value` in this step. Nothing uses them until Steps 3 and 4, and the denied clippy lints reject dead code.

Acceptance: the four new tests pass, and these guards keep passing: `a_table_that_contains_itself_raises_but_a_shared_table_encodes` (`tests/json/errors.rs` 208, a shared table in two places still encodes), `a_loop_of_127_tables_repeating_at_level_128_reports_the_cycle` (`errors.rs` 227), `a_var_value_127_levels_deep_encodes_because_views_add_no_level` (`tests/json/views.rs` 47), the view-loop tests in `views.rs`, and the `budgets.rs` tests. The budget constants are estimates: Host memory for a budget-sized encode can still be several times the Lua heap, which is accepted. Commit: one commit holding this step's code and tests, in the repository's usual style per `AGENTS.md`.

</step-2>

<step-3>

### Step 3: Cap held keys and read values lazily in the encoder [completed]

- Component: Bounded conversion
- Tests:
  - `cargo nextest run --locked -p runvil-lua json`
  - `cargo nextest run --locked -p runvil-lua`
  - `cargo fmt --all --check`
  - `$env:CARGO_BUILD_WARNINGS="deny"; cargo clippy -p runvil-lua --all-targets --all-features`

Goal (C-1): `json.encode` never panics inside mlua for a large table, and every list or object that fits the Lua heap still encodes. Today `entries` (`crates/runvil-internal/lua/src/json/keys.rs` lines 91-101) collects every `(key, value)` pair of a table at once. In mlua 0.12 each live string, table, function, thread, or userdata `Value` holds one slot on the auxiliary reference stack, and `ref_stack_pop` panics with "cannot create a Lua reference, out of auxiliary stack space" when `lua_checkstack` cannot grow it; Lua 5.5 caps a stack at 1,000,000 slots. The Engine has no `catch_unwind` outside tests, so that panic unwinds into the Host. The panic path is inferred from reading mlua, not executed; the first test below turns it into a checked fact. Scope: modify files only inside `c:\Users\Vinnie\cursor\runvil`; do not edit `runvil-docs`.

Order rule: write the two tests first, run each at the current tree, and confirm it fails for the stated reason. A test that already passes proves its finding false: keep the test and skip that change.

1. Add both tests to `crates/runvil-internal/lua/src/tests/json/limits.rs` (created in Step 2). They use the default heap, `TestVm::new().shims().build()` (the `json_vm()` helper in `tests/json.rs`), with `caught`, `lua_error`, and `encoded` from `tests/json.rs`.
   - 1,000,000-item list: `vm.eval::<()>("t = {} for i = 1, 1000000 do t[i] = 'a' end")`, then assert `encoded(&vm, "json.encode(t)").len()` equals 4,000,001. The budget Step 2 added allows 8,388,608 values at the default heap, so the current tree behaves like the endpoint here, and this should panic in mlua (C-1). If it passes, C-1 is false for lists: keep the test and skip the lazy-value change in item 3, but still do items 2, 4, and 5.
   - 150,000-key string object: `vm.eval::<()>("t = {} for i = 1, 150000 do t['k' .. i] = true end")`, then `caught(&vm, "json.encode(t)")` equals `lua_error("json.encode: value is too large to encode")` (above `HELD_KEY_LIMIT`). At the current tree it encodes, so `caught` fails with "required ... to raise".
2. `crates/runvil-internal/lua/src/walk_budget.rs`: add `pub(crate) const HELD_KEY_LIMIT: usize = 100_000;` with a doc comment. It is the most keys held as Lua references along the live encode path (string, table, function, thread, and userdata keys; integer, number, and boolean keys hold no slot, so integer-keyed lists are exempt). With the 1,000,000-slot stack cap, 10 times headroom covers the path's own live references.
3. `crates/runvil-internal/lua/src/json/keys.rs`: `entries` returns sorted keys only. It drops each pair's value immediately (a dropped value frees its reference slot) and counts the reference-holding keys. It refuses as soon as the encoder's `held` plus the running count would pass `HELD_KEY_LIMIT`, so the key vector never grows past the cap. `shape` takes the key slice instead of the pairs. `list`, `object`, and `fallback` read each value with `data.raw_get(key.clone())` when its turn comes. `debug_option` and `items` use the same `entries`. Key visiting order and error precedence do not change, because keys were already checked before values.
4. `crates/runvil-internal/lua/src/json/encode.rs`: `Encoder` gains `held: usize`. `Encoder::table` (lines 197-228) adds the table's reference-holding key count to `held` before it encodes the children and subtracts it after the table encodes, on every return path including debug-mode placeholder returns, so a finished table's keys never count against its siblings. When the limit would be passed, `too_large()` (added in Step 2) raises `{function}: value is too large to encode`, debug mode included. The same message covers the node budget, the byte budget, and `HELD_KEY_LIMIT`.
5. Reversible choice, stated for the reviewer: one live path holding more than 100,000 string-like keys is refused. Confidence medium: the headroom comes from reading mlua and Lua source, not from a run.

Acceptance: both new tests pass, the Step 2 tests in `limits.rs` still pass, and these guards keep passing: `a_table_that_contains_itself_raises_but_a_shared_table_encodes` (`tests/json/errors.rs` 208), `a_loop_of_127_tables_repeating_at_level_128_reports_the_cycle` (`errors.rs` 227), `a_var_value_127_levels_deep_encodes_because_views_add_no_level` (`tests/json/views.rs` 47), and the view-loop tests in `views.rs`. Commit: one commit holding this step's code and tests, in the repository's usual style per `AGENTS.md`.

</step-3>
<step-4>

### Step 4: Refuse oversized var writes and run the exit checks [completed]

- Component: Bounded conversion
- Tests:
  - `cargo nextest run --locked -p runvil-lua json`
  - `cargo nextest run --locked -p runvil-lua argv`
  - `cargo nextest run --locked -p runvil-lua sys_and_var`
  - `cargo nextest run --locked -p runvil-lua`
  - `cargo nextest run --locked -p runvil-engine`
  - `cargo nextest run --locked -p runvil-parser`
  - `cargo nextest run --locked -p runvil`
  - `cargo nextest run --locked -p harness-runner`
  - `cargo nextest run --locked -p harness`
  - `$env:CARGO_BUILD_WARNINGS="deny"; cargo clippy -p runvil-lua -p runvil-engine --all-targets --all-features`
  - `cargo fmt --all --check`
  - `cargo test -p build-xtask`
  - `$env:RUSTDOCFLAGS="-D warnings"; cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`
  - `$env:RUSTDOCFLAGS="-D warnings"; cargo doc -p runvil --no-deps`
  - `Remove-Item Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue; cargo +nightly-2026-09-05 xtask api --check` (the pinned nightly named in `crates/build-xtask/src/api/toolchain.rs`; it must report the facade surface unchanged, with no edit to `crates/runvil/public-api.txt`)

Goal (D1-4): a `var` write cannot convert a shared-table value once per occurrence on the Rust heap before any Lua allocation. In `crates/runvil-internal/lua/src/sys.rs`, line 92, `lua.from_value::<Json>(value)` in the `guarded_table` `__newindex` closure does that, so `var.x = t` with a doubling table has the same process-level failure as `json.encode`. Only `sys.rs` is fixed here. `collection.rs` line 138 is a test helper and line 120 is not an independent exposure, and the other `from_value` sites are deferred, so leave them alone. Scope: modify files only inside `c:\Users\Vinnie\cursor\runvil`; do not edit `runvil-docs` and do not run D1-5 or any other deferred item.

Order rule: write the test first, run it at the current tree, and confirm it fails for the stated reason. A test that already passes proves its finding false: keep the test and skip that change.

1. `crates/runvil-internal/lua/src/tests/sys_and_var.rs`, new test `a_var_write_of_a_shared_table_past_the_budget_is_refused`. Build the VM with the small-heap pattern, `TestVm::new().limits(4 * 1024 * 1024, DEFAULT_LUA_LOG_EVENTS).shims().build()`, then define the doubling table with `vm.eval::<()>("t = {} for i = 1, 19 do t = { t, t } end")`. Evaluate `local ok, err = pcall(function() var.x = t end) if ok then return false, '', '' end return true, tostring(err.kind), tostring(err)` as a `(bool, String, String)` (the same pcall shape as `caught` in `tests/json.rs`, which is private to that module, so write the call inline) and assert the write raised, with kind `lua` and message `var.x is too large to store`. The walk visits about 1 million tables against a budget of 524,288 values. At the current tree the conversion builds the tree and then fails differently (a Lua memory error from rebuilding it), so the message assertion fails.
2. `crates/runvil-internal/lua/src/walk_budget.rs`: add `WalkBudget::check_value(&mut self, value: &Value) -> Result<(), TooLarge>`, a pre-walk for `from_value` callers. It is recursive and lazy over `Table::pairs` (it never collects them), counts every value visited including repeats of shared tables, adds string and key bytes, keeps a path vector of table pointers, and stops descending into a table already on the path or past 200 levels, leaving those cases to `from_value` exactly as today (cyclic and very deep values keep mlua's own behavior).
3. `crates/runvil-internal/lua/src/sys.rs`: in the `guarded_table` `__newindex` closure, after the existing function, userdata, and thread check and before `lua.from_value::<Json>(value)`, run `WalkBudget::for_vm(lua).check_value(&value)`. On `TooLarge` return `mlua::Error::runtime(format!("{target} is too large to store"))`, where `target` is the same label the closure's other messages use (`var.x` for key `x`). mlua's own messages for every other failure stay unchanged. Consequence for the reviewer: a cyclic value with fan-out can report the too-large message instead of mlua's recursion message, which is acceptable and rare.
4. Run every command under Tests as written. The exit checks are the Testing Plan's: focused `runvil-lua` runs for `json`, `argv`, and `sys_and_var`, the full `runvil-lua`, `runvil-engine`, `runvil-parser`, `runvil`, `harness-runner`, and `harness` suites, clippy with warnings denied, `cargo fmt --all --check`, `cargo test -p build-xtask` (it enforces the 500-line file limit, so check `walk_budget.rs`, `encode.rs`, `keys.rs`, `vm.rs`, and `sys.rs`), the rustdoc commands with warnings denied, and `cargo xtask api --check`. Fix any failure inside this step's commit.

Acceptance: the new `var` test passes, every earlier new test passes, and these guards keep passing: the existing `var` cases in `sys_and_var.rs` (a value 127 levels deep and a shared small value still write), `a_table_that_contains_itself_raises_but_a_shared_table_encodes` (`tests/json/errors.rs` 208), `a_loop_of_127_tables_repeating_at_level_128_reports_the_cycle` (`errors.rs` 227), `a_var_value_127_levels_deep_encodes_because_views_add_no_level` (`tests/json/views.rs` 47), and the view-loop tests in `views.rs`. All exit checks pass. Commit: one commit holding this step's code and tests, in the repository's usual style per `AGENTS.md`.

</step-4>
</execution-plan>