---
name: One offer API debt removal
overview: "Remove the two debts the one-offer-API run (runvil 890b41faa..8c0d7e4bf) introduced: a script-sized allocation in the offer decoder that can panic or abort the Host, and call counts that re-seed from a live adhoc expansion after the render. Also make tools.adhoc register all or nothing, give a bypassed adhoc catalog tool a clear offer error, make the gateway config UI tests always build their bundle first, and correct their stale notes."
todos:
  - id: offer-bound
    content: "D1-1: refuse an offer array whose #t exceeds HELD_KEY_LIMIT before allocating, with fail-first tests in a new tools/tests-offer-limits.rs"
    status: pending
  - id: rendered-seeding
    content: "D1-2: once a section is rendered, seed tools.calls from the rendered list's ids; counts take ids; fix the false doc invariant; engine regression test"
    status: pending
  - id: adhoc-atomic
    content: "D1-10: tools.adhoc writes the handler and tool object first and registers last, clearing both if registering fails; fault-injection test"
    status: pending
  - id: adhoc-object-offer
    content: "D1-7: offering a tool object under the adhoc Pack that no adhoc tool holds raises the unknown-tool error at the offer line instead of an internal error at render"
    status: pending
  - id: config-ui-pretest
    content: "F-1: add a pretest build script to crates/gateway/config-ui/ui/package.json; prove npm test passes with dist/ deleted"
    status: pending
  - id: config-ui-notes
    content: "F-2: correct the three stale 'debug cargo build' notes in the config UI tests"
    status: pending
  - id: docs-offer-limit
    content: "D1-1 docs: one runvil-docs commit adding the new refusal to 12-tools.md and a limits row to 17-quick-reference.md"
    status: pending
  - id: verify
    content: Run the focused, workspace, clippy, fmt, API, docs-claims, config UI, and runvil-docs checks in the Testing Plan
    status: pending
isProject: false
---

# One offer API debt removal

<product-contract>

## Product Requirements

- Scope and target work:
  - Repository `c:\Users\Vinnie\cursor\runvil`, branch `master`. Baseline `890b41faa` ("Trim the public API of the Engine"), which is `upstream/master` (`git@github.com:cppalliance/runvil.git`) and the merge base with HEAD. Endpoint and disposition `8c0d7e4bf` (HEAD, clean worktree, not pushed).
  - Target commits, all under plan `vibe/2026-10-23-1-one-offer-api.md` except the lockfile commit: `e0e43803a` "Rename local tools to adhoc", `96272d510` "Update tokio and ctutils in lockfile", `f08778d2f` "Add tool parameters, vendors, listing trust; reserve adhoc", `b8bd93528` "Collapse tool offering into tools.offer with adhoc tools", `8c0d7e4bf` "Close plan: one-offer-api".
  - Operator-expanded scope: two pre-existing items in `crates/gateway/config-ui/ui` (F-1, F-2), found while verifying that run, and two candidates the analysis rejected, D1-10 and D1-7, which the operator chose to fix.
  - Docs: `c:\Users\Vinnie\cursor\runvil-docs`, branch `master`, HEAD `24e8143` ("Document the one offer API and bring the guide up to date"). It gains one commit for the new refusal D1-1 adds.
- Cleanup goals:
  - Section Lua can no longer turn a `tools.offer` or `tools.offer_dynamic` call into a Rust panic or a process abort (D1-1).
  - `tools.calls` follows its stated contract after the render: a tool outside the rendered list gets no count until its first call (D1-2).
  - `tools.adhoc` either defines a tool completely or leaves no trace of it (D1-10).
  - A run that ignored the `adhoc` refusal never reaches an internal error by offering a catalog `adhoc/*` tool object; the offer line raises the ordinary unknown-tool error (D1-7).
  - The config UI tests never run against a stale bundle, and their notes say how the bundle is built (F-1, F-2).
- Non-goals: no change to the offer forms, the render order, routing, vendors, listings, or any persisted or wire format; no new public Rust API; no change to the pre-render seeding pattern (D1-19); no streaming offer decode.
- Success criteria:
  - every regression test in the Testing Plan fails at `8c0d7e4bf` and passes after its fix;
  - `cargo nextest run --locked --workspace` (with the Workshop crates run separately, as the project does), both clippy runs, `cargo fmt --all --check`, `xtask api --check`, and the docs-claims scan pass;
  - `npm test` in `crates/gateway/config-ui/ui` passes with `dist/` deleted;
  - the runvil-docs checks in the Testing Plan pass.

## Functional Specification

### Debt Inventory

- Debt added:
  - **D1-1 (introduced by `b8bd93528`): the offer decoder sizes a buffer from a script-chosen `__len` and holds one Lua reference per entry.**
    - Evidence: `crates/runvil-internal/lua/src/tools/offer.rs`, `array` (lines 180-209 at `8c0d7e4bf`): `let count = usize::try_from(table.len()?).unwrap_or(0); let mut entries = Vec::with_capacity(count);` runs before any entry is read, and every entry is pushed as a live `mlua::Value` until `targets` converts them (lines 93-98). `Table::len` honors `__len` (mlua 0.12.0). Section Lua can set `__len`: `tools/tests-offering.rs` already does.
    - Impact: `__len` returning `math.maxinteger` makes `Vec::with_capacity` panic ("capacity overflow"). `__len` returning `1 << 40` requests at least 16 TiB and aborts the process on Windows and under Linux default overcommit. A plain array of about 1,000,000 or more strings or tool objects exhausts mlua's reference stack and panics ("cannot create a Lua reference, out of auxiliary stack space"); `lua/src/tests/json/limits.rs` already proves that panic for `json.encode` at 1,200,000 entries. In the Workshop, a panic ends one conversation's run (each runs in its own tokio task, `workshop/server/src/agents.rs` line 308); the abort kills the server and every conversation.
    - Recurrence: the same cause as the 2026-10-19 JSON plan's C-1, fixed by `6c3d44718` ("Limit held keys and read encoder values lazily"), which introduced `crate::walk_budget::HELD_KEY_LIMIT`.
    - Reversal cost: cheap. `array` and `targets` are private; the fix adds one refusal message to the documented error list.
    - Target state: an array whose `#t` is greater than `HELD_KEY_LIMIT` (100,000) is refused with `<verb> takes an array of at most 100000 entries, got <n>` before anything is allocated or read, where `<verb>` is `tools.offer` or `tools.offer_dynamic` and `<n>` is the `#t` value as the script reported it.
  - **D1-2 (introduced by `b8bd93528`): after the render, counts re-seed from the live adhoc Pack expansion.**
    - Evidence: `section_call_counts` (`crates/runvil-internal/engine/src/execute/scheduler/tool_call.rs` lines 189-196) calls `current_tool_bindings(set, runtime, adhoc)` on every catalog and adhoc dispatch, and `script_call_counts` (`crates/runvil-internal/engine/src/execute/section_context.rs` lines 201-222) calls `counts.ensure` for every binding returned. `expanded` (`crates/runvil-internal/lua/src/vm/state.rs`) maps an offered adhoc Pack to `adhoc.bindings()`, the live registry, and `tools.adhoc` works after the render.
    - Impact: in a section that offered `tools.packs.get('adhoc')` before its first loop round, an adhoc tool defined after that round is not in the rendered list (the model cannot call it), yet after any later tool dispatch `tools.calls['adhoc/late']` reads `0`. The plan's "Calls and counts" contract says it raises `tools.calls: ... has no seeded count` with the adhoc hint. The doc comment on `current_tool_bindings` ("the bindings never change once the list is rendered") is false in the same case. Routing is unaffected: model calls route through the rendered list.
    - Related: D1-19 (pre-render seeding at each dispatch) has the same mechanism, predates the target, and stays as it is.
    - Reversal cost: cheap; internal to `runvil-lua` and `runvil-engine`.
    - Target state: once the section's list is rendered, counts seed only from the rendered list's ids.
- Cheap fixes: none from the analysis.
- Operator-expanded pre-existing items (not debt added by the target):
  - **F-1: the config UI tests can run against a stale bundle.** `crates/gateway/config-ui/ui/package.json` defines `build` (`node build.mjs`) and `test` (`node --test "src/**/*.test.mjs"`) but no `pretest`. The tests import the gitignored `dist/app.js` and `dist/app.css`. During the run's final verification a `dist/` built on 10/16, before `ceae9ad0f` (10/21, "Move the gateway nav back into the header"), failed two `tab-bar.test.mjs` tests; a rebuild passed 211 of 211.
  - **F-2: three notes say a debug `cargo build` produces `dist/`.** `crates/gateway/config-ui/build.rs` writes the bundle to `$OUT_DIR/ui-dist/` "(never into the repository)". The stale notes: `ui/src/test-support.mjs` line 19, `ui/src/styles/tokens.test.mjs` lines 6-7, `ui/src/main.test.mjs` lines 5-6. `ui/build.mjs` lines 1-5 and `src/assets.rs` line 36 describe the crate's own bundle correctly and stay.
- Operator-promoted rejected candidates (introduced by `b8bd93528`; rejected by the analysis, fixed at the operator's choice):
  - **D1-10: `tools.adhoc` registers in three stores without atomicity.** `install_adhoc` (`crates/runvil-internal/lua/src/tools.rs` lines 264-270) calls `adhoc.register(&name, &info)` (the Rust `AdhocTools` push), then `handlers.raw_set(name, handler)`, then `insert_tool_object(lua, &binding)`. The last two allocate in the Lua heap. If either fails after `register` succeeded, for example from a memory error an author's `pcall` survives, the tool has a binding but no handler or object. A redefinition then raises "already registered", and a call to it reaches `Error::Internal("an adhoc tool answer names a registered handler")`. Target state: the handler and the tool object are written first, `register` runs last, and a failed `register` clears both, so a failed `tools.adhoc` leaves no trace.
  - **D1-7: a bypassed `adhoc/*` catalog tool reaches an internal error.** `RunContext::prepare` refuses a catalog or roster naming the Pack id `adhoc` through `Requirements::reserved_adhoc`, but the refusal is advisory, and `runvil::Run::new` accepts an unprepared context. Then `install_tool_objects` (`crates/runvil-internal/lua/src/tools/objects.rs` lines 40-52) gives the catalog tool `adhoc/x` an object. `Offering::target` (`crates/runvil-internal/lua/src/tools/offer.rs` lines 105-125) records any `LuaToolHandle` unchecked, and the render fails in `expanded` (`crates/runvil-internal/lua/src/vm/state.rs` lines 178-187) with `Error::Internal("offered tool \"adhoc/x\" has no binding")`. Offering the same id as a string already raises `tools.offer: "adhoc/x" is not a tool in this run` (`offer.rs` `named`, lines 130-144). Target state: the object path checks an `adhoc/*` id against `AdhocTools` the way the string path does, so the offer line raises that same unknown-tool error and the render never sees the id.
- Exposed pre-existing debt: none.
- Rejected candidates (21 remain; the analysis kept 2 of 21, the challenger rejected 4 more of its own, and the operator promoted 2):
  - residual-but-acceptable (4): the reserved dynamic-offer scaffold (D1-3); a listing kept outside an adhoc call, which the spec states (D1-4); older records lacking `vendor`, which the spec states (D1-5); pre-render model calls, reachable only through a test hook (D1-6).
  - weak/speculative (7): no `runvil` vendor reservation (D1-8); `UsablePack` vendor as a bare string (D1-9); the `(ToolSet, ToolRuntime, AdhocTools)` parameter cluster (D1-11); `AskTool` mirroring `Advertised` (D1-12); the `adhoc` id defined in four crates (D1-13); `insert_tool_object`'s install-order dependency (C-2); `AdhocTools::binding` taking an id or a wire name (C-3). None showed a consequence.
  - false (7): stale `Chain.advertised` misrouting (D1-14); a mismatched `MessageList` (D1-15); wrong-call listing marks (D1-16); wrapper schema drift (D1-17); unchecked tool objects (D1-18); case-colliding adhoc ids (C-1); other `__len`-sized buffers (C-4).
  - unrelated pre-existing (3): pre-render count seeding (D1-19); Pack objects for unavailable declared Packs (D1-20); the rename commit's labels on unchanged mechanisms (D1-21).

</product-contract>
<implementation-contract>

## Technical Design

- **D1-1, the offer array bound** (`crates/runvil-internal/lua/src/tools/offer.rs`, 214 lines):
  - In `array`, read `let len = table.len()?;` first. If `len` is greater than `HELD_KEY_LIMIT` (import `crate::walk_budget::HELD_KEY_LIMIT`, which is `pub(crate)` and 100,000), return `mlua::Error::external(format!("{verb} takes an array of at most {HELD_KEY_LIMIT} entries, got {len}"))` before `Vec::with_capacity` and before any `get`. Keep the existing behavior for every `len` up to the limit, including a negative `len` reading as an empty array through `usize::try_from(..).unwrap_or(0)`.
  - Keep the per-entry checks, the raw-key check, and the rule that every entry is checked before any is recorded. A refusal records nothing. The render refusal still comes first, because `install_offer` checks it before decoding.
  - Update `array`'s doc comment to state the bound.
  - The new message is part of the documented error contract, so `runvil-docs` gains it (see Docs below).
- **D1-2, seeding from the rendered list:**
  - `crates/runvil-internal/lua/src/messages.rs` (454 lines): add `pub(crate) fn rendered_ids(&self) -> Option<Vec<ToolId>>`, next to `rendered_target` (lines 223-226). It returns the ids in the rendered list's `dispatch` values, or `None` before the render. It does not include `RenderedTools.dynamic`.
  - `crates/runvil-internal/lua/src/vm/state.rs` (221 lines): add `pub fn rendered_ids(&self) -> Option<Vec<ToolId>>` on `SectionVm`, delegating to `self.messages`. Change `install_tool_call_counts(&self, bindings: &[ToolBinding])` (line 83) to take `ids: &[ToolId]`. Rewrite the `current_tool_bindings` doc comment (lines 102-111) so it no longer claims the bindings never change once the list is rendered. It should say an offered adhoc Pack expands to the adhoc tools defined when the function runs, and that after the render the Engine reads the rendered list instead.
  - `crates/runvil-internal/lua/src/tools.rs` (343 lines): `install_tool_call_counts(lua, bound_tools, adhoc, ids: &[ToolId])` seeds `ToolCallCounts::new(ids.iter().map(ToString::to_string))`. The unseeded-key hints, ` (a catalog tool that was neither offered in this section nor called with tools.call)` and ` (an adhoc tool that was neither offered in this section nor called with tools.call)` (lines 127-130), are unchanged.
  - `crates/runvil-internal/engine/src/execute/section_context.rs` (294 lines): `script_call_counts(&mut self, ctx, ids: &[ToolId])` and `install_section_scope(.., ids: &[ToolId])` take ids instead of bindings. Their doc comments say the seeded set is the section's live scope before its first render and its rendered list after.
  - `crates/runvil-internal/engine/src/execute/scheduler/tool_call.rs` (358 lines): `section_call_counts` uses `vm.rendered_ids()` when it is `Some`. Otherwise it maps `current_tool_bindings(..)` to ids, as today. Both callers (`prepare_catalog_call`, line 312, and `prepare_adhoc_call` in `tool_call-adhoc.rs`, line 32) are unchanged.
  - Test callers of `install_tool_call_counts` pass ids: `crates/runvil-internal/lua/src/tools/tests.rs` line 230 and `crates/runvil-internal/lua/src/tools/tests-adhoc.rs` line 267.
  - No public API change: these are internal crates, and `runvil/public-api.txt` must not change.
- **D1-10, all-or-nothing `tools.adhoc`:**
  - `crates/runvil-internal/lua/src/vm/adhoc.rs` (71 lines): `AdhocTools::register` takes the prebuilt `ToolBinding` (`pub(crate) fn register(&self, binding: ToolBinding) -> Result<()>`) instead of building it from `name` and `info`. That way the tool object and the registry hold the same binding.
  - `crates/runvil-internal/lua/src/tools.rs` `install_adhoc` (lines 264-270), after `adhoc_info`:
    - `let binding = ToolBinding::from_info(&name, &info);`
    - then `handlers.raw_set(name.as_str(), handler)?;`
    - then `let object = insert_tool_object(lua, &binding)?;`
    - then `adhoc.register(binding)`. If it fails, set the handler entry and the tool-object entry for the id back to `nil`, and return the error.
    - return `object`.
  - A failure in the handler write or the object insert leaves `AdhocTools` untouched. A handler entry left behind by a failed object insert is unreachable, because dispatch and the collision checks read only `AdhocTools`, and a redefinition overwrites it.
  - Update `install_adhoc`'s doc comment to say the definition is all or nothing.
- **D1-7, the object path checks adhoc ids:**
  - `crates/runvil-internal/lua/src/tools/offer.rs` `Offering::target` (lines 105-125): in the `LuaToolHandle` arm, when the handle's id is under the `adhoc` Pack (`is_adhoc(&id.pack())`) and `self.adhoc.binding(&id.to_string())` is `None`, return `mlua::Error::external(format!("{verb}: {name:?} is not a tool in this run"))`. That is the string path's message, with `name` the id text. Every other handle is recorded as before.
  - The internal error in `expanded` stays as the invariant check it now is; no offer can reach it.
  - No new message, so no docs change. The `adhoc` reservation in the registry, the parser, and `RunContext::prepare` is unchanged.
- **F-1:** add `"pretest": "npm run build"` to the `scripts` in `crates/gateway/config-ui/ui/package.json`, so `npm test` always tests a fresh `dist/`. CI (`.github/workflows/ci.yml` lines 311-317) keeps its separate "Build config UI" step, so a build failure is reported under its own name.
- **F-2:** reword the three notes to say `npm test` builds `dist/` first through the `pretest` script:
  - `crates/gateway/config-ui/ui/src/test-support.mjs` line 19 becomes ``/** Imports the built bundle once; `npm test` builds it first through the `pretest` script. */``.
  - `crates/gateway/config-ui/ui/src/styles/tokens.test.mjs` lines 6-7 and `crates/gateway/config-ui/ui/src/main.test.mjs` lines 5-6: replace "Run after `npm run build` (a debug `cargo build` also produces dist/)." with "Run with `npm test`, whose `pretest` script builds dist/ first."
- **Docs (`runvil-docs`, one commit on `master`, not pushed):**
  - `src/language/12-tools.md`, in "Errors from `tools.offer`" (heading at line 901), after the array-key bullet (line 964), add: ``- An array longer than 100,000 entries raises `tools.offer takes an array of at most 100000 entries, got {n}` before any entry is read, so an array whose `#t` is 100,001 raises `tools.offer takes an array of at most 100000 entries, got 100001`.``
  - `src/language/17-quick-reference.md`, in the limits table beside the `json.encode` key limit row (line 263), add: ``| `tools.offer` array length | 100,000 entries per `tools.offer` or `tools.offer_dynamic` call | fixed | [Errors from `tools.offer`](12-tools.md#errors-from-toolsoffer) |``.
  - No em dashes or double dashes.
- Constraints:
  - Every Rust file under `src` or `tests` stays at or under 500 lines (`build-ceiling`).
  - Under `crates/runvil` and `crates/runvil-internal`, every line passes the docs-claims scan: no "host", "workshop", or "papergate" in any case, no lowercase "harness" except "harness = false" and "test harness", no "Harness-", and no "RunHarness". This covers comments and test strings.
  - No comment or doc says what changed or what something used to be.

</implementation-contract>
<verification-contract>

## Testing Plan

- **D1-1 regression tests,** in a new `crates/runvil-internal/lua/src/tools/tests-offer-limits.rs`, wired the way `tests-offering.rs` is and using the same test VM helper over a set that offers `web`:
  - Fail first: evaluating `tools.offer(setmetatable({}, { __len = function() return math.maxinteger end }))` returns an error whose text contains `tools.offer takes an array of at most 100000 entries, got 9223372036854775807`, and the same VM then evaluates `tools.offer('web')` without error. At `8c0d7e4bf`, `Vec::with_capacity` panics with "capacity overflow" before any assertion runs.
  - Fail first: `local t = {} for i = 1, 1200000 do t[i] = 'web' end tools.offer(t)` returns an error containing `got 1200000`. At `8c0d7e4bf`, holding 1,200,000 references panics with "cannot create a Lua reference, out of auxiliary stack space". This is the size `lua/src/tests/json/limits.rs` uses.
  - Boundary: an array of exactly 100,000 `'web'` entries is accepted and offers `web` once, and `tools.offer_dynamic` with 100,001 entries raises `tools.offer_dynamic takes an array of at most 100000 entries, got 100001`.
  - Only after the bound lands: `__len` returning `1 << 40` raises the bound error with `got 1099511627776`. Never run this case against `8c0d7e4bf`: there it aborts the test binary, which neither `#[should_panic]` nor `catch_unwind` can observe.
  - The existing offer tests in `tools/tests-offering.rs` still pass. They cover the array forms, gaps, stray keys, nested tables, `__len` proxies, and `tools.offer(tools.packs)`.
- **D1-2 regression test,** in `crates/runvil-internal/engine/src/execute/tests/debug_and_counts.rs` (282 lines). It drives one `models.loop` round with a scripted model reply that makes no tool call, the way `engine/src/execute/tests/chat_scope.rs` drives a rendered round. Section Lua, in order:
  - `local grab = tools.adhoc({ name = 'grab', description = 'Grab' }, function() return 'got' end)`, then `tools.offer(tools.packs.get('adhoc'))`;
  - one loop round, which renders the list;
  - `tools.adhoc({ name = 'late', description = 'Late' }, function() return 'late' end)`, then `tools.call(grab, {})`;
  - `local ok, err = pcall(function() return tools.calls['adhoc/late'] end)`;
  - assert `not ok`, and that `tostring(err)` contains both `has no seeded count` and `(an adhoc tool that was neither offered in this section nor called with tools.call)`;
  - assert `tools.calls['adhoc/grab'] == 1`.
  - At `8c0d7e4bf`, `section_call_counts` re-expands the adhoc Pack to include `late`, so `tools.calls['adhoc/late']` reads `0`, `pcall` succeeds, and `assert(not ok)` fails.
  - The existing counts tests still pass: `tool_call_counts_seed_by_id_and_explain_an_unseeded_key` (`lua/src/tools/tests.rs` lines 219-280) and the adhoc-hint assertion in `debug_and_counts.rs` (line 268).
- **D1-10 fault-injection test,** in `crates/runvil-internal/lua/src/tools/tests-adhoc.rs` (272 lines), on the test VM's Lua state:
  - save the table stored under the named registry value `runvil.tools.objects` (`TOOL_OBJECTS_REGISTRY` in `tools/objects.rs`; make it `pub(super)` or `pub(crate)` for the test), then set that value to `false`, so `insert_tool_object` fails;
  - `pcall(tools.adhoc, { name = 'x', description = 'y' }, function() return 'z' end)` returns false;
  - restore the saved table, then assert the VM's `AdhocTools::binding("adhoc/x")` is `None`, and that `tools.adhoc({ name = 'x', description = 'y' }, function() return 'z' end)` now succeeds and returns an object equal to `tools.get('adhoc/x')`.
  - At `8c0d7e4bf`, `register` ran before the failed insert, so the binding is `Some` and the second definition raises `tools.adhoc name "x" is already registered`. The `None` assertion fails.
- **D1-7 regression test,** in `crates/runvil-internal/lua/src/tools/tests-offering.rs` (318 lines). Build the test VM over a `ToolSet` whose offering holds a catalog tool `adhoc/x`, as a run that ignored the refusal would. Then:
  - `local ok, err = pcall(tools.offer, tools.get('adhoc/x'))`;
  - assert `not ok` and that `tostring(err)` contains `tools.offer: "adhoc/x" is not a tool in this run`;
  - assert `current_tool_bindings` for the VM returns `Ok` with no `adhoc/x` binding.
  - At `8c0d7e4bf`, the offer succeeds, so `assert(not ok)` fails, and the render then returns `Error::Internal("offered tool \"adhoc/x\" has no binding")`.
  - An adhoc tool the section defined, offered by object, is still recorded; the existing adhoc offer tests cover it.
- **F-1 check,** in PowerShell from `c:\Users\Vinnie\cursor\runvil\crates\gateway\config-ui\ui`: `Remove-Item -Recurse -Force dist -ErrorAction SilentlyContinue; npm test`. At `8c0d7e4bf` this fails, because the tests cannot import the missing `dist/app.js`. After F-1 it builds `dist/` and passes all 211 tests. `npm run typecheck` also passes.
- **F-2 check:** `rg -n "debug \x60cargo build\x60" crates/gateway/config-ui` (from the `runvil` root) prints nothing.
- **Exit checks,** run in PowerShell from `c:\Users\Vinnie\cursor\runvil`; each must exit 0:
  - `cargo nextest run --locked -p runvil-lua -p runvil-engine -p runvil-types --all-features`
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`
  - `cargo fmt --all --check`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`
  - `Remove-Item Env:CARGO_BUILD_WARNINGS, Env:RUSTDOCFLAGS -ErrorAction SilentlyContinue; cargo +nightly-2026-09-05 xtask api --check`, which must report no change
  - `node --test crates/workshop/ui/test/docs-claims.mjs`
- **runvil-docs checks:**
  - `Get-ChildItem c:\Users\Vinnie\cursor\runvil-docs\src -Recurse -Filter *.md | Select-String -Pattern ([string][char]0x2014)` prints nothing;
  - `git -C c:\Users\Vinnie\cursor\runvil-docs grep -n "takes an array of at most 100000 entries" -- src` prints exactly the two new lines, and the message text matches the format string in `offer.rs`;
  - the site build exits 0: `Set-Location c:\Users\Vinnie\cursor\runvil; $env:RUNVIL_DOCS='c:\Users\Vinnie\cursor\runvil-docs'; cargo xtask site --books-only`.

</verification-contract>
<decision-record>

## Decision Record

- User-resolved architecture choices:
  - **D1-1 is fixed by refusing an array longer than `HELD_KEY_LIMIT` (100,000) before anything is allocated.** User: "Refuse arrays longer than HELD_KEY_LIMIT (100,000) before allocating anything." It reuses the limit `json.encode` already applies, one check covers all three crash paths, and raising or removing the limit later breaks no prompt that works under it.
  - **The two config UI follow-ups are in scope.** User: "sweep up those two optional follow-ups into the resulting plan."
  - **Two rejected candidates are fixed: D1-10 and D1-7.** The operator chose them from the rejected list when asked which rejected debts were worth fixing. D1-9 (validating the `UsablePack` vendor), D1-19 (count seeding before the render), and the rest stay rejected.
- Reversible decisions and consequences:
  - D1-1 message text `<verb> takes an array of at most 100000 entries, got <n>`: it follows the other `<verb> takes ...` refusals, prints the limit from the constant, and prints the script's `#t` so a hostile `__len` is visible.
  - D1-1 tests go in a new `tests-offer-limits.rs` rather than `tests-offering.rs` (318 lines), which keeps both well under 500 lines and the limit cases together.
  - D1-2 seeds from the rendered list's ids once rendered. It is one branch in `section_call_counts`, makes the contract and the doc true, and leaves the pre-render pattern (D1-19) unchanged. Counts take ids rather than bindings, because ids are all they read.
  - D1-10 writes the Lua-side entries before the Rust registry, because only the Lua writes can plausibly fail (they allocate in the Lua heap). `register` fails only on a poisoned lock, and it clears both Lua entries when it does. `register` takes the prebuilt binding, so the object and the registry cannot differ.
  - D1-7 raises the string path's existing `<verb>: "<id>" is not a tool in this run` at the offer line instead of a new message. The two paths then agree, no docs change, and the advisory `adhoc` reservation keeps its report-only design.
  - F-1 keeps CI's separate "Build config UI" step, so the build runs twice in that job (about a second) in exchange for a clearly named build failure.
  - The design record `vibe/2026-10-23-1-one-offer-api.md` is not amended, because it is history. The new refusal is documented in `runvil-docs`.
- Rejected alternatives:
  - D1-1 bound at the number of distinct tools and Packs the section can name: it is data-dependent, and it refuses arrays that are valid today, such as a repeated `'web'`.
  - D1-1 streaming decode with no bound: a hostile `__len` with an `__index` would loop about 9.2e18 times under the effectively unlimited `HOOK_BUDGET` (`lua/src/hardening.rs` line 21).
  - D1-1 collecting names first and validating later: it brings back the baseline's repeated-string copy exposure, which the endpoint's per-entry validation in `named` removed.
  - D1-2 snapshotting the adhoc Pack expansion inside the Lua crate at render time: it needs render state in `ToolRuntime` and duplicates what the rendered list already holds.
  - D1-2 rewriting the "Calls and counts" contract to allow live seeding: it changes a user-approved contract to match a defect.
  - F-1 removing CI's separate build step: it saves about a second but loses the named build failure.
  - D1-10 keeping `register` first and undoing it when a Lua write fails: it needs an `AdhocTools` removal method for a failure the reorder avoids outright.
  - D1-7 skipping catalog `adhoc/*` tools in `install_tool_objects`: the embedder's tool would vanish from Lua silently instead of raising a clear error.
  - D1-7 making `Run::new` enforce `reserved_adhoc`: it turns one advisory requirement into a mandatory one and changes the public run-start behavior.
- Assumptions and risks:
  - D1-1's panic and abort paths were established by reading `offer.rs`, the mlua 0.12.0 source, and the repository's JSON reference-stack test, not by running them. The fail-first tests confirm the panics. The abort case is never run before the fix.
  - The 1,200,000-entry test table fits in the 64 MiB Lua heap, at about 16 bytes per array slot.

### Deferred and Out of Scope

- D1-19, pre-render seeding at each dispatch: pre-existing and unchanged. Revisit if `tools.calls` must be stable before the first call.
- D1-20, Pack objects for declared Packs the roster marks unavailable: pre-existing. Revisit if a run with an unavailable declared Pack can reach section Lua.
- D1-7's script-call path: with a bypassed refusal, `tools.call('adhoc/x')` still runs the catalog tool, because `tool_route` finds no adhoc binding and routes it as a catalog call. That is not an internal error. Revisit if `RunContext::prepare`'s refusal becomes mandatory.
- D1-9, validating the `UsablePack` vendor as one segment: no consequence today, and the operator left it. Revisit before a release if the public constructor's signature should be fixed for good.
- Streaming the offer decode to one live reference at a time: not needed once the bound holds. Revisit if `HELD_KEY_LIMIT` is raised toward the reference-stack cap.
- Reserving the `runvil` vendor (D1-8): no spoofing path today. Revisit if the Workshop loads third-party plugins.
- The `runvil-docs` commit `24e8143` was not analyzed, because that repository has no `upstream` remote and is outside this target. Revisit with a separate run against `runvil-docs`.
- Pushing either repository.

</decision-record>
<project-survey>

## Project Survey

- Status: complete
- Build command: `cargo build --locked` (the workspace `default-members` build only the gateway, `crates/gateway/app`). The desktop app is `cargo build --locked -p workshop`. The Engine crates alone: `cargo build --locked -p runvil -p runvil-engine -p runvil-lua -p runvil-parser -p runvil-types -p runvil-vfs -p runvil-model-client`.
- Focused test command pattern: `cargo nextest run --locked -p <crate> --all-features <test-name-substring>`, for example `cargo nextest run --locked -p runvil-lua --all-features offer`. For a Workshop crate (`workshop`, `workshop-server`, `workshop-server-api`), drop `--all-features`. For one Node test file: `node --test <path>.test.mjs` from the package directory, after `npm run build` when the file imports `dist/`.
- Component test command pattern: `cargo nextest run --locked -p <crate> [-p <crate> ...] --all-features`; for this plan's Engine work, `cargo nextest run --locked -p runvil-lua -p runvil-engine -p runvil-types --all-features`. Node UI packages: `npm test` in the package directory (`crates/gateway/config-ui/ui`), or `npm test --workspaces --if-present` from `crates/workshop`, with `npm ci` first when `node_modules` is missing. Structural checks: `cargo test -p build-xtask`.
- Full-suite test command: `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`, then `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`. The workspace run includes `build-xtask`'s structural checks. cargo-nextest 0.9.128 is installed; profiles are in `.config/nextest.toml`.
- Linter command: `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`, and with the same variable `cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`. Clippy covers `cargo check`; the one extra check is `cargo check -p gateway --no-default-features`. Companion gates:
  - wording scan: `node --test crates/workshop/ui/test/docs-claims.mjs` (no `npm ci` needed, runs from any directory);
  - facade surface: `Remove-Item Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue; cargo +nightly-2026-09-05 xtask api --check`, compared against `crates/runvil/public-api.txt` (the pinned nightly is installed);
  - UI types: `npm run typecheck` in `crates/gateway/config-ui/ui`;
  - supply chain: `cargo deny check` (the pre-push hook runs it when installed).
- Formatter check command: `cargo fmt --all --check` (rustfmt `style_edition = "2024"`; the pre-commit hook runs it).
- Docs command: `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`. With the same flags, CI also runs `cargo doc -p runvil --no-deps` (default features), `cargo doc -p harness --no-deps`, and `cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`. The `runvil-docs` book: `$env:RUNVIL_DOCS='c:\Users\Vinnie\cursor\runvil-docs'; cargo xtask site --books-only`.
- Test placement and naming conventions:
  - Unit tests sit in sibling files wired under `#[cfg(test)]` with a path attribute: `foo-tests.rs` or `foo-tests-<label>.rs` beside `foo.rs` (for example `lua/src/messages-tests-list.rs`), or, in a module directory, `foo/tests.rs`, which wires its splits as `#[path = "tests-<label>.rs"] mod <label>;` (`lua/src/tools/tests.rs` wires `tests-adhoc.rs`, `tests-offering.rs`, `tests-parameters.rs`, `tests-render.rs`).
  - Large suites use a `tests/` module directory inside `src`, one file per topic with `<topic>-<label>.rs` splits (for example `engine/src/execute/tests/debug_and_counts.rs` and `debug_and_counts-infer-rounds.rs`). A crate's top-level `tests/` holds integration tests under Cargo target conventions.
  - Test functions are snake_case sentences stating the behavior, for example `tool_name_accepts_an_id_and_refuses_a_string_without_a_slash`.
  - Lua tests build VMs with `crate::test_vm` (`TestVm`, `lua_over`, `echo_tool`, `tool_at`); Engine execute tests share `execute/tests/fixtures.rs`.
  - `unwrap` and `expect` are allowed in tests only. No Engine test sleeps, reads the wall clock, waits with a timeout, or draws OS entropy; pseudo-random input comes from a fixed seed the failure prints; temporary directory names come from the test name, process id, and a counter.
  - Node tests are `src/**/*.test.mjs`, run by `node --test`.
- Directory map:
  - `crates/`: every Rust crate plus the UI packages. Public crates sit at the top level (`runvil`, `runvil-plugin`, `harness`, `harness-gateway-client`, `gateway-api-types`, `gateway-api-discovery`, `plugin-web`, `plugin-mcp`, `plugin-user-input`, `shared-*`, `build-*`). Manifestless containers hold each family's private crates: `runvil-internal/` (engine, lua, parser, types, vfs, model-client), `harness-internal/` (runner, plugin-loader, test-plugin), `gateway/` (app, cloud-providers, config, config-ui, local, logging, progress, protocol, routing, web-search, and the `stt/` subsystem), and `workshop/` (desktop, server, server-api, agents, gateway, menu, protocol, registry, run-log, status, support, user-state, workspace, plus the `ui`, `look`, and `platform` web packages). `crates/shared-ui` is a TypeScript and CSS package, not a crate.
  - `crates/build-*`: build tooling. `build-xtask` (`cargo xtask`: structural checks, facade surface, site), `build-workshop` (`cargo workshop`), `build-ceiling` (the 500-line file check every build script runs), `build-ui`, `build-user-guide`, `build-llama-cuda`.
  - `guide/`: user guide books, site chrome, and landing page.
  - `prompts/`: example prompt files.
  - `local/`: local development gateway config, env files, profiles, and STT fixtures.
  - `tools/`: Node scripts for staging the gateway sidecar and live TTS checks.
  - `vibe/`: dated plan and design records (`YYYY-MM-DD-N-<slug>.md`, with monthly folders).
  - `.github/workflows/`: CI (`ci.yml`) and release workflows. `.githooks/`: pre-commit (fmt) and pre-push (docs-claims, headless gateway check, clippy, deny). `.cargo/config.toml`: the `xtask` and `workshop` aliases and the Windows `rust-lld` linker. `.config/nextest.toml`: nextest profiles.
  - `images/`: README banners. `target/`: build output.
- Component boundaries:
  - Engine: `runvil-types` and `runvil-vfs` at the bottom; `runvil-model-client` depends on types; `runvil-lua` on model-client, types, and vfs; `runvil-parser` on lua and types; `runvil-engine` on lua, parser, model-client, types, and vfs; the `runvil` facade on all of them; `runvil-plugin` on types and vfs. Engine crates never depend on `tokio`, `tokio-util`, `async-trait`, `reqwest`, `getrandom`, or `fastrand`.
  - Harness: `harness-runner` depends on `runvil` and `runvil-plugin`; `harness-plugin-loader` on `runvil-plugin`; `harness` on runner, plugin-loader, `runvil`, and `runvil-plugin`; `harness-gateway-client` on `harness`, `runvil`, and `plugin-web`.
  - Plugins (`plugin-*`) depend only on `runvil-plugin`, `shared-*`, and outside libraries.
  - Gateway (`gateway` and `gateway-*`) never depends on Engine, Harness, or Workshop crates; `gateway-stt` is the STT subsystem's public member.
  - Workshop: `workshop` (desktop app) depends only on `workshop-server-api`, which depends only on `workshop-server`, which depends on `harness`, `harness-gateway-client`, the plugins, `runvil`, `runvil-plugin`, and the private Workshop crates. It reaches the gateway only through `gateway-api-types` and `gateway-api-discovery`.
  - Outside a family, only its public crates may be named; `shared-*` crates depend on no product crate. `cargo test -p build-xtask` enforces all of this.
- Conventions summary:
  - Five capitalized defined terms (Engine, Harness, Host, Pack, Plugin) have one meaning each. Engine crates call their driver "the caller" and never mention the Host or a Host application; the docs-claims scan enforces this in code, comments, and test strings.
  - Strict workspace lints: clippy `all` and `pedantic` denied, `unwrap_used` and `expect_used` denied outside tests, `#[expect(.., reason = ..)]` instead of `#[allow]`, `unsafe_code` denied, and `missing_docs` warned (denied in CI), so every module and item carries `//!` or `///` docs.
  - Flat source directories: one or two related files sit beside the parent as `foo-bar.rs` with `#[path = "foo-bar.rs"] mod bar;`, and three or more become a `foo/` directory.
  - Every Rust file under `src` or `tests` stays at or under 500 lines (`build-ceiling`).
  - Comments state only non-obvious constraints; workarounds cite an upstream issue URL; no comment says what changed.
  - Error messages are written for model consumption: concise and self-contained, naming what is unmet with required versus actual (for example the `<verb> takes ...` refusals).
  - JSON reaching a recorder round-trips exactly (`serde_json` with `float_roundtrip`, sorted keys, finite numbers).
  - Behavior changes ship with tests in the same change; prefer types, then behavior tests and deterministic fault injection.
  - Rust edition 2024 on the stable toolchain, with a 1.95 floor for the Engine crates; Cargo features gate only real constraints.
  - Commit subjects are short and imperative (`Add ...`, `Rename ...`, `Close plan: ...`).

</project-survey>
<execution-plan>

## Execution Instructions

<step-1>

### Step 1: Build the config UI bundle before its tests [completed]

- Component: Config UI test build
- Tests: run each in PowerShell as written; each exits 0 unless its note says otherwise.
  - `Set-Location c:\Users\Vinnie\cursor\runvil\crates\gateway\config-ui\ui; Remove-Item -Recurse -Force dist -ErrorAction SilentlyContinue; npm test` builds `dist/` through `pretest`, then every test passes (211 of 211 when the plan was written).
  - `Set-Location c:\Users\Vinnie\cursor\runvil\crates\gateway\config-ui\ui; npm run typecheck`
  - `Set-Location c:\Users\Vinnie\cursor\runvil; rg -n "debug \x60cargo build\x60" crates/gateway/config-ui` prints nothing and exits 1, which is rg's no-match code.

- Placement: first. This component touches only the Node package in `crates/gateway/config-ui/ui`, shares no file with any other component, and depends on nothing, so it lands ahead of the Rust work. Its two pieces, the `pretest` script (F-1) and the corrected notes (F-2), are built jointly in this one step: the new notes name the `pretest` script, so they are only true once it exists, and the checks above cover both.
- Target: the config UI tests never run against a stale bundle, and their notes say how the bundle is built. Today `package.json` defines `build` (`node build.mjs`) and `test` (`node --test "src/**/*.test.mjs"`) but no `pretest`, and the tests import the gitignored `dist/app.js` and `dist/app.css`. A `dist/` built before `ceae9ad0f` once failed two `tab-bar.test.mjs` tests until a rebuild.
- F-1: in `crates/gateway/config-ui/ui/package.json`, add `"pretest": "npm run build"` to `scripts`, beside `test`. Leave CI's separate "Build config UI" step in `.github/workflows/ci.yml` (lines 311-317) as it is, so a build failure is still reported under its own name; the build then runs twice in that job, about a second, which is the accepted cost.
- F-2: reword the three notes that say a debug `cargo build` produces `dist/`. `crates/gateway/config-ui/build.rs` writes its bundle to `$OUT_DIR/ui-dist/`, never into the repository.
  - `crates/gateway/config-ui/ui/src/test-support.mjs` line 19 becomes ``/** Imports the built bundle once; `npm test` builds it first through the `pretest` script. */``.
  - In `crates/gateway/config-ui/ui/src/main.test.mjs` (lines 5-6) and `crates/gateway/config-ui/ui/src/styles/tokens.test.mjs` (lines 6-7), the sentence "Run after `npm run build` (a debug `cargo build` also produces dist/)." becomes "Run with `npm test`, whose `pretest` script builds dist/ first." Keep each comment wrapped like its neighboring lines.
  - Leave `crates/gateway/config-ui/ui/build.mjs` lines 1-5 and `crates/gateway/config-ui/src/assets.rs` line 36 as they are; they describe the crate's own bundle correctly.
- Fail first: before adding `pretest`, run the first Tests command and confirm it fails because the tests cannot import the missing `dist/app.js`, as it does at `8c0d7e4bf`. Then make both changes.
- Commit `Build the config UI bundle before its tests` with the four files. Do not push.

</step-1>

<step-2>

### Step 2: Bound offer arrays at the held key limit [completed]

- Component: Offer decoder
- Tests: run each in PowerShell from `c:\Users\Vinnie\cursor\runvil` as written; each exits 0.
  - `cargo nextest run --locked -p runvil-lua --all-features`
  - `cargo fmt --all --check`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy -p runvil-lua --all-targets --all-features`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`

- Placement: the offer decoder is the first Rust component because the docs component documents its new refusal, and it fixes the most severe defect, a script-chosen `__len` that can panic a run or abort the process. It has two pieces, this bound and the adhoc object check in Step 3. They are sequential because both edit `crates/runvil-internal/lua/src/tools/offer.rs`, and the bound goes first because the docs component needs its final message.
- Target: section Lua can no longer turn a `tools.offer` or `tools.offer_dynamic` call into a Rust panic or a process abort. Today `array` sizes `Vec::with_capacity` from `table.len()`, which honors `__len`, before reading any entry, and holds one live `mlua::Value` per entry. `__len` returning `math.maxinteger` panics with "capacity overflow", `1 << 40` requests at least 16 TiB and aborts, and a plain array of about 1,000,000 or more entries exhausts mlua's reference stack and panics with "cannot create a Lua reference, out of auxiliary stack space". In the Workshop the panic ends one conversation's run and the abort kills the server.
- Change, in `crates/runvil-internal/lua/src/tools/offer.rs` `array`:
  - Read `table.len()?` first. When it is greater than `crate::walk_budget::HELD_KEY_LIMIT` (`pub(crate)`, a `usize` of 100,000), return `mlua::Error::external(format!("{verb} takes an array of at most {HELD_KEY_LIMIT} entries, got {len}"))` before `Vec::with_capacity` and before any `get`. `len` is the `#t` value as the script reported it, an `i64`, so `math.maxinteger` prints `9223372036854775807`. Compare it with the `usize` limit without a lossy `as` cast.
  - Keep the current behavior for every `len` up to the limit, including a negative `len` reading as an empty array through `usize::try_from(..).unwrap_or(0)`. Keep the per-entry checks, the raw-key check, and the rule that every entry is checked before any is recorded, so a refusal records nothing. The rendered-list refusal still comes first, because `install_offer` checks it before decoding.
  - Update `array`'s doc comment to state the bound.
- Tests to write, in a new `crates/runvil-internal/lua/src/tools/tests-offer-limits.rs` that opens with a `//!` line naming its concern, wired from `crates/runvil-internal/lua/src/tools/tests.rs` as `#[path = "tests-offer-limits.rs"] mod offer_limits;` beside the other splits. Build each VM with `lua_over(offer_set())` from `crate::test_vm`, whose set offers `web`, and read refusals with `raised` and recorded offers with `offers`:
  1. `tools.offer(setmetatable({}, { __len = function() return math.maxinteger end }))` raises text containing `tools.offer takes an array of at most 100000 entries, got 9223372036854775807`. The same VM then runs `tools.offer('web')` without error, and `offers` is then exactly `static:web`.
  2. `local t = {} for i = 1, 1200000 do t[i] = 'web' end tools.offer(t)` raises text containing `got 1200000`. This is the size `crates/runvil-internal/lua/src/tests/json/limits.rs` uses, and the table fits in the 64 MiB Lua heap.
  3. An array of exactly 100,000 `'web'` entries is accepted, and `offers` is exactly `static:web`.
  4. `tools.offer_dynamic` with an array of 100,001 `'web'` entries raises `tools.offer_dynamic takes an array of at most 100000 entries, got 100001`.
  5. `__len` returning `1 << 40` raises the bound error with `got 1099511627776`.
- Fail first: write cases 1 and 2 alone, run `cargo nextest run --locked -p runvil-lua --all-features offer_limits`, and confirm both fail, with the "capacity overflow" panic and the "out of auxiliary stack space" panic, as they do at `8c0d7e4bf`. Then add the bound and cases 3 to 5. Never run case 5 before the bound lands: it aborts the test binary, which neither `#[should_panic]` nor `catch_unwind` can observe.
- The existing offer tests in `crates/runvil-internal/lua/src/tools/tests-offering.rs` must still pass. They cover the array forms, gaps, stray keys, nested tables, `__len` proxies, and `tools.offer(tools.packs)`.
- Commit `Bound offer arrays at the held key limit` with the code and its tests. Do not push.

</step-2>

<step-3>

### Step 3: Refuse offered adhoc objects that no adhoc tool holds [completed]

- Component: Offer decoder
- Tests: run each in PowerShell from `c:\Users\Vinnie\cursor\runvil` as written; each exits 0.
  - `cargo nextest run --locked -p runvil-lua --all-features`
  - `cargo fmt --all --check`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy -p runvil-lua --all-targets --all-features`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`

- Placement: the second offer decoder piece, built after Step 2 because both edit `crates/runvil-internal/lua/src/tools/offer.rs` and must not be built in parallel.
- Target: a run that ignored the `adhoc` refusal never reaches an internal error by offering a catalog `adhoc/*` tool object; the offer line raises the ordinary unknown-tool error. Today `RunContext::prepare` refuses a catalog or roster naming the Pack id `adhoc` through `Requirements::reserved_adhoc`, but that refusal is advisory and `runvil::Run::new` accepts an unprepared context. `install_tool_objects` (`crates/runvil-internal/lua/src/tools/objects.rs`) then gives the catalog tool `adhoc/x` an object, `Offering::target` records any `LuaToolHandle` unchecked, and the render fails in `expanded` (`crates/runvil-internal/lua/src/vm/state.rs`) with `Error::Internal("offered tool \"adhoc/x\" has no binding")`. The string form, `tools.offer('adhoc/x')`, already raises `tools.offer: "adhoc/x" is not a tool in this run` from `named`.
- Change, in `crates/runvil-internal/lua/src/tools/offer.rs` `Offering::target`: in the `LuaToolHandle` arm, when the handle's id is under the `adhoc` Pack (`is_adhoc(&id.pack())`) and `self.adhoc.binding(&id.to_string())` is `None`, return `mlua::Error::external(format!("{verb}: {name:?} is not a tool in this run"))` with `name` the id text, which is the string path's message. Map a poisoned-lock error from `binding` with `mlua::Error::external`, as `named` does. Record every other handle as before.
- Leave the internal error in `expanded` as the invariant check it now is, and leave the `adhoc` reservation in the registry, the parser, and `RunContext::prepare` unchanged. The message is not new, so the docs do not change. Do not skip catalog `adhoc/*` tools in `install_tool_objects`, which would make the tool vanish from Lua silently, and do not make `Run::new` enforce `reserved_adhoc`, which would change public run-start behavior.
- Test to write, in `crates/runvil-internal/lua/src/tools/tests-offering.rs`: build the VM with `lua_over` over a `ToolSet`, made with `ToolSet::new` the way `crate::test_vm::offer_set` makes one, whose offering holds the catalog tool `adhoc/x` (for example the binding `crate::ToolBinding::from_info("adhoc_x", &tool_at("adhoc/x"))`), as a run that ignored the refusal would. Then:
  - `local ok, err = pcall(tools.offer, tools.get('adhoc/x'))`;
  - assert `not ok` and that `tostring(err)` contains `tools.offer: "adhoc/x" is not a tool in this run`;
  - assert that `current_tool_bindings` for the VM returns `Ok` with no `adhoc/x` binding (`crate::test_vm::scoped` resolves it and expects `Ok`).
- Fail first: write the test, run `cargo nextest run --locked -p runvil-lua --all-features offering`, and confirm `assert(not ok)` fails, as it does at `8c0d7e4bf`, where the offer succeeds and the render then returns the internal error. Then make the change.
- An adhoc tool the section defined, offered by object, is still recorded; the existing adhoc offer tests, such as the `grab` object in `every_accepted_form_records_each_target_once_in_call_order_under_its_verbs_mode`, cover it. `tests-offering.rs` (332 lines now) stays at or under 500.
- Commit `Refuse offered adhoc objects that no adhoc tool holds`. Do not push.

</step-3>

<step-4>

### Step 4: Make tools.adhoc all or nothing [completed]

- Component: Adhoc definition
- Tests: run each in PowerShell from `c:\Users\Vinnie\cursor\runvil` as written; each exits 0.
  - `cargo nextest run --locked -p runvil-lua --all-features`
  - `cargo fmt --all --check`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy -p runvil-lua --all-targets --all-features`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`

- Placement: its own component, independent of the offer decoder. It comes before call-count seeding (Step 5) because both edit `crates/runvil-internal/lua/src/tools.rs` and `crates/runvil-internal/lua/src/tools/tests-adhoc.rs`, and this change stays inside `runvil-lua` while Step 5 crosses into `runvil-engine`. One piece, one step.
- Target: `tools.adhoc` either defines a tool completely or leaves no trace of it. Today `install_adhoc` calls `adhoc.register(&name, &info)` (the Rust `AdhocTools` push), then `handlers.raw_set(name, handler)`, then `insert_tool_object(lua, &binding)`. The last two allocate in the Lua heap. If either fails after `register` succeeded, for example from a memory error an author's `pcall` survives, the tool has a binding but no handler or object, a redefinition raises "already registered", and a call reaches `Error::Internal("an adhoc tool answer names a registered handler")`.
- Change:
  - `crates/runvil-internal/lua/src/vm/adhoc.rs`: `AdhocTools::register` takes the prebuilt binding, `pub(crate) fn register(&self, binding: ToolBinding) -> Result<()>`, so the tool object and the registry hold the same binding. It still fails only on a poisoned lock. Drop any import it no longer needs.
  - `crates/runvil-internal/lua/src/tools.rs` `install_adhoc`, after `adhoc_info`: `let binding = ToolBinding::from_info(&name, &info);`, then `handlers.raw_set(name.as_str(), handler)?;`, then `let object = insert_tool_object(lua, &binding)?;`, then `adhoc.register(binding)`. If `register` fails, set the handler entry for the name and the tool-object entry for the id back to `nil`, and return the error; a small helper beside `insert_tool_object` in `crates/runvil-internal/lua/src/tools/objects.rs` can clear the object entry. Otherwise return `object`.
  - A failure in the handler write or the object insert leaves `AdhocTools` untouched. A handler entry left by a failed object insert is unreachable, because dispatch and the collision checks read only `AdhocTools`, and a redefinition overwrites it.
  - Update `install_adhoc`'s doc comment to say the definition is all or nothing.
  - Make `TOOL_OBJECTS_REGISTRY` in `crates/runvil-internal/lua/src/tools/objects.rs` `pub(super)` or `pub(crate)` so the test can reach it.
- Do not keep `register` first and undo it when a Lua write fails; that needs an `AdhocTools` removal method for a failure the reorder avoids outright.
- Test to write, in `crates/runvil-internal/lua/src/tools/tests-adhoc.rs`, on the test VM's Lua state:
  - save the table stored under the named registry value `TOOL_OBJECTS_REGISTRY` (`runvil.tools.objects`), then set that value to `false`, so `insert_tool_object` fails;
  - `pcall(tools.adhoc, { name = 'x', description = 'y' }, function() return 'z' end)` returns false;
  - restore the saved table, then assert `vm.adhoc_tools().binding("adhoc/x")` is `Ok(None)`, and that `tools.adhoc({ name = 'x', description = 'y' }, function() return 'z' end)` now succeeds and returns an object equal (`==`) to `tools.get('adhoc/x')`.
- Fail first: write the test, run `cargo nextest run --locked -p runvil-lua --all-features adhoc`, and confirm the `None` assertion fails, as it does at `8c0d7e4bf`, where `register` ran before the failed insert, so the binding is `Some` and a second definition raises `tools.adhoc name "x" is already registered`. Then make the change.
- `tests-adhoc.rs` (284 lines now) stays at or under 500.
- Commit `Make tools.adhoc all or nothing`. Do not push.

</step-4>

<step-5>

### Step 5: Seed tool call counts from the rendered list

- Component: Call-count seeding
- Tests: run each in PowerShell from `c:\Users\Vinnie\cursor\runvil` as written and in this order, because the seventh clears the variables the fifth and sixth set; each exits 0.
  - `cargo nextest run --locked -p runvil-lua -p runvil-engine -p runvil-types --all-features`
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`
  - `cargo fmt --all --check`
  - `$env:CARGO_BUILD_WARNINGS='deny'; cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `$env:RUSTDOCFLAGS='-D warnings'; cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`
  - `Remove-Item Env:CARGO_BUILD_WARNINGS, Env:RUSTDOCFLAGS -ErrorAction SilentlyContinue; cargo +nightly-2026-09-05 xtask api --check` reports no change to `crates/runvil/public-api.txt`.
  - `node --test crates/workshop/ui/test/docs-claims.mjs`

- Placement: the last `runvil` component. It spans `runvil-lua` and `runvil-engine` and edits `tools.rs` and `tests-adhoc.rs` after Step 4. Its two pieces, the `runvil-lua` seeding API and the `runvil-engine` dispatch path, are built jointly in this one step, because changing `SectionVm::install_tool_call_counts` to take ids breaks the Engine caller until both land. Every `runvil` item has landed once this step's code is in, so its Tests carry the plan's exit checks.
- Target: `tools.calls` follows its stated contract after the render, so a tool outside the rendered list gets no count until its first call. Today `section_call_counts` (`crates/runvil-internal/engine/src/execute/scheduler/tool_call.rs`) calls `current_tool_bindings` on every catalog and adhoc dispatch, and `script_call_counts` (`crates/runvil-internal/engine/src/execute/section_context.rs`) calls `counts.ensure` for every binding returned. `expanded` maps an offered adhoc Pack to the live adhoc registry, and `tools.adhoc` works after the render. So in a section that offered `tools.packs.get('adhoc')` before its first loop round, an adhoc tool defined after that round, which the model cannot call, reads `0` from `tools.calls` after any later dispatch instead of raising `has no seeded count`. Routing is unaffected. Once the section's list is rendered, counts seed only from the rendered list's ids; seeding before the render (D1-19) stays as it is.
- Change:
  - `crates/runvil-internal/lua/src/messages.rs`: add `pub(crate) fn rendered_ids(&self) -> Option<Vec<ToolId>>` on `MessageList`, next to `rendered_target`. It returns the ids in the rendered list's `dispatch` values (`Advertised.id`), `None` before the render, and never `RenderedTools.dynamic`. The file is at 490 physical lines. If this function and its import take it over 500 after `cargo fmt`, build the same ids in `SectionVm::rendered_ids` from the existing `MessageList::rendered_tools()` instead and leave `messages.rs` unchanged.
  - `crates/runvil-internal/lua/src/vm/state.rs`: add `pub fn rendered_ids(&self) -> Option<Vec<ToolId>>` on `SectionVm`, delegating to `self.messages`, and change `install_tool_call_counts(&self, bindings: &[ToolBinding])` to take `ids: &[ToolId]`. Rewrite the `current_tool_bindings` doc comment so it no longer claims the bindings never change once the list is rendered: an offered adhoc Pack expands to the adhoc tools defined when the function runs, and after the render the Engine reads the rendered list instead.
  - `crates/runvil-internal/lua/src/tools.rs`: `install_tool_call_counts(lua, bound_tools, adhoc, ids: &[ToolId])` seeds `ToolCallCounts::new(ids.iter().map(ToString::to_string))`. The unseeded-key hints, ` (a catalog tool that was neither offered in this section nor called with tools.call)` and ` (an adhoc tool that was neither offered in this section nor called with tools.call)`, are unchanged.
  - `crates/runvil-internal/engine/src/execute/section_context.rs`: `script_call_counts(&mut self, ctx, ids: &[ToolId])` and `install_section_scope(.., ids: &[ToolId])` take ids instead of bindings. Their doc comments say the seeded set is the section's live scope before its first render and its rendered list after.
  - `crates/runvil-internal/engine/src/execute/scheduler/tool_call.rs`: `section_call_counts` uses `vm.rendered_ids()` when it is `Some`, and otherwise maps `current_tool_bindings(..)` to ids, as today. Its callers, `prepare_catalog_call` and `prepare_adhoc_call` in `tool_call-adhoc.rs`, are unchanged.
  - Test callers pass ids: `crates/runvil-internal/lua/src/tools/tests.rs` (`.install_tool_call_counts(&[echo])`) and `crates/runvil-internal/lua/src/tools/tests-adhoc.rs` (`.install_tool_call_counts(&bound)`).
  - No public API change: these are internal crates, and `crates/runvil/public-api.txt` must not change.
- Do not snapshot the adhoc Pack expansion inside `runvil-lua` at render time, which duplicates what the rendered list already holds, and do not reword the "Calls and counts" contract to allow live seeding.
- Test to write, in `crates/runvil-internal/engine/src/execute/tests/debug_and_counts.rs`: drive one `models.loop` round with a scripted model reply that makes no tool call, the way `a_round_advertises_the_section_scope_with_adhoc_tools` in `crates/runvil-internal/engine/src/execute/tests/chat_scope.rs` drives a rendered round. The section Lua, in order:
  - `local grab = tools.adhoc({ name = 'grab', description = 'Grab' }, function() return 'got' end)`, then `tools.offer(tools.packs.get('adhoc'))`;
  - one loop round, which renders the list;
  - `tools.adhoc({ name = 'late', description = 'Late' }, function() return 'late' end)`, then `tools.call(grab, {})`;
  - `local ok, err = pcall(function() return tools.calls['adhoc/late'] end)`;
  - assert `not ok`, and that `tostring(err)` contains both `has no seeded count` and `(an adhoc tool that was neither offered in this section nor called with tools.call)`;
  - assert `tools.calls['adhoc/grab'] == 1`.
- Fail first: write the test, run `cargo nextest run --locked -p runvil-engine --all-features debug_and_counts`, and confirm `assert(not ok)` fails, as it does at `8c0d7e4bf`, where `section_call_counts` re-expands the adhoc Pack to include `late`, so `tools.calls['adhoc/late']` reads `0` and `pcall` succeeds. Then make the change.
- The existing counts tests still pass: `tool_call_counts_seed_by_id_and_explain_an_unseeded_key` in `crates/runvil-internal/lua/src/tools/tests.rs` and the adhoc-hint assertion in `debug_and_counts.rs`. `debug_and_counts.rs` (305 lines now) stays at or under 500.
- Commit `Seed tool call counts from the rendered list`. Do not push.

</step-5>

<step-6>

### Step 6: Document the offer array limit

- Component: Offer limit docs
- Tests: run each in PowerShell as written; each exits 0.
  - `Get-ChildItem c:\Users\Vinnie\cursor\runvil-docs\src -Recurse -Filter *.md | Select-String -Pattern ([string][char]0x2014)` prints nothing.
  - `git -C c:\Users\Vinnie\cursor\runvil-docs grep -n "takes an array of at most 100000 entries" -- src` prints exactly one line, the new bullet in `src/language/12-tools.md`.
  - `git -C c:\Users\Vinnie\cursor\runvil-docs grep -n "array length | 100,000 entries per" -- src` prints exactly one line, the new row in `src/language/17-quick-reference.md`.
  - `rg -n "takes an array of at most" c:\Users\Vinnie\cursor\runvil\crates\runvil-internal\lua\src\tools\offer.rs` prints the format string, and the documented message matches it with `{verb}` as `tools.offer` and `{HELD_KEY_LIMIT}` as `100000`.
  - `Set-Location c:\Users\Vinnie\cursor\runvil; $env:RUNVIL_DOCS='c:\Users\Vinnie\cursor\runvil-docs'; cargo xtask site --books-only`

- Placement: last. It documents Step 2's refusal, so it must match that step's final format string; its site build runs from the finished `runvil` tree; and it commits in another repository. One piece, one step.
- Repository: `c:\Users\Vinnie\cursor\runvil-docs`, branch `master`, HEAD `24e8143` ("Document the one offer API and bring the guide up to date"). This step makes one commit there, no `runvil` commit, and pushes nothing. The design record `vibe/2026-10-23-1-one-offer-api.md` is history and is not amended.
- `src/language/12-tools.md`, in "Errors from `tools.offer`": after the bullet that begins "A table with a key outside `1` to `#t` raises" (line 964), add ``- An array longer than 100,000 entries raises `tools.offer takes an array of at most 100000 entries, got {n}` before any entry is read, so an array whose `#t` is 100,001 raises `tools.offer takes an array of at most 100000 entries, got 100001`.``. The paragraph after that list already says `tools.offer_dynamic` raises every one of these errors under its own name.
- `src/language/17-quick-reference.md`, in the "Limits and defaults" table, add ``| `tools.offer` array length | 100,000 entries per `tools.offer` or `tools.offer_dynamic` call | fixed | [Errors from `tools.offer`](12-tools.md#errors-from-toolsoffer) |``. The table is sorted by limit name, so the row goes last, after `request_timeout`, rather than directly beside the `json.encode` key limit row (line 263) that shares its 100,000.
- No em dashes or double dashes in either addition.
- Commit `Document the offer array limit` in `runvil-docs`. Do not push.

</step-6>

</execution-plan>
