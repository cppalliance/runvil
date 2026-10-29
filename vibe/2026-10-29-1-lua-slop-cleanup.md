# Vibelight: runvil-internal/lua

<product-contract>

## Product Requirements

### Scope

- **Crate:** `runvil-lua` at `crates/runvil-internal/lua`, outline 430 KB, 9,256 lines
- **Size:** 135 source files, 28,852 lines of Rust
- **Outlines:** current at 2026-10-27 05:09 when the analysis read them
- **Context:** 48 other outlines, not analyzed
- **Excluded:** 7 crates, each build tooling or a test fixture
- **Doc roots:**
  - the operator's own root: none stated, so the repository's own were used
  - `crates/runvil-internal/lua/README.md` and `AGENTS.md`
  - the repository's `README.md` and `AGENTS.md`
  - `../runvil-docs/src/language/`, 17 chapters, the guide that documents this crate
- **Limits:**
  - the outline holds tests, benches, and examples, so reachability is settled by search
  - one vibelight warning: 99 items written more than one way, in Spellings
  - `vibe/` is not a doc root: 264 dated records, too stale to settle anything
  - `runvil-design/design/` is provisional by its own record, so it is not a doc root
  - the guide goes stale too, so source settles every disagreement, not the doc
  - a doc claim is drift only when the code contradicts it today, not when it is merely old
  - four dispatch aborts did not stop their agents, so two wrote each of three files
  - `findings-5.md` holds only the last drift writer; its two earlier drafts were lost
  - line numbers were read at `7a5341bba`; HEAD has since advanced to `4eb444630`
  - the engine moved in 61 files after that baseline, so every engine line citation has drifted
  - the lua crate changed once: `NonEmpty::new` became `pub` in `1dbdbf70d`, which no item touches
  - the outline is stale against HEAD, so a fresh `vibelight` run is needed before another scan

- Stats: 121 raised and 9 added in verification, 15 rejected, 11 duplicates and 6 same-cause findings merged, 98 kept
- Stats: of the 98 kept, 86 cheap and 12 substantial, and both operator decisions answered in favor
- Goal: one crate that names each fact once, in the file that owns it, with no doc contradicting its own body
- Goal: delete the instruction-quota machinery the guide already says does not exist
- Goal: close the dispatch reporting path so a public entry point cannot skip its event
- Non-goal: no author-visible behavior change: every Lua global, table shape, and message stays as the guide documents it, and every type or signature change stays inside the workspace
- Non-goal: no new dependency, no new crate, no ratchet on line counts or test counts
- Success: the AGENTS.md Verification gates pass: the full suite, clippy with `CARGO_BUILD_WARNINGS=deny`, `cargo fmt --all --check`, the docs build with `RUSTDOCFLAGS="-D warnings"`, and the wording scan
- Success: a fresh plain `cargo vibelight` run at the repository root shows each renamed and deleted declaration

## Functional Specification

### Debt Inventory

**State and invariants**

- State (substantial), `Request::ToolCall` pairs two `Option`s whose third state exists only for a test hook, `src/protocol/request.rs:139` (S2-23)
- State (substantial), `TaskStatus.ok` restates what its phase already decides for every phase but `Done`, `src/protocol/answer.rs:218` (S2-12)
- State (substantial), every `Answer` variant wraps its own `Result` while the failure side is identical for all, `src/protocol/answer.rs:246` (S2-11)
- State (substantial), `increment` errors on an unseeded id, so every dispatch site must seed first and the arm is unreachable, `src/scope.rs:59` (S3-23)
- State (substantial), the compacting flag and the held snapshot are hand-paired across five sites, one arm unreachable, `src/models_loop/machine.rs:74` (S4-12)
- State (substantial), `attach_rendered_tools` returns nothing, so its engine caller re-reads a slot it just filled, `src/messages.rs:237` (S4-9)
- State (substantial), `sys_live` is an `Option` set before `injected()` returns, so every `None` arm is dead, `src/vm.rs:146` (S1-6)
- State (substantial), `prepare_dispatch` accepts `None` publicly and then fires no `ToolResult` at all, `src/dispatch.rs:119` (V3-3)
- State (substantial), `ModelReport` carries its turn in a field named `script`, typed `ScriptReport`, `src/dispatch.rs:46` (S6-17)

**Vestiges**

- Vestiges (substantial), the instruction quota cannot trip because `HOOK_BUDGET` is `u64::MAX`, and `InstructionBudget` is named for a budget it no longer spends, `src/hardening.rs:21` (S1-2)
- Vestiges (substantial), the `model_tool_call` stash installs into every production VM for a test-only hook, `src/coro.rs:294` (S1-4)
- Vestiges, `#[non_exhaustive]` on `LuaProgram` restricts nothing its private fields do not, `src/program.rs:58` (S1-17)
- Vestiges, `resume_block_coro` and `inject_values` have only test and bench callers, `src/vm/run.rs:84` (S1-14)
- Vestiges, eleven sites in nine files present a "scheduler mode" and a "direct chunk path" as production modes, `src/lib.rs:32` (S1-15)
- Vestiges, `Compactor` derives `Default` whose only use is one test, `src/compactors.rs:104` (S4-15)
- Vestiges, `UsageAnchor::tokens` is a production accessor only tests read, `src/compactors.rs:204` (S4-17)
- Vestiges, the `error` module doc names a "core" crate that does not exist, `src/error.rs:5` (S2-1)
- Vestiges, `SharedSource` and `Error::shared` cite a "resolver decision" cache nothing has, `src/error.rs:27` (S2-2)
- Vestiges, `ToolCallOutcome` docs predate structured and adhoc outputs, `src/protocol/answer.rs:44` (S2-13)
- Vestiges, `ToolBinding.id` is documented as "the selected stable live identity" with no selection, `src/handles.rs:45` (S3-7)

**Duplication and reuse**

- Duplication, `tools.stats` and `tools.packs` hand-build one walkable read-only proxy twice, `src/tools/stats.rs:255` (S3-24)
- Duplication, the author refusal helper, `Error::Lua` as external, is defined in two modules, `src/json.rs:89` (S4-27)
- Duplication, `read_range` and `read_range_numbered` are one-line wrappers of one function, `src/engine_globals.rs:213` (S4-5)
- Duplication, `rendered()` duplicates `rendered_tools()` over one field, `src/messages.rs:216` (S4-10)
- Duplication, `install_prose` is `replace_prose` without its result, `src/globals.rs:116` (S4-7)
- Duplication, `LuaModelHandle::label` duplicates `name`, both reading `binding.alias()`, `src/models-userdata.rs:43` (S3-14)
- Duplication, the log length refusal hardcodes the limit its own constant holds, `src/engine_globals.rs:62` (V4-1)
- Duplication, `error::lua_quota` is a module of three strings for one `Display` match, `src/error.rs:212` (S2-5)
- Duplication, the store error field list is written out in two docs that already differ, `src/error-value-store.rs:6` (S2-9)
- Duplication, `direct_yield`, `raw_field`, and `json_field` build errors their callers discard, `src/protocol/parse.rs:46` (S2-17)
- Reuse, `parse::store` frames author-argument errors by hand instead of through `call_error`, `src/protocol/parse/store.rs:33` (V2-2)
- Duplication, the `tools` and `models` installers restate their module docs' shim note, and the `models` copy lost `models.compact`, `src/models.rs:244` (S3-17, S6-7)

**Naming**

- Naming, the offers type is `ToolRuntime`, its locks say "declaration runtime", its field doc says "tool-addition runtime", and `ModelRuntime` holds only a selection, `src/scope.rs:141` (S3-12, S6-5, S1-12)
- Naming, `bound_tools` names the unbound `ToolSet` at eleven sites, `src/vm.rs:123` (S3-8)
- Naming, `ToolCallCounts` is keyed by tool id but its API calls the key a name, `src/scope.rs:29` (S6-3)
- Naming, `tool_name` returns a tool id, `src/tools/decode.rs:22` (S3-9)
- Naming, `current_tool_bindings` returns `OfferedTool`s that are not bindings, `src/vm/state.rs:140` (S1-16)
- Naming, `store_conflicts` is documented "The slot" and read by a singular accessor, `src/vm.rs:151` (S1-11)
- Naming, `globals` and `messages` each define a function named `install_messages`, `src/globals.rs:145` (S4-8)
- Naming, `YieldParse::Call` and `FieldFailure::Call` reuse the name of the `call` request, `src/protocol/parse.rs:180` (S2-16)
- Naming, `prelude` calls the Pack's id its "local name" in one sentence and its id in the next, `src/prelude.rs:4` (S1-21)
- Naming, `Encoder.function` holds `to_json`'s message prefix, not a function, `src/json/encode.rs:162` (S4-26)
- Naming, `compile_internal` says "the coroutine shim prelude" for six different chunks, `src/program.rs:132` (S1-19)
- Naming, the snapshot module says "rendered tool list" and "send state" for the last send and the frozen sets, `src/messages-snapshot.rs:5` (S4-11)

**Placement and visibility**

- Placement (substantial), `install_section_loop_shim` exists to be called right after `install_coro_shims`, for agent VMs that no longer exist, `src/coro.rs:347` (S1-5)
- Placement, the store operation bodies and read helpers live outside the store module, `src/engine_globals.rs:24` (S4-6)
- Placement, `handles` holds no handle and its doc omits most of its contents, `src/handles.rs:1` (S3-6)
- Placement, `mod order` sits under `collection`, a fanout detail, though three modules use it, `src/collection.rs:16` (S4-19)
- Placement, `sys.rs` holds nine `var` items beside the one `sys` item, `src/sys.rs:17` (S1-23)
- Placement, record validation lives in the chat request parser, which never calls it, `src/protocol/parse/chat.rs:67` (S2-20)
- Visibility, `ToolRuntime.offers` is `pub`, so outside code can break "each held once", `src/scope.rs:143` (S3-13)

**Doc comments, by file**

- Doc comments, `src/error.rs:70` names a stale span list and repeats the module doc (S2-3)
- Doc comments, `src/error.rs:30` lists the shared-source callers and omits the `require` loader (V1-1)
- Doc comments, `src/error-value.rs:277` says `Raised` holds strings, though fields also hold integers (S2-7)
- Doc comments, `src/error-value.rs:51` and `:74` scope two error kinds narrower than what maps to them (S2-6)
- Doc comments, `src/protocol.rs:4` lists suspending calls and omits most task operations (S2-10)
- Doc comments, `src/protocol/parse.rs:166` states the `resolve_section_target` rule in six docs (S2-18)
- Doc comments, `src/protocol/request.rs:34` restates the handle, empty-list, and adhoc rules (S2-19)
- Doc comments, `src/protocol/request.rs:201` says every store operation takes the yield path (S2-24)
- Doc comments, `src/protocol/request.rs:230` says arguments are checked at parse, then describes a run-time check (S2-25)
- Doc comments, `src/protocol/record.rs:12` promises data-URI images, but any URL string parses (S2-22)
- Doc comments, `src/protocol/record.rs:94` says `tool_calls` is assistant-only, but any role may hold it (S2-21)
- Doc comments, `src/protocol/answer.rs:143` says an abandoned task is delivered as `cancelled`; it is never delivered (V2-1)
- Doc comments, `src/dispatch.rs:108` says "the loop" reports a model-issued result; `prepare_model_dispatch` does (S3-3)
- Doc comments, `src/dispatch.rs:26` names a `tool_call_id` field `ScriptReport` does not hold (S3-4)
- Doc comments, `src/dispatch.rs:5` states the no-call rule four times in one file (S3-5)
- Doc comments, `src/models.rs:39` calls the documented raw-model-id fallback "the hack" (S3-16)
- Doc comments, `src/models-userdata.rs:14` repeats its module doc and one boundary rule three times (S3-15)
- Doc comments, `src/models_loop.rs:110` cites a plan section and quotes a closure body that is not the code (S3-18)
- Doc comments, `src/tools.rs:5` restates the wire-name fallback `wire_name_of` owns (S3-19)
- Doc comments, `src/tools/decode.rs:74` claims its walk is the crate's only unsorted one (S3-11)
- Doc comments, `src/engine_globals.rs:1` lists `ui` as installed into every section VM (S4-2)
- Doc comments, `src/engine_globals.rs:302` narrates the other store path that `install_store_table` owns (S4-3)
- Doc comments, `src/engine_globals.rs:85` plus six siblings narrate closure mechanics and `mlua::Scope` (S4-4)
- Doc comments, `src/engine_globals-store.rs:1` still names the "shared library" the crate replaced with `require` (S4-1)
- Doc comments, `src/projection.rs:260` promises a concatenation its `unreachable!` arm cannot return (S4-13)
- Doc comments, `src/projection.rs:30` implies a per-model projection that does not exist (S4-14)
- Doc comments, `src/compactors.rs:12` states each policy's rule five times in one module (S4-16)
- Doc comments, `src/collection-order.rs:106` says refused keys have no position; two kinds do (S4-20)
- Doc comments, `src/collection.rs:97` omits `null`, which `render_item` renders as compact JSON (S4-23)
- Doc comments, `src/collection-order.rs:6` omits `json.encode` from the order's sharers (S4-18)
- Doc comments, `src/iteration.rs:27` and `:66` call the trailing segment non-scalar; it holds scalar keys too (S4-24, V4-2)
- Doc comments, `src/iteration.rs:255` repeats the module doc's resume limit nearly word for word (S4-25)
- Doc comments, `src/argv.rs:6` restates the frozen rule and malformed check across four places (S4-28)
- Doc comments, `src/vm.rs:183` says the VM retains no emitter, though its persistent globals do (S1-9)
- Doc comments, `src/vm.rs:84` groups `set_cancel` with the stage-gated calls (S1-10)
- Doc comments, `src/vm.rs:279` names two limits and omits the ceiling it records and the byte budget it resets (S1-8)
- Doc comments, `src/vm/state.rs:77` explains where the delegation lives (S1-13)
- Doc comments, `src/hardening.rs:23` says `coroutine` is never loaded; the shim prelude loads it (S1-3)
- Doc comments, `src/walk_budget.rs:21` says a limit "converts as it does today" (S1-24)
- Doc comments, `src/program.rs:40` calls `load` a "crate-internal step"; it is `pub` (S1-18)
- Doc comments, `src/sys.rs:143` and `src/prelude.rs:202` restate the whole `read_only_proxy` contract before calling it (S1-22)
- Doc comments, `src/vm/install.rs:132` scopes `max_tool_iterations` to the loop shim; handles spend it too (V1-2)
- Doc comments, `src/packs.rs:135` restates the Pack membership rule the module doc and `pack_tools` already state (S6-14)

**Documentation drift**

- Drift, 31 `# Errors` sections in 16 files say `Error::Lua` where the body builds `Error::LuaRuntime` (S1-1, S3-1, V3-1)
- Drift, `src/lib.rs:44` and five siblings say the first model call freezes the sets; only the first loop round does (S3-2)
- Drift, `../runvil-docs/src/language/05-lua-environment.md:183` setup order never places five Engine globals (S5-2)
- Drift, `crates/runvil-internal/lua/AGENTS.md:4` says `prepare_dispatch` applies counts; it has not since `be75abaa7` (S5-1)

**Report defects in the vibelight output**

- None. `ToolDispatch::trust` is absent from the outline because `#[cfg(test)]` gates it, per the Legend
- Observation: the Spellings warning counts 99 spellings, mostly `mlua::Table` beside `Table`, both idiomatic

**Rejected candidates**

- Rejected: 15. 11 weak, 2 intentional (S1-20, S4-21, the second a documented guide limit), and 2 moot (S3-21, settled by the `RosterEntry` commit, and V3-2, settled when HEAD returned `ToolSet::roster()` to `pub(crate)` behind a `test_support` accessor)
- Merged: 11 duplicates folded into the line of the cause they repeat (S6-2, S6-4, S6-6, S6-8 to S6-13, S6-15, S6-16)
- Merged: 6 same-cause findings folded into one line each (S3-1 and V3-1 into S1-1; S6-5 and S1-12 into S3-12; V4-2 into S4-24; S6-7 into S3-17)

</product-contract>
<implementation-contract>

## Technical Design

- **One rule for the `Error::Lua` mislabel.** A failure that passes through `Error::lua` or `Error::shared` is `Error::LuaRuntime`; only a directly built `Error::Lua(..)` is `Error::Lua`. Costs 29 doc edits, buys an end to the recurrence. Renaming the constructor to `lua_runtime` would prevent it structurally but touches every `map_err` in the crate, so it stays out.
- **Delete the instruction quota rather than keep it dormant.** `HOOK_BUDGET` is `u64::MAX`, the guide states three times that no instruction limit exists, and the engine pass deferred the variant to this one. Renaming `InstructionBudget` to `CancelHook` follows from that: the hook's only remaining job is the cancel poll. The firing counter goes too: once nothing trips, it only lets two tests observe that a cancelled firing is not counted, an ordering nothing else reads.
- **`Offers` and `ModelSelection`, not `SectionOffers`.** The engine already owns `execute::scope::SectionOffers`, so that name would collide at the crate seam. `Offers` is free in the workspace. Each type, its lock message, and its accessor name what the lock holds.
- **Narrow `prepare_dispatch` instead of documenting `None`.** The public entry point's `None` arm fires no event at all. Taking `ScriptReport` and keeping an `Option` body private makes the silent path unrepresentable, which is a smaller change than the doc fix it replaces.
- **Flatten `ModelReport` to `turn`.** It reads `script.turn` at its one site, and the field name asserts a path the call is not on.
- **Two phase variants for the compaction retry, not one field move.** The snapshot outlives the compacting flag by design: `step` restores it and `chat` retries holding it. Merging it into `Compacting` alone would drop the retry state.
- **Build the `sys` mirror at injection.** The `SectionVm<S>` typestate already guarantees injection before every reader, so the mirror can be created when `install_values` succeeds and the `Option` with its dead `None` arms goes.
- **One coroutine shim installer.** `install_coro_shims` installs `models.loop` and `models.compact` itself. `max_tool_iterations` stays on it, because a model handle's `loop` spends the cap whether or not `models.loop` exists.

</implementation-contract>
<verification-contract>

## Testing Plan

- Full suite: `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
- Linter: `cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`, with `CARGO_BUILD_WARNINGS=deny`
- Formatter: `cargo fmt --all --check`
- Docs: `cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`, with `RUSTDOCFLAGS="-D warnings"`
- Facade docs and surface, because work item 11 edits the doc of `VfsOp`, which the facade re-exports: `RUSTDOCFLAGS="-D warnings" cargo doc -p runvil --no-deps`, then `cargo +<pinned nightly> xtask api --check` with `CARGO_BUILD_WARNINGS` unset, where the nightly is the one `crates/build-xtask/src/api/toolchain.rs` names
- Wording scan, because work item 5 edits a rulebook: `node --test crates/workshop/ui/test/docs-claims.mjs`
- Engine private docs, as CI runs them, because several items edit engine comments: `RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`
- Per-item loop: `cargo nextest run --locked -p runvil-lua -p runvil-engine -p runvil-parser --all-features`, the full-suite command narrowed to the three crates this plan edits; the gates above run before each commit
- Docs reach: the docs gate resolves intra-doc links only in the docs it builds, which leave out `pub(crate)` and private items. `cargo doc --locked --no-deps --all-features -p runvil-lua --document-private-items`, the form CI runs for runvil-engine, also resolves the private `# Errors` links this plan edits; read its warnings for edited lines only, since it is not a CI gate
- No gate can tell `Error::Lua` from `Error::LuaRuntime` in a doc, because both links resolve; work item 1 is checked by reading each edited section against its body
- New test, `increment_seeds_a_missing_id_and_counts_it` in `src/scope-tests.rs`: increments an id on `ToolCallCounts::default()` that was never seeded and asserts `snapshot()` maps it to 1. Today `increment` returns the "was not pre-seeded" error, so the test's `expect` fails
- Extended test, `a_list_is_rendered_only_once_sets_are_attached_and_keeps_the_first` in `src/messages-tests.rs:65`: also asserts with `Arc::ptr_eq` that the second `attach_rendered_tools` returns the first attached sets. Today it does not compile, because the method returns `()`
- Changed test, `src/tests/logging.rs:172`: the expected refusal text is built from `LUA_LOG_CHARACTER_LIMIT`, as the input at `:163` already is
- Existing protection relied on: `tools/tests-offering.rs` for the offer-after-freeze rule, `projection-tests.rs:345` for `tool_calls` on a non-assistant role, `packs-tests.rs:100` and `tools/tests-stats.rs:108` for the proxy labels, `models_loop/machine-tests-compaction.rs:58` for the compacting flag, `tests/budgets.rs:6` for a loop past the old instruction budget, `dispatch-tests.rs` for both dispatch paths, and engine `execute/tests/models_loop-arguments.rs:117` for the loop's argument order

</verification-contract>
<decision-record>

## Decision Record

- Deleting `QuotaResource::Instruction` is a public enum change, but every user is in the workspace and the guide already documents the absence
- `Offers` replaces `ToolRuntime` and `ModelSelection` replaces `ModelRuntime`; the accessors follow, so `tool_runtime()` becomes `offers()` and `model_runtime()` becomes `model_selection()`
- `model_tool_name` keeps its name, since in this crate a tool's name is its wire name
- Eleven candidates repeated a cause another candidate raised; each folds into the surviving line, so the Debt Inventory cites one tag per cause
- The `RosterEntry` work is treated as landed: HEAD holds it, and the same series returned `ToolSet::roster()` to `pub(crate)` with `ToolSet::roster_for_test` in `test_support` for the one engine assertion
- Assumption: the guide fix in `../runvil-docs` is a second repository and needs its own commit
- Risk: HEAD advanced eight commits past this plan's baseline during review; if it moves again, reread the affected lines before editing them
- Operator decision: the `Answer` restructure lands in this pass, as work item 9, so the per-variant `Result` goes now rather than in a later pass
- Operator decision: the guide's setup-order fix lands as a second commit in `../runvil-docs`, as work item 27, in this same pass
- Consequence of the first decision: work item 9 runs after work item 8, since both edit `YieldParse` and `classify`
- Consequence of the second decision: work item 27 commits in a different repository, so it cannot share a commit or a rollback with the rest
- Review 2026-10-29: every lua-crate line citation checked landed on its symbol; the engine ones had drifted and now locate by symbol
- Review 2026-10-29: work item 9's site list was under-scoped. The engine holds 28 files naming `Answer` and 27 `Answer::*(Err(..))` sites; the item now says to finish by compiler error, not by list
- Review 2026-10-29: work item 10 would have left `TaskState::ok` as dead code and failed the `deny` lint gate; the item now deletes it
- Review 2026-10-29: work item 7 missed two test matches on `Phase::Chatting` as a unit variant and contradicted itself on `machine.rs:212`; both fixed
- Review 2026-10-29: work item 20 cannot give `ToolCallFields` a required turn, since batch entries carry none; the callers assemble `ModelCall` instead
- Review 2026-10-29: work item 22's move of `resume_block_coro` breaks on a private `step_block_coro`; it is now a `cfg` gate instead
- Review 2026-10-29: work item 21 folds the loop installer, so work item 27 renumbers the guide's step 7 away instead of keeping it
- Review 2026-10-29: `install_models` runs at injection (`vm/inject.rs:143`), before `install_coro_shims`, so the fold has the `models` table it needs
- Review 2026-10-29: `cargo xtask site --books-only` exists (`crates/build-xtask/src/main.rs`) but requires `RUNVIL_DOCS` to name the guide checkout; the item now sets it
- Review 2026-10-29: the shim table lives in `install_shim_prelude` (`coro.rs:275`), not `install_coro_shims`, so work item 21 places the loop install there
- Review 2026-10-29: work item 1 cannot blind-replace `Error::Lua`; 16 bodies build the variant directly and their docs are already correct
- Review 2026-10-29: work item 3's engine comment needs all four lines replaced, since its first half also goes false once `increment` seeds
- Review 2026-10-29: items 1, 3, and 8 named callers or sites that do not exist as described; each now names verified sites
- Decomposition 2026-10-29: work item 16 would have moved about 190 lines into the 402-line `engine_globals-store.rs`, past the 500-line build ceiling. `run_store_op` moves into that file, ending near 450 lines, and the read helpers move to a new private child module, `engine_globals-store-read.rs`, as `error-value.rs` holds `error-value-store.rs`
- Decomposition 2026-10-29: work item 10 repeated work item 7's `Phase::Chatting` test edits, which have nothing to do with `TaskPhase`; its step instead names the unit-variant match `TaskPhase::Done => "done"` in `protocol/answer.rs`, which breaks once `Done` takes a field

### Open Questions

None. Both questions were answered by the operator: the `Answer` restructure lands here, and the guide fix lands as a second commit.

### Decomposition constraint

- The operator requires a low step count: group the 27 work items into as few steps as shared files and shared causes allow, targeting 6 to 9 steps, never one step per work item
- Grouping rule: a step owns one commit, so every work item in it must touch overlapping files or share a cause; keep the required orderings intact (8 before 9, 20 before 21, 21 before 27)
- Do not split a step just because it spans two crates; the engine and the lua crate change together in most items

### Deferred and Out of Scope

- The engine's `bound_tool_set` carries the same misleading name as `bound_tools`; it belongs to `runvil-engine`
- `TOOL_LIST_FIXED`'s omission of `handle:loop` stays: the guide quotes the message verbatim and a test pins it
- Deferred: the `Compaction` value that would own the compacting flag end to end, revisit after work item 7 lands
- Deferred: renaming `Error::lua` to `Error::lua_runtime`, revisit if the mislabel recurs after this pass
- Noticed for the next scan: engine `execute/tests/happens_before.rs:9` cites "Functional Specification", a plan heading, in a test doc
- Noticed for the next scan: `iteration-tests.rs:245` says "A cleared non-scalar key"; check whether its key is scalar once work item 26 renames the trailing group "unordered"

</decision-record>
<project-survey>

## Project Survey

- Status: complete
- Build command: `cargo build --locked -p <crate>`. Plain `cargo build --locked` builds only the default member, the gateway app (package `gateway`); the desktop app is `cargo build --locked -p workshop`. `rust-toolchain.toml` pins Rust 1.99, and CI also builds the Engine crates at the 1.95 floor.
- Focused test command pattern: `cargo nextest run --locked -p <crate> --all-features -E 'test(<module path>)'`, for example `cargo nextest run --locked -p runvil-lua --all-features -E 'test(dispatch::tests)'` (verified with `cargo nextest list`; cargo-nextest 0.9.128 is installed).
- Component test command pattern: `cargo nextest run --locked -p <crate> --all-features`. The Workshop crates `workshop`, `workshop-server`, and `workshop-server-api` run without `--all-features`. A change in an internal Engine crate should also run the facade suite `cargo nextest run --locked -p runvil --all-features` and the structural checks `cargo test -p build-xtask`.
- Full-suite test command: `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`, then `cargo nextest run --locked -p workshop -p workshop-server -p workshop-server-api`, then the wording and rulebook scan `node --test crates/workshop/ui/test/docs-claims.mjs`.
- Linter command: with `CARGO_BUILD_WARNINGS=deny` (in PowerShell, `$env:CARGO_BUILD_WARNINGS = "deny"`), run `cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features` and `cargo clippy -p workshop -p workshop-server -p workshop-server-api --all-targets`, plus the headless shape check `cargo check -p gateway --no-default-features`. Never run a standalone `cargo check --workspace` beside clippy.
- Formatter check command: `cargo fmt --all --check` (`rustfmt.toml` sets `style_edition = "2024"`; the pre-commit hook runs the same check).
- Docs command: with `RUSTDOCFLAGS="-D warnings"`, run `cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api` and the default-features facade build `cargo doc -p runvil --no-deps`. The facade surface check is `cargo +nightly-2026-09-05 xtask api --check` with `CARGO_BUILD_WARNINGS` unset (`Remove-Item Env:CARGO_BUILD_WARNINGS`); it compares against `crates/runvil/public-api.txt`. CI also runs `cargo doc -p harness --no-deps` and `cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`.
- Test placement and naming conventions:
  - Unit tests live beside their module as `<stem>-tests.rs`, or `<stem>-tests-<topic>.rs` when split, wired at the bottom of the module as `#[cfg(test)] #[path = "<stem>-tests.rs"] mod tests;`. Test names are full-sentence snake_case behavior statements, such as `prepare_dispatch_wraps_a_canned_untrusted_output_and_reports_it`.
  - Once a test group reaches three files it becomes a subdirectory, such as `runvil-lua/src/tests/*.rs`, `src/protocol/tests/*.rs`, and `src/tools/tests-*.rs` beside `src/tools/tests.rs`.
  - `runvil-lua` has no `tests/` directory: all its tests are under `src`, with shared helpers in `test_support.rs` (also exposed to companion crates by the `test-support` feature) and `test_vm.rs`.
  - Integration tests are one binary per crate at `tests/suite/main.rs`, which declares one `mod` per topic, with helpers in `tests/suite/support.rs`. Prompt fixtures are Markdown files under `tests/prompts/valid/`, `tests/prompts/invalid/`, and `tests/prompts/execution/`.
  - Benches use criterion under `benches/`; `runvil-lua`'s `surface` bench needs `--features test-support`.
  - Engine tests never sleep, read the wall clock, wait with a timeout, or draw OS entropy. Pseudo-random input comes from a fixed seed that the failure prints, and temporary directory names come from the test's name, the process id, and a counter. `unwrap` and `expect` are allowed in tests only.
- Directory map:
  - `crates/`: every Rust crate plus the TypeScript UI packages. Flat crates are `runvil` (the Engine facade), `runvil-plugin` (the Plugin API), `harness`, `harness-gateway-client`, the Packs' Plugins `plugin-std`, `plugin-web`, `plugin-mcp`, and `plugin-user-input`, the gateway wire crates `gateway-api-types` and `gateway-api-discovery`, the leaves `shared-error-source` and `shared-loopback`, and the tooling crates `build-xtask` (structural checks and `cargo xtask api`), `build-ceiling` (the 500-line file cap), `build-ui`, `build-user-guide`, `build-workshop`, and `build-llama-cuda`.
  - Manifestless containers under `crates/` hold private family crates: `runvil-internal/` (`engine`, `lua`, `parser`, `types`, `vfs`, `model-client`), `harness-internal/` (`runner`, `plugin-loader`, `test-plugin`), `gateway/` (`app` is package `gateway`, plus `config`, `config-ui`, `local`, `logging`, `progress`, `protocol`, `routing`, `web-search`, `cloud-providers`, and `stt/` with `api`, `engine`, `backend-whisper`, and `whisper-ffi`), and `workshop/` (Rust crates `desktop`, `server`, `server-api`, `gateway`, `menu`, `protocol`, `registry`, `status`, `support`, `user-state`, `workspace`, `run-log`, and `agents`, plus the TypeScript packages `ui`, `look`, and `platform`). `crates/shared-ui` is a TypeScript and CSS package, not a crate.
  - `runvil-lua` source is `crates/runvil-internal/lua/src/`: flat modules with kebab siblings, subdirectories `json/`, `models_loop/`, `protocol/`, `tools/`, `vm/`, and `tests/`, and the embedded Lua shims `__impl_coro.lua`, `__impl_fanout.lua`, `__impl_globals.lua`, `__impl_require.lua`, `__impl_store.lua`, and `__impl_tasks.lua`.
  - `.vibelight/`: gitignored per-crate outlines in Markdown and `html/`, generated by `cargo vibelight`, which runs the sibling `../vibelight` checkout from the repo root.
  - `vibe/`: dated plan records; `vibe/ACTIVE` names the active one, currently `vibe/2026-10-29-1-lua-slop-cleanup.md`.
  - `guide/`: the mdbook user guide sources (`books`, `chrome`, `landing`). `prompts/`: example prompts. `tools/`: Node scripts with `.test.mjs` tests. `images/`: README art. `local/`: gitignored machine-local configs and fixtures.
  - `.github/workflows/`: `ci.yml` plus release, nightly, and site workflows. `.githooks/`: pre-commit runs fmt; pre-push runs the docs-claims scan, the headless gateway check, clippy, and `cargo deny` when installed. `.config/nextest.toml`: nextest profiles. `.cargo/config.toml`: the `xtask`, `workshop`, and `vibelight` aliases and the Windows `rust-lld` linker.
- Component boundaries:
  - Engine: `runvil` re-exports `runvil-engine`, which depends on `runvil-lua`, `runvil-parser`, `runvil-model-client`, `runvil-types`, and `runvil-vfs`. `runvil-parser` depends on `runvil-lua`, never the reverse, so functions that would create that cycle stay in `runvil-lua`. `runvil-lua` may depend only on `runvil-types`, `runvil-model-client`, and `runvil-vfs`; `runvil-model-client` depends on `runvil-types`; `runvil-types` and `runvil-vfs` are leaves. Engine crates never depend on `tokio`, `tokio-util`, `async-trait`, `reqwest`, `getrandom`, or `fastrand`, and they emit effects for the caller instead of performing work themselves.
  - Plugin API: `runvil-plugin` depends on `runvil-types` and `runvil-vfs`; each `plugin-*` crate depends only on `runvil-plugin`.
  - Harness: `harness` depends on `harness-runner`, `harness-plugin-loader`, `runvil`, and `runvil-plugin`. `harness-gateway-client` depends on `harness`, `runvil`, and `plugin-web`, and is the Harness-side client for the gateway.
  - Gateway: `gateway` sits on its family crates plus `gateway-api-types`, `gateway-api-discovery`, and `shared-loopback`. No gateway crate depends on any Engine, Harness, or Plugin crate.
  - Workshop: `workshop-server` is the hub, depending on the Harness, the Plugins, `runvil`, and the Workshop family crates. `workshop-server-api` wraps it, and the desktop app `workshop` depends on `workshop-server-api` and `gateway-api-discovery`. `workshop-protocol` and `workshop-support` are leaves, and `workshop-registry` sits on `workshop-protocol`.
  - `cargo test -p build-xtask` enforces the product and container boundaries, the Workshop tier graph, the `## Invariants` marker, and lint inheritance.
- Conventions summary:
  - Every crate sets `[lints] workspace = true`. Clippy `all` and `pedantic` are deny, as are `unwrap_used`, `expect_used`, `todo`, `exit`, `allow_attributes`, and `allow_attributes_without_reason`, so suppressions are written `#[expect(lint, reason = "...")]`. `missing_docs`, `missing_debug_implementations`, and `unreachable_pub` warn; `unsafe_code` is deny outside its owned boundary. The root `clippy.toml` disallows process-global installers such as `std::panic::set_hook` outside binary entry points, and `crates/runvil`, `crates/runvil-internal`, and `crates/runvil-plugin` carry their own `clippy.toml`.
  - Every crate's `build.rs` calls `build_ceiling::check()`, which fails the build when any `.rs` file under `src`, `tests`, `benches`, or `examples` exceeds 500 lines. An oversized file splits only into private child modules of that file.
  - Source directories are flat by default: one or two child files sit beside the parent as `<stem>-<topic>.rs` wired with `#[path]`, and three or more become a `<stem>/` subdirectory. Whichever conversion applies is done when touching the group.
  - Each crate root doc has a `## Invariants` section naming its allowed dependencies and rules, and many crates have a local `AGENTS.md`.
  - The seven defined terms (Engine, Harness, Host, Pack, Plugin, Static Set, Dynamic Set) are capitalized and used only in their defined sense. Engine crates call their driver "the caller" and never mention the Host. `docs-claims.mjs` enforces this wording.
  - Dependencies are declared once in the root `[workspace.dependencies]`, each version pin or feature choice justified by a comment, and members use `.workspace = true`.
  - Error and status messages are written for a model to read: concise, self-contained, and naming required versus actual.
  - Comments explain only a non-obvious constraint, ordering requirement, or workaround, and every workaround cites its upstream issue URL.
  - JSON that reaches a recorder or a replay comparison round-trips exactly: sorted keys, finite numbers, and `float_roundtrip` parsing.
  - Behavior changes ship with tests in the same change. Cargo features gate real constraints only, such as `test-support` and `gateway/test-fixtures`.

</project-survey>
<execution-plan>

## Execution Instructions

- Each step is one commit holding its code and its tests. Run the steps in number order.
- The operator's decomposition constraint groups the 27 work items into 8 steps, each owning one commit of work items that share files or a cause. Each step lists its work items, the numbers the decision record and the testing plan cite, and an Order line; inside a step, do the work items in that order, since a later item edits what an earlier one leaves. Every ordering the decision record states holds: work item 8 before 9 (both in step 4), 20 before 21 (both in step 5), and 21 (step 5) before 27 (step 8).
- The letters and numbers in parentheses after a work item are tags from the analysis; they trace a finding and are never needed to act.
- Each step's Tests are its commit gate, run once every work item in the step is done, written for PowerShell at the repository root `c:\Users\Vinnie\cursor\runvil` (step 8 says where it commits). Each command sets the environment variable it needs, and the private-docs command clears both variables before it runs. Each step's Loop is the fast check between work items.

**Components and pieces, in dependency order**

1. **Error surface** (step 1). First, because its rule - a failure built through `Error::lua` or `Error::shared` is `Error::LuaRuntime` - governs every `# Errors` line later steps touch; because it removes `QuotaResource::Instruction` from the protocol tests before step 4 rewrites them; and because its `CancelHook` doc rewrite removes the "direct chunk path" wording that step 5 counts as already gone.
   - Piece "error docs, quota, and refusal helper" (step 1, work items 1, 12, 2, and 18), sequential inside the step: work item 1 applies the variant rule across 16 files; work item 12 rewrites the `error` module docs and keeps the two cross-references work item 1 relies on; work item 2 deletes `mod lua_quota` from `src/error.rs`; and work item 18 then adds `author_refusal` there. All four edit the error surface and overlap in `error.rs`, `hardening.rs`, `globals.rs`, `messages.rs`, `coro.rs`, and the `vm` files.
2. **Tool surface** (steps 2-3). Second, because it renames `SectionVm` fields and accessors (`offers`, `model_selection`, `tool_set`, `store_conflict`, `current_static_set`), moves the `handles` and `sys` items into `bindings`, `section`, and `var`, and narrows the public dispatch entry point. Every later component edits files that name them, so landing them first means later steps write final names and import paths.
   - Piece "offers and frozen sets" (step 2, work items 3, 4, 13, and 6), sequential inside the step: work item 3 changes the counts API and the offer types and leaves the `scope` module doc to work item 4, which states where the sets freeze; work item 13 then renames more fields in the same `vm` files; and work item 6 changes the method that attaches the frozen sets work item 4 documents.
   - Piece "module layout and dispatch path" (step 3, work items 14, 15, and 5), after step 2 so the split moves items under their final names, and sequential inside the step: work item 14 splits `handles` and `sys`; work item 15 edits `seal_sys` in the slimmed `sys.rs` and builds its proxy among final imports; and work item 5 then rewrites `dispatch.rs` and `tools/tests-parameters.rs`, whose imports work item 14 just moved. The piece lands before step 4 because work items 5 and 9 both edit engine `execute/scheduler/apply.rs`, and the smaller change lands first.
3. **Protocol types** (step 4). Before the VM component, because work item 9's `Answer` change sets the signature of `resume_block_coro_answer`, which step 5 retargets doc links to, and work item 11's `request.rs` docs land before step 5 reshapes `Request::ToolCall` in the same file.
   - Piece "parse, answer, and protocol docs" (step 4, work items 8, 9, 10, and 11), sequential inside the step: work item 9 needs work item 8's `YieldParse::Refused`; work item 10 reshapes `TaskStatus` in the `answer.rs` and `render.rs` work item 9 just rewrote; and work item 11 documents `CHAT_PART_TYPES` where work item 8 moved it and the `Answer` work item 9 left.
4. **VM setup and models loop** (steps 5-6). After protocol: work item 22 needs work item 2's hardening rewrite and work item 9's resume signature, and work item 20 edits `protocol/request.rs` after work item 11.
   - Piece "VM setup" (step 5, work items 20, 21, 22, 17, and 23), sequential inside the step: work items 20, 21, and 22 all edit `coro.rs`, and work item 21 needs work item 20's stash edits; work item 22's `coro.rs` doc edit sits beside the loop-shim mention work item 21 deletes; work item 17 changes `SectionVm`'s fields and `Injected` in the `vm/inject.rs` and `vm/install.rs` that work items 21 and 22 edit; and work item 23 then rewrites the `vm.rs` docs around them.
   - Piece "models loop" (step 6, work items 7 and 19), after step 5 because work item 19's `install_models` doc names the `models.loop` and `models.compact` shims that work item 21 now installs; inside the step, work item 7 changes the loop machine, and work item 19 then rewrites `loop_begin`'s doc over the closure work item 7 leaves.
5. **Store and module docs** (step 7). Independent of the components above, so it runs last among the Rust steps, where its `vm/install.rs` doc edits land on settled installers and its closing outline check sees every rename.
   - Piece "placement and vestiges" (step 7, work items 16, 24, 25, and 26), in number order. They share a cause rather than files: each moves an item, or the one statement of a rule, to the module that owns it, or drops an attribute or derive nothing needs. Work item 26 carries the closing outline check, so it goes last.
6. **Guide setup order** (step 8). Last, because it describes the merged installer step 5 builds, and it commits in the second repository `c:\Users\Vinnie\cursor\runvil-docs`, so it shares no commit or rollback with the rest.
   - Piece "guide" (step 8, work item 27): one step.

<step-1>

### Step 1: the error surface: the `Error::Lua` doc rule, the error docs, the instruction quota, and one refusal helper [completed]

- Component: Error surface
- Tests:
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `$env:CARGO_BUILD_WARNINGS = "deny"; cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `cargo fmt --all --check`
  - `$env:RUSTDOCFLAGS = "-D warnings"; cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`
  - `$env:RUSTDOCFLAGS = "-D warnings"; cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`
  - `Remove-Item Env:RUSTDOCFLAGS, Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue; cargo doc --locked --no-deps --all-features -p runvil-lua --document-private-items`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`

- Work items: 1, 12, 2, and 18, in one commit.
- Order: work item 1, then 12, then 2, then 18. Work item 12 keeps the cross-references work item 1 relies on; work item 2 deletes `mod lua_quota` from `src/error.rs` before work item 18 adds `author_refusal` there; and work items 2 and 18 keep the `# Errors` text work item 1 sets.
- Paths: lua paths are under `crates/runvil-internal/lua/`, paths marked "engine" are under `crates/runvil-internal/engine/src/`, and commands run from `c:\Users\Vinnie\cursor\runvil`. Line numbers were read at `7a5341bba`, and earlier steps and earlier work items in this step shift them, so every line number is a hint: locate each site by symbol or quoted text, and where a line and a symbol disagree, the symbol wins.
- Constraint: no author-visible behavior change. Every Lua global, table shape, and message stays as the guide documents it, and every type or signature change stays inside the workspace. Add no dependency and no crate. Every `.rs` file stays at or under 500 lines, or `build_ceiling::check()` fails the build.
- Acceptance: the Tests pass, plus each work item's acceptance below. The `runvil-lua --document-private-items` build is not a CI gate and passes on exit status; read its warnings for the lines this step edited, and treat a warning on an edited line as a failure.
- Loop: between work items, `cargo nextest run --locked -p runvil-lua -p runvil-engine -p runvil-parser --all-features`, and for the docs-only work items 1 and 12, the workspace docs command in Tests; the Tests above gate the commit.

**Work item 1 (S1-1, S3-1, V3-1): the `Error::Lua` doc rule, 29 `# Errors` sections**

- Files: `src/coro.rs`, `src/hardening.rs`, `src/vm/install.rs`, `src/vm/state.rs`, `src/sys.rs`, `src/globals.rs`, `src/prose.rs`, `src/messages.rs`, `src/compactors.rs`, `src/engine_globals.rs`, `src/engine_globals-store.rs`, `src/iteration.rs`, `src/collection.rs`, `src/models.rs`, `src/models_loop.rs`, `src/tools/objects.rs`
- Scope: docs only.
- Problem: these `# Errors` sections promise `Error::Lua` for failures their bodies build through `Error::lua` or `Error::shared` (`src/error.rs:222-240`), both of which build `Error::LuaRuntime`. Rust code that matches the documented variant misses the real one, and `start_block_coro` (`src/vm/run.rs:66-69`) already documents as `Error::LuaRuntime` the same failure `block_guard` documents as `Error::Lua`. Rustdoc resolves both links, so no check catches it.
- Rule: a failure that passes through `Error::lua` or `Error::shared` is `Error::LuaRuntime`; only a directly built `Error::Lua(..)` is `Error::Lua`. Renaming the constructor to `lua_runtime` is deferred and out of scope.
- Change: replace "[`Error::Lua`]" with "[`Error::LuaRuntime`]" and keep the rest of the sentence in these 26 sections: `install_shim_prelude` (`coro.rs:168`), `block_guard` (`coro.rs:333`), `install_store_shims` (`coro.rs:380`), `InstructionBudget::install_on_thread` (`hardening.rs:122`), `install_instruction_budget` (`hardening.rs:190`), `SectionVm::install_lazy_prose` (`vm/install.rs:292`), `globals::install` (`globals.rs:73`), `freeze_argv` (`globals.rs:108`), `replace_prose` (`globals.rs:130`), `globals::install_messages` (`globals.rs:143`), `globals::messages` (`globals.rs:152`, the first clause only, since its `Error::Internal` clause is right), `prose::install` (`prose.rs:71`), `prose::swap` (`prose.rs:88`), `prose::restore` (`prose.rs:101`), `lazy_read` (`prose.rs:110`), `messages::install_messages` (`messages.rs:63`), `install_compactors` (`compactors.rs:302`), `install_ui` (`engine_globals.rs:149`), `route_store_to_shims` (`engine_globals-store.rs:365`), `install_deterministic_iteration` (`iteration.rs:49`), `render_item_value` (`collection.rs:118`), `install_models` (`models.rs:248`), `loop_begin` (`models_loop.rs:87`), `compact_begin` (`models_loop.rs:209`), `install_tool_objects` (`tools/objects.rs:38`), and `install_tool_get` (`tools/objects.rs:78`). Work item 2 renames the two `hardening.rs` items later in this step; edit their sections under the old names first.
- Change: `SectionVm::argv_json` (`vm/state.rs:42-43`) becomes "Returns [`Error::Lua`] when H1 left `argv` as a function, userdata, or thread, and [`Error::LuaRuntime`] when its value cannot be represented as JSON."
- Change: `var_snapshot_table` (`sys.rs:245`) becomes "Returns [`Error::Lua`] if the author reassigned the `var` global, [`Error::LuaRuntime`] if a registry slot cannot be read (Engine values were never injected) or a nested proxy cannot be copied, and [`Error::Internal`] if the root data value is not a table."
- Change: `var_to_json` (`sys.rs:277-280`) becomes "Fails as [`var_snapshot_table`] does, and with [`Error::LuaRuntime`] if the data cannot be represented as JSON."
- Change: skip `install_section_loop_shim` (`coro.rs:345`) and `globals::install_prose` (`globals.rs:120`), which step 5 deletes (work items 21 and 17). Do not blind-replace `Error::Lua` with `Error::LuaRuntime`: some bodies build the variant directly, and those docs are already right. Confirmed direct builders include `alias.rs:30`, `collection.rs:33` and `:60`, `prelude.rs:163` and `:197`, `projection.rs:64`, `:81`, `:87`, `:95`, `:121`, and `:143`, `prose.rs:137` and `:145`, `sys.rs:254`, `protocol/parse.rs:42`, `vm/run.rs:264`, `compactors.rs:120`, and `SectionVm::var` (`vm/state.rs:30`). For each site, read the body it documents and pick the variant the body actually builds. Keep the cross-references at `error.rs:92` and `:136`, the one place a reader learns that `Error::lua` builds `LuaRuntime`.
- Ceiling: `src/messages.rs` holds 499 lines and the 500-line build ceiling fails at 501, so edit its sentence in place without rewrapping it onto a new line.
- Acceptance: no gate can tell `Error::Lua` from `Error::LuaRuntime` in a doc, because both links resolve and no test pins these variants, so read each edited section against the body it documents.

**Work item 12 (S2-1, S2-2, V1-1, S2-3, S2-6, S2-7, S2-9): the error module docs**

- Files: `src/error.rs`, `src/error-value.rs`, `src/error-value-store.rs`
- Scope: docs only.
- Problem: the module doc names a "core" crate that does not exist; `SharedSource` and `Error::shared` cite a resolver cache nothing has and list the callers without the `require` loader; the `Error` type doc gives a span list that misses six variants and repeats the module doc; `ErrorKind::Cancelled` and `ErrorKind::ContextExhausted` are documented narrower than what maps to them; `Raised` says its fields are strings; and the store error field list is written out twice and already differs.
- Change: in the `error` module doc (`error.rs:5`), "wraps and classifies core's own error type, which maps this one back" becomes "wraps and classifies `runvil-engine`'s internal error type, which maps this one back".
- Change: the first paragraph of the `SharedSource` doc. Line 24 is the one-line summary, and the "Some caches..." paragraph is `:26-30`; replace `:26-30` and keep the summary. New text: "Some compiled-program statics re-produce a typed `Error` on every lookup, so a non-`Clone` dependency error cannot be moved into a fresh `Error` each time. Wrapping it in a reference-counted `SharedSource` keeps the typed cause and clones it cheaply per lookup instead of flattening it to a string." Its caller sentence (`error.rs:30-33`) becomes "Every embedded chunk's compiled-program static is a caller, through `detail::shared_source_new`." `Error::shared` (`error.rs:231-234`) becomes "Re-produces a typed `Error` from a `SharedSource` a compiled-program static captured once, cloning the `Arc` rather than flattening the cause to a string."
- Change: the `Error` type doc (`error.rs:70-74`) becomes "The crate's internal error type: every failure a section VM's setup, Lua code, or Engine calls can end in." Delete its second paragraph, which the module doc keeps.
- Change: `ErrorKind::Cancelled` (`error-value.rs:74`) becomes "The run was cancelled, the caller dropped a call the run was waiting on, or a wait delivered a task its owner cancelled (that value includes `task`)." `ErrorKind::ContextExhausted` (`error-value.rs:51-52`) becomes "A request overflowed the model's context window and the selected compactor did not compact, or a caller raised it through `compactors.fail` with its own reason; includes `reason`."
- Change: the `Raised` doc (`error-value.rs:276-277`) says "the kind's fields" in place of "the kind's string fields"; `ErrorField::Integer` (`error-value.rs:197-198`) becomes "An integer-valued field (a store anchor's `count`, a UTF-8 failure's `valid_up_to`)."; and the `fields` doc (`error-value.rs:290-291`) says "string- or integer-valued (a store anchor's `count`, a UTF-8 failure's `valid_up_to`)".
- Change: `store_error_value_fields` (`error-value-store.rs:61-68`) keeps the one full list, written as the detailed one: `anchor` and its integer `count`, `rule` as the `PathReason` tag, and the integer `valid_up_to`. The private `store_error_fields` (`error-value-store.rs:6-11`) becomes "Returns the variant's own fields, without `reason`, in the shape `store_error_value_fields` documents."
- Keep: the cross-references at `error.rs:92` and `:136` that tell a reader `Error::lua` builds `LuaRuntime`; work item 1 relies on them.
- Acceptance: the docs commands pass.

**Work item 2 (S1-2, S2-5, S1-3): delete the instruction quota and rename the hook**

- Files: `src/hardening.rs`, `src/error.rs`, `src/error-tests.rs`, `src/vm.rs`, `src/vm/install.rs`, `src/vm/run.rs`, `src/vm/state.rs`, `src/vm/inject.rs`, `src/coro.rs`, `src/models_loop.rs`, `src/models_loop/tests.rs`, `src/tests.rs`, `src/tests/cancellation.rs`, `src/tests/budgets.rs`, `src/protocol/tests/answer.rs`, `src/protocol/tests/answer_retained.rs`, and engine `lua/tests/errors.rs`
- Problem: `HOOK_BUDGET` is `u64::MAX` (`hardening.rs:21`), so the trip in `budget_hook` (`:175-179`) needs about 1.8e23 instructions. The guide says three times that no instruction limit exists (`../runvil-docs/src/language/16-limits-and-errors.md:208`, `:242`, `:244`), and the engine's cleanup pass deferred `QuotaResource::Instruction` to this one. The hook's names and docs still describe a budget, `error::lua_quota` (`error.rs:210-219`) is a module of message strings with one reader, and the `harden` doc says `coroutine` is never loaded although `install_shim_prelude` loads it (`coro.rs:177`) and strips the global again (`coro.rs:276-278`).
- Design: delete the quota rather than keep it dormant. The hook's only remaining job is the cancel poll, so `InstructionBudget` becomes `CancelHook`, and the firing counter goes too: once nothing trips, it only lets two tests observe that a cancelled firing is not counted. `QuotaResource` is public, but every user is in the workspace and the facade does not re-export it.
- Change: delete `HOOK_BUDGET`, the trip branch in `budget_hook`, and `QuotaResource::Instruction` (`error.rs:56-57`) with its `Display` arm (`error.rs:65`).
- Change: write the two remaining strings into the `QuotaExceeded` `Display` match (`error.rs:200-204`), `QuotaResource::LogEvent => "lua log event budget exceeded"` and `QuotaResource::LogByte => "lua log cumulative byte budget exceeded"`, and delete `mod lua_quota` with its doc comments.
- Change: delete the `fired` counter, `InstructionBudget::fired()` (`hardening.rs:145-150`), and `SectionVm::hook_firings()` (`vm/state.rs:119-123`); `budget_hook` loses its `fired` parameter, and the `AtomicU64` and `Ordering` imports go if nothing else uses them.
- Change: rename `InstructionBudget` to `CancelHook`, the `SectionVm` field `instruction_budget` to `cancel_hook`, `install_instruction_budget` to `install_cancel_hook`, and `budget_hook` to `cancel_poll`. The 36 lines that name them are in `coro.rs` (6), `hardening.rs` (5), `models_loop.rs` (6), `models_loop/tests.rs` (4), `vm.rs` (6), `vm/run.rs` (2), `vm/inject.rs` (2), and one each in `vm/state.rs`, `vm/install.rs`, `tests.rs`, `tests/budgets.rs`, and `tests/cancellation.rs`.
- Change: `hardening.rs:1-2` becomes "Sandbox hardening for section VMs: the `_G` guard, global removal, and the cancellation hook." The `CancelHook` doc (`hardening.rs:97-104`) becomes "A VM's shared cancellation-hook state. The hook polls the run's cancel flag every [`HOOK_INTERVAL`] instructions, so a long-running or infinite loop is legal and only the flag aborts it. The flag is the run's synchronous [`CancelHandle`], installed once by the executor through [`set_cancel`](Self::set_cancel); a VM no executor claimed (a test fixture) is never cancelled." Its per-coroutine paragraph (`:106-110`) stays, with "(over the same counter)" changed to "(over the same flag)". This rewrite also removes "the direct chunk path" at `hardening.rs:104`, the eleventh "scheduler mode" site that work item 22 in step 5 counts as done here.
- Change: `install_on_thread` (`hardening.rs:119`) becomes "Installs the cancellation hook on a block coroutine." `cancel_poll` (`:159-161`) becomes "Builds the every-Nth-instruction hook body shared by the main state and every block coroutine: the cancellation poll." `install_cancel_hook` (`:187`) says "with the returned hook" in place of "with the returned counter". Keep each `# Errors` section work item 1 edited.
- Change: `error.rs:123-124` begins "An Engine quota on Lua (log events or log bytes) was exhausted."; the `cancel_hook` field doc (`vm.rs:159-160`) becomes "The VM's cancellation hook, shared with every block coroutine's hook (hooks are per-coroutine in PUC Lua)."; `vm/run.rs:44` says "the VM's cancellation hook" in place of "the VM's budget/cancellation hook".
- Change: the `harden` doc's second sentence (`hardening.rs:24-25`) becomes "The `io`, `os`, `package`, and `debug` libraries are never loaded; the coroutine library loads only while the shim prelude installs, and its global is stripped again." Its `table.concat` paragraph (`:30-33`) ends after "coerces like `tostring`" with ". Plain tables, booleans, and nil still raise as stock Lua does."
- Change: in `tests/cancellation.rs`, delete each `let fired = ...` line and the `assert_eq!` on `budget.fired()` (`:34-41`) and on `vm.hook_firings()` (`:68-75`); keep the `is_cancelled`, "lua execution cancelled", and `Interrupted` assertions. Its module doc (`:1`) says "the cancellation hook", and the comments at `:11-15` and `:59-62` drop "before counting itself".
- Change: the samples of `QuotaResource::Instruction` at `protocol/tests/answer.rs:57`, `:63`, `:85`, `:92`, `protocol/tests/answer_retained.rs:12`, `:48`, and engine `lua/tests/errors.rs:265`, `:269` become `QuotaResource::LogEvent`, with any expected text they compare following that variant ("lua log event budget exceeded"). Delete the `error-tests.rs:80` row and the `"lua instruction budget exceeded"` entry at `tests/budgets.rs:52`.
- Acceptance: `tests/budgets.rs` still shows a loop past the old budget completing (`:6-17`) and author raises of the two remaining quota texts staying ordinary runtime errors (`:45`); `tests/cancellation.rs` keeps its cancellation assertions. The exhaustive matches on `QuotaResource` (`error.rs:62-66`, `:200-204`) flag every arm left behind. The two log quota messages read exactly as before.

**Work item 18 (S4-8, S4-27, S4-26): `set_messages`, one author refusal helper, and the encoder's prefix**

- Files: `src/globals.rs`, `src/messages.rs`, `src/messages-view.rs`, `src/json.rs`, `src/json/encode.rs`, `src/json/decode.rs`, `src/error.rs`
- Problem: two functions named `install_messages` sit at two layers: `messages::install_messages` (`messages.rs:60-68`) builds the userdata and calls `globals::install_messages` (`globals.rs:139-147`), which only writes the guard's slot beside `freeze_argv`, `replace_prose`, and the getter `messages`. The author refusal helper, `fn refusal(message: String) -> mlua::Error` returning `mlua::Error::external(Error::Lua(message))`, is defined in both `json.rs:87-91` and `messages.rs:280-284`. `Encoder.function` (`json/encode.rs:162`) holds the message prefix `to_json` takes (`:44-51`), such as `PARAMETERS_NOT_JSON` (`tools.rs:66`), not a function name.
- Change: rename `globals::install_messages` to `set_messages`; its one caller is `messages.rs:67`. Keep the `# Errors` text work item 1 set.
- Change: add `pub(crate) fn author_refusal(message: String) -> mlua::Error` as a free function in `src/error.rs`, not an associated one, documented "An author error raised across the Lua boundary: an external [`Error::Lua`], which `pcall` reports as kind `lua`." Delete both module copies and point the 25 `refusal(` calls in `json.rs`, `json/encode.rs`, `json/decode.rs`, `messages.rs`, and `messages-view.rs` at `crate::error::author_refusal`. Leave `globals::refusal` (`globals.rs:172`), an unrelated `(lua, &'static str) -> Result<Function>`.
- Change: rename `Encoder.function`, `Encoder::new`'s parameter (`json/encode.rs:172`), and the free `too_large`'s parameter (`:113-115`) to `prefix`; leave `debug_option`'s `function`, which always is one. `Failure`'s doc (`:117-118`) becomes "Why a value cannot be encoded, before the prefix and path are added."
- Ceiling: `src/messages.rs` holds 499 lines; this work item deletes its `refusal` copy, so it shrinks. `src/json/encode.rs` holds 477 lines; the renames add none.
- Acceptance: `messages-tests-list.rs:16-27` still downcasts the raise to `Error::Lua`, the json tests still read kind `lua`, and `tools/tests-adhoc.rs` still pins the `PARAMETERS_NOT_JSON` message.

</step-1>

<step-2>

### Step 2: the counts API, `Offers` and `ModelSelection`, the tool names, and where the sets freeze [completed]

- Component: Tool surface
- Tests:
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `$env:CARGO_BUILD_WARNINGS = "deny"; cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `cargo fmt --all --check`
  - `$env:RUSTDOCFLAGS = "-D warnings"; cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`
  - `$env:RUSTDOCFLAGS = "-D warnings"; cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`
  - `Remove-Item Env:RUSTDOCFLAGS, Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue; cargo doc --locked --no-deps --all-features -p runvil-lua --document-private-items`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`

- Work items: 3, 4, 13, and 6, in one commit.
- Order: work item 3, then 4, then 13, then 6. Work item 3 changes the counts API and the offer types and leaves the `scope` module doc to work item 4; work item 13 renames more fields in the `vm` files work item 3 just edited; and work item 6 changes the attach method whose freeze point work item 4 documents. All four overlap in `scope.rs`, `vm.rs`, `vm/state.rs`, `vm/inject.rs`, `lib.rs`, `tools.rs`, `messages.rs`, `test_support.rs`, and engine `execute/scope.rs`, `execute/scheduler/tool_call.rs`, and `execute/scheduler/chat.rs`.
- Paths: lua paths are under `crates/runvil-internal/lua/`, paths marked "engine" are under `crates/runvil-internal/engine/src/`, and commands run from `c:\Users\Vinnie\cursor\runvil`. Line numbers were read at `7a5341bba`, and earlier steps and earlier work items in this step shift them, so every line number is a hint: locate each site by symbol or quoted text, and where a line and a symbol disagree, the symbol wins.
- Constraint: no author-visible behavior change. Every Lua global, table shape, and message stays as the guide documents it, and every type or signature change stays inside the workspace. Add no dependency and no crate. Every `.rs` file stays at or under 500 lines, or `build_ceiling::check()` fails the build.
- Acceptance: the Tests pass, including the new test `increment_seeds_a_missing_id_and_counts_it` and the extended test `a_list_is_rendered_only_once_sets_are_attached_and_keeps_the_first`, plus each work item's acceptance below. The `runvil-lua --document-private-items` build is not a CI gate and passes on exit status; read its warnings for the lines this step edited, and treat a warning on an edited line as a failure.
- Loop: between work items, `cargo nextest run --locked -p runvil-lua -p runvil-engine -p runvil-parser --all-features`; the Tests above gate the commit.

**Work item 3 (S3-23, S6-3, S3-12, S6-5, S1-12, S3-13): the counts API, `Offers`, and `ModelSelection`**

- Files: `src/scope.rs`, `src/scope-tests.rs`, `src/lib.rs`, `src/vm.rs`, `src/vm/state.rs`, `src/vm/inject.rs`, `src/tools.rs`, `src/tools/offer.rs`, `src/models.rs`, `src/models-tests.rs`, `src/test_support.rs`, `src/test_vm.rs`, `src/protocol/tests/answer.rs`, `src/tools/tests-adhoc.rs`, `src/tools/tests-stats.rs`, and engine `execute/scope.rs`, `execute/scheduler/tool_call.rs`, `execute/scheduler/tool_call-adhoc.rs`, `execute/scheduler/tool_call-discovery.rs`, `execute/scheduler/dispatch.rs`, `execute/scheduler/chat.rs`, `execute/section_context.rs`, `execute/tests/tool_scoping.rs`, `error/tests.rs`
- Problem: `ToolCallCounts::increment` (`scope.rs:54-68`) errors on an id that was never seeded, so every dispatch site calls `ensure` first, taking the lock twice, and the error arm cannot run. Its API calls the key a name, though every caller passes a tool id and "name" is this crate's word for a wire name. Beside it, `ToolRuntime` (`scope.rs:139-165`) holds only `offers`, its field doc says "tool-addition runtime" (`vm.rs:127`), and its locks say "tool declaration runtime"; `ModelRuntime` (`models.rs:197-219`) holds only a selection, `used`, under "model declaration runtime". `pub offers` (`scope.rs:143`) lets outside code break "each held once", which only `add` keeps, and nothing outside the crate reads it.
- Design: `Offers` and `ModelSelection`, not `SectionOffers`, because the engine already owns `execute::scope::SectionOffers` and that name would collide at the crate seam; `Offers` is free in the workspace. Each type, its lock message, and its accessor name what the lock holds.
- Change: write the failing test first in `src/scope-tests.rs`: `increment_seeds_a_missing_id_and_counts_it` calls `increment` on `ToolCallCounts::default()` for an id never seeded and asserts `snapshot()` maps it to 1. Today `increment` returns the "was not pre-seeded" error, so the test's `expect` fails.
- Change: `increment` becomes `*map.entry(id.to_owned()).or_insert(0) += 1` under one lock, and its `# Errors` keeps only the poisoned mutex. The type doc's second sentence (`scope.rs:16-17`) becomes "`increment` seeds a missing id at 0, and `ensure` pre-seeds one so `tools.stats` lists it before any call." Drop the `Cow` import if nothing else uses it.
- Change: delete the `ensure` before each `increment` at engine `execute/scheduler/tool_call.rs:347`, `tool_call-discovery.rs:118`, `tool_call-adhoc.rs:46`, and in the test helpers at `tools/tests-adhoc.rs:348` and `tools/tests-stats.rs:51`. Replace the whole four-line comment above engine `tool_call.rs:347` with "The attempt counts at dispatch - before the tool runs, so a cancelled dispatch still counts." The current comment's first two lines say a tool outside the section's sets "must still be seeded here, because the increment errors on an unseeded id", and both halves go false once `increment` seeds. Keep `ensure` at engine `execute/section_context.rs:249`, which seeds the visible zeros.
- Change: rename the parameters `names` to `ids` and `name` to `id` in `new`, `ensure`, `increment`, and `snapshot`, whose docs become "Creates a counts map pre-seeded with 0 for every id.", "Ensures `id` is present in the map, seeding it at 0 when new.", "Increments the count for `id`, seeding it at 0 first when new.", and "Returns every seeded id with its count, read under one lock." Engine `error/tests.rs:290` copies the deleted "was not pre-seeded" message to test owned-message passing; give it any other owned message. Keep the user-facing "seeded names" refusal at `tools/stats.rs:108`, which the guide quotes.
- Change: rename `ToolRuntime` to `Offers`, the `SectionVm` field `tool_runtime` and its accessor to `offers` and `offers()`, `ModelRuntime` to `ModelSelection` with its field `used` renamed `selection`, and the field `model_runtime` and its accessor to `model_selection` and `model_selection()`. Sites: `scope.rs:141`, `:146`; `models.rs:201`, `:205`, `:207`, `:254`; `lib.rs:132`, `:144`; `vm.rs:40`, `:42`, `:128`, `:130`, `:212-213`, `:326-327`, `:338-339`; `vm/state.rs:17`, `:20`, `:142`, `:163`, `:190`, `:211`, `:262`; `vm/inject.rs:139`, `:147`, `:160-161`, `:181-182`; `tools.rs:36`, `:115`; `tools/offer.rs:25`, `:35`; `test_support.rs:20`, `:23`, `:199`, `:202`, `:212`, `:218`; `test_vm.rs:176`; `models-tests.rs:8`, `:19`, `:27`, `:41`, `:109`, `:139`, `:220`, `:291`, `:414`, `:425`; `protocol/tests/answer.rs:171`. In the engine, find each site by name instead: `execute/scope.rs` (`vm.tool_runtime()`), `execute/scheduler/tool_call.rs`, `execute/scheduler/dispatch.rs`, `execute/scheduler/chat.rs`, `execute/section_context.rs` (`model_runtime()`), and `execute/tests/tool_scoping.rs`. In the `vm/state.rs` docs at `:126-127`, `:138`, `:155`, `:159`, `:181`, and `:186`, "tool runtime" becomes "offers".
- Change: the four poison messages become "section offers lock was poisoned" (`tools/offer.rs:62`, `vm/state.rs:217`) and "model selection lock was poisoned" (`models.rs:279`, `vm/state.rs:267`).
- Change: the field doc at `vm.rs:127` becomes "The section's offers, read by the executor's scope path."; `tool_bag_handles` (`test_support.rs:197`) becomes "Returns the shared tool set and the live section offers."
- Change: `pub offers` becomes `pub(crate) offers` (`scope.rs:143`); the type stays `pub`. Leave the module doc at `scope.rs:1-3` to work item 4, next in this step.
- Acceptance: `increment_seeds_a_missing_id_and_counts_it` passes. `tools/tests-stats.rs`, `tools/tests-adhoc.rs`, engine `execute/tests/debug_and_counts.rs:153`, `:167`, `:215`, and `execute/tests/discovery.rs:399` still pin exact counts, and `models-tests.rs:24-41` still pins the selection. The "seeded names" refusal text at `tools/stats.rs:108` is unchanged.

**Work item 4 (S3-2): the first loop round freezes the sets, six docs**

- Files: `src/lib.rs`, `src/tools.rs`, `src/scope.rs`, `src/messages.rs`, `src/vm/state.rs`
- Scope: docs only.
- Problem: six docs say the section's first model call freezes its sets. Only a loop round attaches them (engine `execute/scheduler/chat.rs:116-142`, `:230`); `models.infer` sends no tools and freezes nothing (engine `execute/scheduler/dispatch.rs:206-231`), as the guide (`12-tools.md:372`, `:913`, `:1172`) and the repository AGENTS.md's Static Set definition say. A reader orders offers before an `infer` for no reason, or expects an offer after one to raise.
- Change: `lib.rs:44-46` becomes "`tools.offer` and `tools.offer_dynamic` remember tools and Packs, by id or object, for the section's Static Set and Dynamic Set, which the section's first loop round freezes; after that, every offer raises. `models.infer` freezes nothing."
- Change: in `tools.rs:16-18`, the sentence ending "froze its sets." becomes "Offering a tool the run does not hold is a hard error, and so is every offer once the section's first loop round froze its sets."
- Change: `scope.rs:1-3` becomes "Per-VM tool state: call counts, the offers a section made, the frozen sets its first loop round built from them, and the Engine's two discovery tools, which join the Static Set beside a Dynamic Set." The first sentence at `scope.rs:167` becomes "The two sets a section entry froze at its first loop round (`models.loop` or a handle's `loop`): the one record of what its model was shown."
- Change: `messages.rs:25-27`, through "exactly as long as the section.", becomes "The list also holds the section's frozen sets, which the Engine attaches at the section's first loop round, so they last exactly as long as the section."
- Change: `vm/state.rs:132` becomes "The section's first loop round freezes its Static Set from these tools."
- Keep: `TOOL_LIST_FIXED` (`tools.rs:60-62`) as it is; the guide quotes it verbatim (`12-tools.md:920`, `:1020`) and `tools/tests-offering.rs:234` pins it.
- Ceiling: `src/messages.rs` sits at the 500-line build ceiling, which fails at 501; the replacement text must fit in no more lines than the three it replaces.
- Acceptance: the docs commands pass, and `tools/tests-offering.rs:234` still pins `TOOL_LIST_FIXED`.

**Work item 13 (S3-8, S3-9, S1-16, S1-11, S3-19, S3-11): the tool-set, tool-id, static-set, and store-conflict names, and two `tools` docs**

- Files: `src/vm.rs`, `src/vm/state.rs`, `src/vm/inject.rs`, `src/vm/install.rs`, `src/tools.rs`, `src/tools/stats.rs`, `src/tools/decode.rs`, `src/tools/tests.rs`, `src/tools/tests-adhoc.rs`, `src/tools/tests-render.rs`, `src/protocol/parse/tools.rs`, `src/test_support.rs`, `src/test_vm.rs`, `src/lib.rs`, `src/tests/engine_misuse.rs`, `src/engine_globals-store.rs`, and engine `lua.rs`, `execute/scope.rs`, `execute/scheduler/tool_call.rs`, `execute/tests.rs`, `execute/tests/tool_scoping.rs`
- Problem: four names describe a value they do not hold. `bound_tools` is the run's `ToolSet`, whose doc says "Nothing in it is bound" (`handles.rs:246-247`); `tool_name` returns a tool id; `current_tool_bindings` returns `OfferedTool`s, the static offers, rather than `ToolBinding`s; and `store_conflicts` is one slot that a later conflict overwrites (`engine_globals-store.rs:205`). The `tools` module doc also restates the wire-name fallback `wire_name_of` owns (`tools.rs:5-7` against `handles.rs:126-128`), and `adhoc_params_schema` claims to be the crate's only unsorted `pairs` walk (`tools/decode.rs:74-80`), a ranking no one can check locally.
- Change: rename `bound_tools` to `tool_set` at its eleven sites: `vm.rs:123`, `:210`, `:264`; `vm/state.rs:86`; `vm/inject.rs:138`, `:158`, `:179`; `tools/stats.rs:148`, `:156`; `test_support.rs:201`; and the doc at `tools/stats.rs:140`, which reads "by `tool_set`'s run catalog". Leave the engine's `bound_tool_set`, which is deferred and out of scope.
- Change: rename `tool_name` to `tool_id` at `tools/decode.rs:22`, the re-export at `tools.rs:45`, `protocol/parse/tools.rs:7` and `:63`, and `tools/tests.rs:7`, `:44`, `:47`, `:89`, `:98`, `:109`, `:119`. Leave `model_tool_name`, since a tool's name is its wire name in this crate.
- Change: rename `current_tool_bindings` to `current_static_set` at `vm/state.rs:140`, `vm.rs:50`, `lib.rs:148`, `test_vm.rs:219`, `:234`, `tools/tests-render.rs:85`, `:175`, `tools/tests-adhoc.rs:331`, and engine `lua.rs:12`, `execute/scope.rs:22`, `:73`, `execute/scheduler/tool_call.rs:61`, `:232`, `execute/tests.rs:19`, and `execute/tests/tool_scoping.rs:55`, `:76`, `:137`.
- Change: rename the `SectionVm` field `store_conflicts` to `store_conflict` (`vm.rs:151`, `:220`; `vm/inject.rs:168`, `:189`; `vm/install.rs:51`, `:64`), the test accessor `store_conflicts_handle` to `store_conflict_handle` (`vm/install.rs:72-73`, `tests/engine_misuse.rs:10`), and the `conflicts` field, locals, parameters, and doc mention in `engine_globals-store.rs` to `conflict` (`:70`, `:92`, `:96`, `:199`, `:205`, `:235`, `:255`, `:272`).
- Change: the wire-name sentence at `tools.rs:5-7` becomes "The model sees only wire names, as `wire_name_of` gives them, such as `web_fetch`." The `adhoc_params_schema` paragraph at `tools/decode.rs:74-80` becomes "`properties` is a `serde_json::Map`, a `BTreeMap` while `serde_json`'s `preserve_order` is off, so only `required` needs the sort below."
- Acceptance: the compiler catches every missed site, and `tools/tests.rs:41-120` still pins the decode behavior.

**Work item 6 (S4-9, S4-10): the messages list hands back its frozen sets**

- Files: `src/messages.rs`, `src/messages-tests.rs`, `src/tools.rs`, `src/compactors-tests.rs`, `src/tools/tests-offering.rs`, and engine `execute/scheduler/chat.rs`
- Problem: `attach_rendered_tools` (`messages.rs:237-243`) returns `()` though `get_or_insert_with` always leaves the slot `Some`, so engine `chat.rs:140-142` re-reads it with `rendered_tools()` and maps an impossible `None` to `Error::internal("a list holds the sets attached to it")`. Beside it, `rendered()` (`:216-220`) and `rendered_tools()` (`:222-226`) read one field.
- Change: first extend `a_list_is_rendered_only_once_sets_are_attached_and_keeps_the_first` (`messages-tests.rs:65-90`) to assert with `Arc::ptr_eq` that the second `attach_rendered_tools` returns the sets the first attach left in place. Today it does not compile, because the method returns `()`.
- Change: `attach_rendered_tools` returns the held `Arc<RenderedTools>`; engine `chat.rs:140` uses the returned value and deletes the re-read and its internal error. The workspace denies clippy's pedantic lints, `must_use_candidate` among them, so if clippy asks for `#[must_use]`, add it and bind the result with `let _ =` where a caller drops it: engine `chat.rs:230` (`merge_render`), `compactors-tests.rs:175`, and `tools/tests-offering.rs:228`.
- Change: delete `rendered()`; `refuse_after_render` (`tools.rs:71`), `compactors-tests.rs:182`, and `messages-tests.rs:68`, `:72` use `rendered_tools().is_some()`.
- Ceiling: `src/messages.rs` held 499 lines at the baseline, and the 500-line build ceiling fails at 501; deleting `rendered()` frees the room an added `#[must_use]` needs. `src/compactors-tests.rs` holds 478 lines.
- Acceptance: the extended test passes, and `tools/tests-offering.rs` still pins the offer-after-freeze refusal.

</step-2>

<step-3>

### Step 3: split `handles` and `sys`, one walkable read-only proxy, and the dispatch report path

- Component: Tool surface
- Tests:
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `$env:CARGO_BUILD_WARNINGS = "deny"; cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `cargo fmt --all --check`
  - `$env:RUSTDOCFLAGS = "-D warnings"; cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`
  - `$env:RUSTDOCFLAGS = "-D warnings"; cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`
  - `Remove-Item Env:RUSTDOCFLAGS, Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue; cargo doc --locked --no-deps --all-features -p runvil-lua --document-private-items`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`

- Work items: 14, 15, and 5, in one commit.
- Order: work item 14, then 15, then 5. Work item 14 splits `handles` and `sys`; work item 15 edits `seal_sys` in the slimmed `sys.rs` and builds its proxy in `tools/stats.rs` and `packs.rs`, whose imports work item 14 just moved; and work item 5 rewrites `dispatch.rs` and `tools/tests-parameters.rs`, which work item 14 also re-imports.
- Paths: lua paths are under `crates/runvil-internal/lua/`, paths marked "engine" are under `crates/runvil-internal/engine/src/`, and commands run from `c:\Users\Vinnie\cursor\runvil`. Line numbers were read at `7a5341bba`, and earlier steps and earlier work items in this step shift them, so every line number is a hint: locate each site by symbol or quoted text, and where a line and a symbol disagree, the symbol wins.
- Constraint: no author-visible behavior change. Every Lua global, table shape, and message stays as the guide documents it, and every type or signature change stays inside the workspace. Add no dependency and no crate. Every `.rs` file stays at or under 500 lines, or `build_ceiling::check()` fails the build.
- Acceptance: the Tests pass, plus each work item's acceptance below; the wording scan in Tests covers work item 5's `AGENTS.md` rulebook edit. The `runvil-lua --document-private-items` build is not a CI gate and passes on exit status; read its warnings for the lines this step edited, and treat a warning on an edited line as a failure.
- Loop: between work items, `cargo nextest run --locked -p runvil-lua -p runvil-engine -p runvil-parser --all-features`; the Tests above gate the commit.

**Work item 14 (S3-6, S3-7, S1-23): split `handles` and `sys`**

- Files: `src/handles.rs` (becomes `src/bindings.rs` and `src/section.rs`), `src/handles-tests.rs` (becomes `src/bindings-tests.rs`), `src/sys.rs`, new `src/var.rs`, `src/lib.rs`, and the importers listed below
- Problem: `handles` holds tool bindings, the tool set, `RosterEntry`, block results, section targets, section text, and library sections, but no Lua handle (those are `LuaToolHandle`, `LuaModelHandle`, and `LuaPackHandle`), and its module doc (`handles.rs:1-2`) names fewer than half its items. `ToolBinding.id` is documented "The selected stable live identity" (`handles.rs:45`, `:94`), though `from_info` copies `info.id` with no selection (`:70`). `sys.rs` holds one `sys` item, `seal_sys`, beside nine `var` items.
- Change: `bindings.rs` takes `ToolOutputKind`, `ToolBinding`, `wire_name_of`, `RosterEntry`, and `ToolSet`, with the module doc "Tool bindings, the per-binding output kind, the run's shared tool set, and how bound tools reach Lua." `handles-tests.rs`, which tests only those items, becomes `bindings-tests.rs`, wired as `#[cfg(test)] #[path = "bindings-tests.rs"] mod tests;`.
- Change: `section.rs` takes `LuaBlockResult`, `resolve_section_target`, `SectionText`, `LibraryKey`, `Library`, and `LibraryBlock`, with the module doc "What the VM exchanges with its caller about sections: a block's result, a `call` or `jump` target, the text a prompt read sees, and the library sections `require` loads."
- Change: `ToolBinding.id`'s doc becomes "The tool's id." and `id()`'s becomes "Returns the tool's id."
- Change: `var.rs` takes `VAR_DATA_REGISTRY`, `VAR_PROXY_REGISTRY`, `VAR_GUARDED_DATA_REGISTRY`, `guarded_json_value`, `guarded_table`, `materialize_guarded_value`, `guarded_var`, `var_snapshot_table`, and `var_to_json`, with the `# Errors` text step 1 gave the last two; `sys.rs` keeps `seal_sys`. Split the `sys.rs:1` module doc between the two files.
- Change: in `lib.rs`, replace `mod handles;` with `mod bindings;` and `mod section;`, add `mod var;`, and point the root re-exports (`lib.rs:127-130`) at the new modules under the same names, so no public path changes. Update the imports in the 22 files that name `crate::handles::`, all under this crate's `src/`: `dispatch.rs`, `messages.rs`, `packs.rs`, `packs-tests.rs`, `protocol/answer.rs`, `protocol/parse.rs`, `require.rs`, `scope.rs`, `test_support.rs`, `tests.rs`, `tools.rs`, `tools/objects.rs`, `tools/offer.rs`, `tools/stats.rs`, `tools/tests-parameters.rs`, `tools/userdata.rs`, `vm.rs`, `vm/adhoc.rs`, `vm/install.rs`, `vm/run.rs`, `vm/state.rs`, and `lib.rs`. Update the `var` imports from `crate::sys::` at `coro.rs:38`, `prose.rs:30`, `tests.rs:26`, `vm/inject.rs:17`, `vm/install.rs:20`, and `vm/state.rs:22`. A `crate::handles::` hit in `crates/workshop/workspace/src/workspace/tests/close.rs` is that crate's own `handles` module, not this one; leave it.
- Ceiling: `src/messages.rs` sits at the 500-line build ceiling; its edit here is an import path only, so add no lines to it.
- Acceptance: the compiler catches every missed import. The engine imports through the crate root (engine `crates/runvil-internal/engine/src/lua.rs`), so no engine file changes, and every public path stays as it was.

**Work item 15 (S3-24, S1-22): one walkable read-only proxy, and two seal docs**

- Files: `src/proxy.rs`, `src/tools/stats.rs`, `src/packs.rs`, `src/sys.rs`, `src/prelude.rs`
- Problem: `tools::stats::proxy` (`tools/stats.rs:255-271`) builds an empty table whose metatable holds `__index`, a `__pairs` over `step`, a `__newindex` raising `mlua::Error::external("<path> is read-only")`, and that label as `__metatable`. `install_pack_list` builds the same four entries inline for `tools.packs` (`packs.rs:192-239`) with `PACKS_LABEL` (`packs.rs:42`), adding only `__len`, so a hardening change to one misses the other. Separately, the `seal_sys` doc (`sys.rs:143-148`) and prelude `seal` (`prelude.rs:202-204`) restate the contract of `read_only_proxy` (`proxy.rs:26-48`), which both bodies call.
- Change: move `tools::stats::proxy` into `src/proxy.rs` as `pub(crate) fn read_only_walk(lua, index, step, path)`, keeping its `external` refusal, and build `tools.stats`, `tools.stats.packs`, `tools.stats.tools`, and `tools.packs` through it. Do not fold it into `read_only_proxy`, which raises a per-key runtime error and has no `__pairs`; the guide documents both messages (`05-lua-environment.md:1819-1825`, `12-tools.md:569`, `:1249`).
- Change: `install_pack_list` raw-sets `__len` on the metatable `read_only_walk` returns (Rust's `metatable()` reads past `__metatable`), and drops `PACKS_LABEL` and its inline `__pairs`, `__newindex`, and `__metatable` code (`packs.rs:224-239`). Add one line to the `proxy` module doc (`proxy.rs:1-3`) naming `read_only_walk`'s users, `tools.stats` and `tools.packs`.
- Change: the `seal_sys` doc body becomes "A [`read_only_proxy`] over the JSON object: a read of an absent field raises, and a present `null` reads as nil." Prelude `seal`'s doc becomes "Seals a prelude's table global at its top level with a [`read_only_proxy`] over the hidden table." Both files import `read_only_proxy`, so the links resolve. Keep `read_only_var`'s depth rule.
- Acceptance: `packs-tests.rs:95-116` and `tools/tests-stats.rs:105-138` still pin the labels and the protected metatable. They match with `contains`, so they would not catch an added "runtime error: " prefix; keeping `mlua::Error::external` avoids one, and the `<path> is read-only` text stays exactly as the guide quotes it.

**Work item 5 (V3-3, S6-17, S3-3, S3-4, S3-5, S5-1): the dispatch report path**

- Files: `src/dispatch.rs`, `src/dispatch-tests.rs`, `src/tools/tests-parameters.rs`, the crate's `AGENTS.md` (`crates/runvil-internal/lua/AGENTS.md`), and engine `execute/scheduler/apply.rs`
- Problem: `prepare_dispatch` (`dispatch.rs:119-127`) is public with `script: Option<ScriptReport>`, and under `None` it fires no `ToolResult` (`:157-168`). Its outside callers are `dispatch-tests.rs` at lines 20, 72, 148, 197, and 217, `tools/tests-parameters.rs:155`, and engine `apply.rs:317`, and all but two pass `Some`; `None` comes only from `prepare_model_dispatch` (`dispatch.rs:201`) and the two tests that exercise it (`dispatch-tests.rs:72-80`, `tools/tests-parameters.rs:155-163`). `ModelReport` (`:46-51`) holds its turn in a field named `script`, typed `ScriptReport`, read only as `report.script.turn` (`:212`). Three docs send the reader to "the loop" or to a `tool_call_id` field for a report only `prepare_model_dispatch` fires, the no-call rule is stated four times in the file, and the crate's `AGENTS.md:4` says `prepare_dispatch` applies counts, which it has not since commit `be75abaa7`.
- Design: narrow `prepare_dispatch` instead of documenting `None`. Taking `ScriptReport` and keeping an `Option` body private makes the silent path unrepresentable. `ModelReport` flattens to `turn`, since the old field name asserts a path the call is not on.
- Change: move the body of `prepare_dispatch` into `pub(crate) fn dispatch_body` with the same parameters, `script: Option<ScriptReport>` included; it is crate-visible only because `tools/tests-parameters.rs:155` sits outside the `dispatch` module. The public `prepare_dispatch` takes `script: ScriptReport` and calls `dispatch_body` with `Some(script)`. `prepare_model_dispatch` and the two tests that pass `None` (`dispatch-tests.rs:72-80`, `tools/tests-parameters.rs:155-163`) call `dispatch_body`. The four other `dispatch-tests.rs` call sites pass `Some` and keep calling the public function.
- Change: `ModelReport` becomes `pub struct ModelReport { pub turn: u32, pub call_id: String }`. Its doc becomes "The run coordinates a model-issued dispatch reports under: the turn the call fired in and the call id the model issued.", and the `turn` field doc "The section's completed model-turn count at dispatch." `dispatch.rs:212` reads `report.turn`, engine `apply.rs:302` builds `turn: call.report.turn`, and `dispatch-tests.rs:93-97` follows.
- Change: `ScriptReport`'s doc (`dispatch.rs:26-29`) becomes "[`prepare_dispatch`] fires the `ToolResult` event with them under an empty `tool_call_id`, since only a model-issued call has one." and keeps "The chain and depth are not part of the report: ...". `ToolDispatch`'s second sentence (`:55-57`) becomes "[`prepare_model_dispatch`] reports the pair through the `ToolResult` event under the model's call id; a script-initiated dispatch reads only the content, its report already fired inside [`prepare_dispatch`]."
- Change: in the public `prepare_dispatch` doc (`:106-110`), the sentence from "A script-initiated call" becomes "It also fires the `ToolResult` event with the final content and `vendor`, the vendor of the tool's Pack." `dispatch_body`'s doc says "A model-issued call passes `None`, and [`prepare_model_dispatch`] reports the returned [`ToolDispatch`] under the model's call id." The public doc must not link `dispatch_body`, which `private_intra_doc_links` denies.
- Change: end the module doc's clause at `:5-7` at "takes the tool's outcome as a value", dropping ", so the Harness performs the call and the Engine applies the same rules when the answer arrives", and keep `:10-14`. Delete the "Nothing here awaits: ..." sentence from `prepare_dispatch` (`:113-114`) and from `prepare_model_dispatch` (`:184-186`).
- Change: the first sentence of the crate's `AGENTS.md:4` becomes "`prepare_dispatch` is the single tool-dispatch body used by every executor: synchronous, it applies the succeeded/failed observation, trust classification, the nonce wrap, and the `ToolResult` report to a bound tool's answer; the executor counts each call when it issues it, and adhoc and discovery tools report in the executor." Keep its second sentence.
- Acceptance: `dispatch-tests.rs` covers both paths (`:16-90`, and from `:100` the model report's turn and call id). No public caller can reach the path that fires no `ToolResult`.

</step-3>

<step-4>

### Step 4: the parse module, the `Answer` failure side, the task status phase, and the protocol docs

- Component: Protocol types
- Tests:
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `$env:CARGO_BUILD_WARNINGS = "deny"; cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `cargo fmt --all --check`
  - `$env:RUSTDOCFLAGS = "-D warnings"; cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`
  - `$env:RUSTDOCFLAGS = "-D warnings"; cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`
  - `$env:RUSTDOCFLAGS = "-D warnings"; cargo doc -p runvil --no-deps`
  - `Remove-Item Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue; cargo +nightly-2026-09-05 xtask api --check`
  - `Remove-Item Env:RUSTDOCFLAGS, Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue; cargo doc --locked --no-deps --all-features -p runvil-lua --document-private-items`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`

- Work items: 8, 9, 10, and 11, in one commit. The operator chose to land work item 9's restructure in this pass, after work item 8, since both edit `YieldParse` and `classify`.
- Order: work item 8, then 9, then 10, then 11. Work item 9 needs work item 8's `YieldParse::Refused`; work item 10 reshapes `TaskStatus` in the `answer.rs` and `render.rs` work item 9 just rewrote; and work item 11 documents `CHAT_PART_TYPES` where work item 8 moved it and the `Answer` work item 9 left. All four overlap in `protocol/parse.rs`, `protocol/answer.rs`, the `protocol/parse/` files, `protocol/record.rs`, and `src/protocol/tests/`.
- Paths: lua paths are under `crates/runvil-internal/lua/`, paths marked "engine" are under `crates/runvil-internal/engine/src/`, and commands run from `c:\Users\Vinnie\cursor\runvil`. Line numbers were read at `7a5341bba`, and earlier steps and earlier work items in this step shift them, so every line number is a hint: locate each site by symbol or quoted text, and where a line and a symbol disagree, the symbol wins.
- Constraint: no author-visible behavior change. Every Lua global, table shape, and message stays as the guide documents it, and every type or signature change stays inside the workspace. Add no dependency and no crate. Every `.rs` file stays at or under 500 lines, or `build_ceiling::check()` fails the build.
- Acceptance: the Tests pass, plus each work item's acceptance below. The facade docs build and the facade surface check run because work item 11 edits the doc of `VfsOp`, a facade re-export: `cargo +nightly-2026-09-05 xtask api --check` (the nightly `crates/build-xtask/src/api/toolchain.rs` names, run with `CARGO_BUILD_WARNINGS` unset) must pass with `crates/runvil/public-api.txt` unchanged. The `runvil-lua --document-private-items` build is not a CI gate and passes on exit status; read its warnings for the lines this step edited, and treat a warning on an edited line as a failure.
- Loop: between work items, `cargo nextest run --locked -p runvil-lua -p runvil-engine -p runvil-parser --all-features`, and for the docs-only work item 11, the workspace docs command in Tests; the Tests above gate the commit.

**Work item 8 (S2-16, V2-2, S2-17, S2-20): the parse module's names, helpers, and record validators**

- Files: `src/protocol/parse.rs`, `src/protocol/parse/store.rs`, `src/protocol/parse/tasks.rs`, `src/protocol/parse/tools.rs`, `src/protocol/parse/chat.rs`, `src/protocol/record.rs`, `src/protocol.rs`, `src/messages.rs`, `src/vm/run.rs`, `src/protocol/tests/parse.rs`, `src/protocol/tests/parse_tasks.rs`, `src/protocol/tests/parse_tool_call.rs`, `src/protocol/tests/parse_chat.rs`, `src/messages-tests-view.rs`, and engine `execute/scheduler/step.rs` and `lua/tests/shims.rs`
- Problem: `YieldParse::Call` (`parse.rs:180-190`) and `FieldFailure::Call` (`parse.rs:67-75`) mean "an author-supplied argument failed validation", while `Request::Call` and `Answer::Call` mean the `call(target, input?)` request, so tests read `YieldParse::Call(Answer::Call(Err(..)))` (`protocol/tests/parse.rs:269`) and the engine needs a comment at `step.rs:80-82` to explain the variant. `parse::store` builds `FieldFailure::Call(Error::Lua(format!(..)))` by hand at `store.rs:33-36` and `:80-82` instead of through `call_error` (`parse.rs:77-80`), the helper every other per-op parser uses. `direct_yield<T>` (`:46`), `raw_field` (`:54`), and `json_field` (`:59`) are a three-layer chain whose errors are all discarded: `raw_field` has one caller at `:209` and `json_field` one at `:163`, and `json_field` itself calls `direct_yield` twice (`:61`, `:62`). The record validators sit in the chat request parser (`parse/chat.rs:16-27`, `:67-288`), which never calls them; their callers are the list builders (`messages.rs:312`, `:456`) and tests.
- Change: `parse::store` uses `call_error(format!(..))` at both sites, imports it from `super`, and drops `use crate::Error;` (`store.rs:6`).
- Change: rename `YieldParse::Call` to `YieldParse::Refused` and `FieldFailure::Call` to `FieldFailure::Argument` at their definitions, in `classify`, `call_error`, the docs of `Request::from_yield` and `SectionVm::request_from_yield`, `vm/run.rs:102`, `parse/tasks.rs`, `parse/tools.rs`, engine `execute/scheduler/step.rs:83` (deleting its comment at `:80-82` if it only explains the name), engine `lua/tests/shims.rs:325`, and the tests in `protocol/tests/parse.rs`, `parse_tasks.rs`, `parse_tool_call.rs`, and `parse_chat.rs`. The `call_*` helpers keep their names; there "call" means the shim call.
- Change: delete `direct_yield<T>`, `raw_field`, and `json_field`, keeping `DIRECT_YIELD` and `direct_yield_error` as the one builder. Read `op` (`parse.rs:209`) with `table.raw_get::<Value>("op")` as the other readers do, and give `shim_var` (`:156-164`) `json_field`'s raw read and JSON conversion inline, each failure mapping to `FieldFailure::Malformed`. Add `raw_field`'s constraint to the module doc: "Field reads are raw: the table comes from script space, so a metatable must not intercept or forge a field."
- Change: move `parse_message`, `parse_content_part`, `parse_content_parts`, `parse_tool_call_record`, `chat_roles`, `CHAT_PART_TYPES`, `content_refusal`, and `image_url_refusal` from `parse/chat.rs` into `protocol/record.rs` beside `MessageRecord` and `MAX_RECORD_META_BYTES`, and rename `parse_message` to `parse_record`, deleting the one-line wrapper (`chat.rs:67-72`). Update the paths at `messages.rs:44`, `protocol.rs:36`, `protocol/parse.rs:17`, and `messages-tests-view.rs:9`. The `parse::chat` module doc (`chat.rs:1-3`) and the `parse` module doc (`parse.rs:1-6`) then describe only the requests, and `MessageRecord::to_json` (`record.rs:101-104`) says "the record parse" in place of "the chat parse".
- Ceiling: `protocol/record.rs` holds 156 lines and gains about 235, staying under 500. `src/messages.rs` sits at the 500-line build ceiling; its edit here is a path change only, so add no lines to it.
- Acceptance: `protocol/tests/parse.rs:85-203` (`assert_direct_yield`) still pins every malformed path, and `messages-tests-view.rs:47-98`, `messages-tests-records.rs`, and `messages-tests-list.rs` still pin the record refusal texts, which the move leaves unchanged.

**Work item 9 (S2-11): the `Answer` failure side**

- Files: `src/protocol/answer.rs`, `src/protocol/render.rs`, `src/protocol/parse.rs`, `src/protocol/parse/store.rs`, `src/protocol/parse/tasks.rs`, `src/protocol/parse/tools.rs`, `src/protocol/parse/chat.rs`, `src/vm/run.rs`, every file under `src/protocol/tests/`, and in the engine `execute/scheduler/`, `execute/error.rs`, `lua/tests/errors.rs`, and `lua/tests/shims.rs`
- Scope warning: this is the widest work item in the plan. A search of the engine finds 28 files that name `Answer` and 27 `Answer::*(Err(..))` constructions, well beyond the sites listed below. Do not treat the list as complete: after the type change, build and work through every compiler error. Grep `Answer::` across `crates/`, not just the engine.
- Problem: every `Answer` variant (`answer.rs:246-306`) wraps its own `Result`, but the Lua side never reads the variant of a failed answer: `render` sends `(false, table)` for all of them through one or-pattern (`render.rs:158-178`). Each engine failure site picks a variant only to wrap the error, `dropped_answer` (engine `apply.rs:95-103`) calls the store variant "the harmless stand-in" for a timer, and the one engine reader of a failed variant, `fill_batch_slot` (engine `tool_call.rs:254-276`), treats any failed non-`ToolCallResult` answer as an internal error nothing produces.
- Change: `Answer<E>` holds success values only, with `JoinAny` keeping `TaskDelivery<E>` for a member's failure; `map_error` (`answer.rs:308-334`) keeps only the variants that still carry `E`; render and resume take `Result<Answer<E>, E>`; `YieldParse::Refused` (work item 8's name, set earlier in this step) holds a bare `Error`; and `classify` (`parse.rs:247-258`) loses its `answer` parameter, so the fourteen per-op closures at `parse.rs:216-242` go.
- Change: sites the analysis confirmed, as a starting set rather than an exhaustive one. The engine's failure wrappers: `apply.rs:95-103` (delete `dropped_answer` and its comment), `apply.rs:254`, `tasks-spawn.rs:52`, `waits.rs:85`, `dispatch.rs:138`, `:200`, `tool_call.rs:121`, `chat.rs:73`, `:204`, `chain.rs` (the `Answer` construction near `:235`), and `step.rs:83`. Functions and fields that take `Answer<Error>` and need the new shape: `chain_ops.rs`, `scheduler.rs`, `admission.rs`, and `chain_record.rs`. Whole-`Result` wrappers the analysis missed: `timer.rs`, `tool_call-adhoc.rs`, and further sites in `apply.rs`, `waits.rs`, `tool_call.rs`, and `chat.rs`. All of these are under engine `execute/scheduler/`.
- Change: `fill_batch_slot` becomes `Err(error) => Err(error)` for every failure; `SectionVm::resume_block_coro_answer` (`vm/run.rs`) and the `Retained` handling in `render.rs` follow the new signature, and so does any other `vm/run.rs` resume method the compiler flags.
- Change: the tests that pin a per-op failure variant change with it: `protocol/tests/parse.rs:221-275`, `parse_tasks.rs:112-291`, `parse_tool_call.rs:154-207`, `parse_chat.rs:43`, `answer.rs:56`, `:231`, `answer_chat.rs:10`, `answer_retained.rs:34`, engine `lua/tests/errors.rs:131`, `:267`, and the success-side constructions throughout the engine's own test files. `lua/tests/shims.rs` also names the variant.
- Acceptance: the compiler checks every match on `Answer`, which is the real completion test for this work item. The Lua envelope is unchanged: every failure still resumes the coroutine with `(false, table)`, and every success with the same values as before.

**Work item 10 (S2-12): the task status phase holds the outcome**

- Files: `src/protocol/answer.rs`, `src/protocol/render.rs`, engine `execute/scheduler/waits.rs`, and engine `execute/scheduler/tasks.rs`
- Problem: `TaskStatus.ok` (`answer.rs:228-230`) is `None` exactly when the phase is `Running` and `Some(false)` for `Cancelled` and `Abandoned`; only `Done` needs a value of its own. The one builder (engine `waits.rs:136-164`) derives both fields from one engine `TaskState`, which already holds `Done { ok }` and `Delivered { ok }` (engine `execute/scheduler/tasks.rs:95-117`), so the protocol type re-splits a fact the engine keeps whole.
- Change: `TaskPhase::Done { ok: bool }` (`answer.rs:158-169`), and delete `TaskStatus.ok`. The renderer (`render.rs:25-45`) derives the unchanged Lua `ok`: absent for `running`, the flag for `done`, and `false` otherwise. `task_phase` (engine `waits.rs:46-55`) maps `Done { ok } | Delivered { ok }` to `Done { ok }`, and the builder drops `ok: state.ok()` (`waits.rs:142`). Keep `section` and `blocked` as they are; both are `Option` even while running.
- Change: `TaskState::ok` (engine `execute/scheduler/tasks.rs:112`) has exactly one caller, `waits.rs:142`, so deleting that call leaves it dead. Delete the method too, or the `CARGO_BUILD_WARNINGS=deny` gate fails on it.
- Change: the unit-variant uses of `TaskPhase::Done` stop compiling once it takes a field. The phase-name match at `answer.rs:178` (`TaskPhase::Done => "done"`) becomes `TaskPhase::Done { .. } => "done"`; engine `waits.rs:51` is the `task_phase` arm the change above rewrites. The `TaskStatus.phase` doc at `answer.rs:226` links [`TaskPhase::Done`], which still resolves.
- Acceptance: the Lua `ok` field reads exactly as before for every phase. Engine `execute/tests/waits.rs`, `execute/tests/timeouts.rs`, and `execute/tests/scheduler/concurrency.rs` exercise `tasks.status`.

**Work item 11 (S2-10, S2-13, V2-1, S2-18, S2-19, S2-21, S2-22, S2-24, S2-25): the protocol docs**

- Files: `src/protocol.rs`, `src/protocol/answer.rs`, `src/protocol/request.rs`, `src/protocol/parse.rs`, `src/protocol/parse/tasks.rs`, `src/protocol/parse/chat.rs`, `src/protocol/record.rs`, `src/compactors.rs`
- Scope: docs only.
- Problem: the protocol docs miss most task requests in a list that claims to be the audit surface, predate structured and adhoc tool outputs, say an abandoned task is delivered though it never is, restate four rules in eleven places, call a role-unchecked field assistant-only, promise data-URI images where any URL parses, say every store call yields though Pack preludes call the direct closures, and say arguments are checked once at parse while describing a run-time check.
- Change: in `protocol.rs:4-12`, the sentence holding the parenthetical list becomes "Every suspending Engine call, one per `Request` variant, is a Lua-side shim that yields a request table; the driver validates the yield into a `Request`, dispatches it, and resumes the coroutine with the `(ok, result)` envelope rendered from an `Answer`."
- Change: the `ToolCallOutcome` doc (`answer.rs:44-48`) becomes "One dispatched `tools.call`'s successful answer: a bound tool's output, classified by the binding's declared `ToolOutputKind` so the envelope resumes the right Lua shape (plain text as a Lua string, structured JSON as a Lua table, the Engine performing the one conversion), or an adhoc tool's handler for the shim to run." Replace the whole doc, not just 44-46, or two lines are orphaned. In the `Plain` doc, delete only "every existing tool, ". The `TaskDelivery` clause at `answer.rs:142-143` becomes "the `cancelled` value for a task its owner cancelled (an abandoned task is never delivered, since its owner has ended)".
- Change: the target rule. In `Request::from_yield` (`parse.rs:200-204`), "One boundary conversion keeps its own byte-identical error:" becomes "One argument keeps another boundary's error:". `call_target` (`parse.rs:166-168`) becomes "Reads the author-supplied `target` of a `call` or `spawn`; a non-string target fails as `resolve_section_target` fails." Delete the parenthetical "(validated with the `resolve_section_target` rule, keeping its byte-identical error)" from `parse_call` (`parse.rs:296-298`) and "(validated with the `resolve_section_target` rule, as `call`'s is)" from `parse_spawn` (`parse/tasks.rs:35-38`). `Request::Call.target` (`request.rs:54-55`) and `Request::Spawn.target` (`request.rs:68-69`) become "The heading string."
- Change: the handle, empty-list, and adhoc rules. Keep the clone rule in the `Request` type doc (`request.rs:34-39`) and `call_handle` (`parse.rs:272-294`). `Request::Chat.binding` (`request.rs:195-198`) becomes "The receiver of a handle's `loop`, as its frozen binding; `None` when the round names no handle, and the driver resolves the section's current model." `parse_infer` (`parse.rs:260-265`) becomes "Parses an `infer` request: the author-supplied `prompt` and the optional handle `call_handle` reads." From `parse_chat` (`parse/chat.rs:35-39`), delete "An empty list parses: the driver refuses it at dispatch." and "A handle failure is the call's error, raised at the `models.loop` call site so a program `pcall` catches it." From the `Request::Chat` summary (`request.rs:185-186`), delete "The dispatch arm first refuses an empty list;" and keep the rule on `Request::Chat.list` (`:191-192`). Delete the second paragraph of the `answer` module doc (`answer.rs:4-9`), keeping the adhoc flow in the `request` module doc (`request.rs:5-10`).
- Change: the `MessageRecord.tool_calls` doc (`record.rs:94-95`) becomes "The normalized tool calls the record holds. Only an assistant record may send them: the record parse accepts them on any role, and the projection refuses them on every other role."
- Change: `ContentPart`'s doc (`record.rs:12-13`) says "visible text or an image URL"; `ImageUrl` (`record.rs:18`) becomes "An image URL; the record parse does not restrict its scheme."; the `CHAT_PART_TYPES` doc, now in `record.rs` since work item 8 moved it earlier in this step, ends "(the Multimodal contract: text parts and image-URL parts)"; and `compactors.rs:236` says "the URL's length" in place of "the data-URI length".
- Change: `Request::Store` (`request.rs:201-205`) becomes "`store.*(...)`: one run-scoped store operation as a leaf yield. Once a VM's yield shims install, every operation takes this path, memory- and real-file-backed alike, with no inline fast path, so interleaving behavior never depends on the backend. Pack preludes run before that and use the direct store closures." Leave `coro.rs:370-373` and `__impl_coro.lua:271-275`, which are accurate.
- Change: the `VfsOp` doc's second paragraph, at `request.rs:230`, which reads "The Engine checks the arguments once, when it parses the call.", becomes "The parse checks each argument's shape; the operation checks line ranges when it runs." Leave the summary sentence at `:227-228` alone, and leave the serde form untouched.
- Acceptance: the docs commands pass, the facade docs build and the facade surface check pass with `crates/runvil/public-api.txt` unchanged, and the new `VfsOp` sentence names no internal crate.

</step-4>

<step-5>

### Step 5: the test-only hooks, one coroutine shim installer, the `sys` mirror at injection, and the VM docs

- Component: VM setup and models loop
- Tests:
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `$env:CARGO_BUILD_WARNINGS = "deny"; cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `cargo fmt --all --check`
  - `$env:RUSTDOCFLAGS = "-D warnings"; cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`
  - `$env:RUSTDOCFLAGS = "-D warnings"; cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`
  - `Remove-Item Env:RUSTDOCFLAGS, Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue; cargo doc --locked --no-deps --all-features -p runvil-lua --document-private-items`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`

- Work items: 20, 21, 22, 17, and 23, in one commit.
- Order: work item 20, then 21, then 22, then 17, then 23. Work item 21 needs work item 20's `stash_shims` edits (the decision record requires 20 before 21); work item 22's `coro.rs` doc edit sits beside the loop-shim mention work item 21 deletes; work item 17 changes `SectionVm`'s fields and `Injected` in the `vm/inject.rs` and `vm/install.rs` that work items 21 and 22 edit; and work item 23 then rewrites the `vm.rs` docs around the fields work item 17 leaves. All five overlap in `coro.rs`, `__impl_coro.lua`, `test_support.rs`, `lib.rs`, `vm.rs`, `vm/inject.rs`, and `vm/install.rs`.
- Paths: lua paths are under `crates/runvil-internal/lua/`, paths marked "engine" are under `crates/runvil-internal/engine/src/`, and commands run from `c:\Users\Vinnie\cursor\runvil`. Line numbers were read at `7a5341bba`, and earlier steps and earlier work items in this step shift them, so every line number is a hint: locate each site by symbol or quoted text, and where a line and a symbol disagree, the symbol wins.
- Constraint: no author-visible behavior change. Every Lua global, table shape, and message stays as the guide documents it, and every type or signature change stays inside the workspace. Add no dependency and no crate. Every `.rs` file stays at or under 500 lines, or `build_ceiling::check()` fails the build.
- Acceptance: the Tests pass, including the new batch-turn parse test, plus each work item's acceptance below. The `runvil-lua --document-private-items` build is not a CI gate and passes on exit status; read its warnings for the lines this step edited, and treat a warning on an edited line as a failure.
- Loop: between work items, `cargo nextest run --locked -p runvil-lua -p runvil-engine -p runvil-parser --all-features`, and for the docs-only work item 23, the workspace docs command in Tests; the Tests above gate the commit.

**Work item 20 (S1-4, S2-23): the test-only tool-call hook and the tool-call request shape**

- Files: `src/coro.rs`, `src/__impl_coro.lua`, `src/test_support.rs`, `src/protocol/request.rs`, `src/protocol/parse/tools.rs`, `src/lib.rs`, `src/protocol/tests/parse_tool_call.rs`, and engine `lua.rs`, `execute/scheduler/dispatch.rs`, `execute/scheduler/tool_call.rs`, `execute/scheduler/effects.rs`, `execute/tests/tool_call_arm.rs`, `execute/tests/full_id_calls.rs`, `execute/tests/tool_call_arm-adhoc-handlers.rs`
- Problem: `stash_shims` stores `model_tool_call` under `MODEL_TOOL_CALL_REGISTRY` on every VM (`coro.rs:294-296`), though its only reader is `test_support::install_model_tool_call_shim` (`test_support.rs:94-102`), whose one caller is behind `#[cfg(test)]` (engine `execute/section_vm.rs:192-195`). That hook, `tools.call_as_model`, is also the only producer of a third `Request::ToolCall` state: production yields `(None, None)` from the script shim (`__impl_coro.lua:197-200`) and `(Some, Some)` from the loop (`models_loop.rs:361-369`), `(Some, None)` comes only from the hook, and `(None, Some)` has no producer, yet the engine keeps `CallSite::Script { turn: Option<u32> }` for it (engine `effects.rs:88-125`).
- Change: gate the `model_tool_call` stash in `stash_shims` and the `MODEL_TOOL_CALL_REGISTRY` constant behind `#[cfg(any(test, feature = "test-support"))]`. `stash_shims`'s doc (`coro.rs:282-283`) stops naming `model_tool_call`, and the constant's doc (`coro.rs:80-85`) says test builds stash it for `install_model_tool_call_shim` rather than every prelude install. Work item 21, next in this step, removes `loop` and `compact` from the same `stash_shims` doc. Optionally move `tools_call_as_model` (`__impl_coro.lua:202-219`) into a test-only chunk that `install_model_tool_call_shim` loads, so production VMs never compile it.
- Change: `Request::ToolCall { alias, args, model: Option<ModelCall> }` with `pub struct ModelCall { pub call_id: String, pub turn: u32 }`, documented on the type and both fields and re-exported beside `Request` in `lib.rs`, since the workspace warns on `missing_docs` and `unreachable_pub`.
- Change: `ToolCallFields` (`parse/tools.rs:12-19`) is shared by a `tools.call` request and one entry of the loop's `tool_calls` batch, and a batch entry carries no turn of its own; `parse_tool_calls` reads one `turn` for the whole batch (`:108`) and every entry's `call_id` is required (`:112-126`). So `ToolCallFields` cannot simply take the new shape. Keep `call_id: Option<String>` and `turn: Option<u32>` in it, and have the two callers assemble `ModelCall`: `parse_tool_call` (`:21-36`) builds `model` only when `call_id` is `Some`, pairing it with its own `turn` and failing as malformed when a `call_id` arrives without a turn; `parse_tool_calls` builds `model` from the entry's required `call_id` and the batch's `turn`.
- Change: add a parse test in `src/protocol/tests/parse_tool_call.rs` for a batch of two entries asserting both carry the batch turn. The `turn: None` parse test (`protocol/tests/parse_tool_call.rs:62-75`) follows the new rule: a `call_id` without a `turn` now fails as malformed, so it asserts that refusal.
- Change: in the engine, `CallSite::Script` drops `turn` and `CallSite::Model.turn` becomes `u32`; update `execute/scheduler/dispatch.rs:101-108`, `tool_call.rs:106-116`, `call_turn` (`tool_call.rs:202-208`), and `tool_call.rs:311-315`.
- Change: the hook takes a required turn argument, since the live turn counter lives in the engine (`chain.ctx.turns()`), not in Lua. Update its five calls (engine `execute/tests/tool_call_arm.rs:87`, `full_id_calls.rs:236`, `tool_call_arm-adhoc-handlers.rs:99`, `:172`, `:215`).
- Acceptance: the batch test passes and both batch entries carry the batch turn. The engine dev-dependency enables `runvil-lua/test-support` (engine crate `Cargo.toml:33`), so the gated hook still serves those five tests, and `protocol/tests/parse_tool_call.rs:36-108` still pins the parse. A production build has no `MODEL_TOOL_CALL_REGISTRY`.

**Work item 21 (S1-5, V1-2): one coroutine shim installer**

- Files: `src/coro.rs`, `src/vm/install.rs`, `src/lib.rs`, `src/prelude-tests.rs`, and engine `execute/section_vm.rs` and `lua.rs`
- Problem: the only production setup calls `install_coro_shims` and then `install_section_loop_shim` back to back (engine `execute/section_vm.rs:176-177`). Commit `19c16834a` split the loop install out so agent VMs would not receive it, and no agent VM remains, yet the public installer, its re-export, the `LOOP_REGISTRY` and `COMPACT_REGISTRY` stashes (`coro.rs:68-74`, `:285-290`), and a runtime "prelude never installed" error survive for a call order that excludes them. `install_coro_shims`'s doc (`vm/install.rs:132-134`) also says `max_tool_iterations` serves only the `models.loop` shim a later install adds and that a VM without it "passes any value", though `loop_begin` spends the cap for a model handle's `loop` too (`coro.rs:199-205`, `__impl_coro.lua:260-261`).
- Design: one coroutine shim installer. `install_coro_shims` installs `models.loop` and `models.compact` itself. `max_tool_iterations` stays on it, because a model handle's `loop` spends the cap whether or not `models.loop` exists.
- Change: the shims table lives in `install_shim_prelude` (`coro.rs:275` calls `stash_shims(lua, &shims)`), not in `install_coro_shims`, which is a thin wrapper in `vm/install.rs:138` that passes five arguments into it. So the loop install belongs in `install_shim_prelude`, right after `stash_shims`, where the `shims` table is still in hand: read `shims.raw_get("loop")` and `shims.raw_get("compact")` and `raw_set` them on the `models` table there. The `models` table already exists at that point, because `install_models` runs during injection (`vm/inject.rs:143`), before any shim installer. `install_shim_prelude` moves `models` by value into the chunk call, so clone it or re-read the global before the loop install.
- Change: delete `install_section_loop_shim` (`coro.rs:347-353`), its re-export (`lib.rs:115`, keeping `install_store_shims`), the two registry stashes in `stash_shims` and the `LOOP_REGISTRY` and `COMPACT_REGISTRY` constants, and the doc mention at `coro.rs:136-139`. `stash_shims`'s doc becomes "Stashes the shim chunk's `handle_methods` and `store` under their registry keys, for the installs and the handle getters that read them after the prelude.", plus the test-build `model_tool_call` stash if work item 20 left a mention of it there.
- Change: update engine `execute/section_vm.rs:182` (the `crate::lua::install_section_loop_shim(vm.lua())?` line) and its doc near `:103`, the engine `lua.rs` re-export list, and `prelude-tests.rs:16`, `:30`.
- Change: the `install_coro_shims` doc (`vm/install.rs:130-134`) becomes "Installs the coroutine yield shims (`models.infer`, `models.loop`, `models.compact`, `call`, `tasks.fanout`, `tools.call`, the `tasks` namespace) through `install_shim_prelude`, which also puts `models.loop` and `models.compact` on the `models` table." followed by "`max_tool_iterations` is the run's resolved round cap, which every `models.loop` and model handle `loop` call on this VM spends." Keep the parameter on this installer and keep its `# Errors`.
- Change: VMs built through `TestVm::shims()` (`test_vm.rs:384-391`) now hold `models.loop` and `models.compact`. No test asserts their absence on such a VM: `models-tests.rs:91` (`a_handle_reads_nil_methods_where_the_shim_prelude_never_ran`) builds through `models_vm()`, which installs no shim prelude, so it still passes. `prelude-tests.rs:27-32` (`prelude_vm`) calls `install_section_loop_shim` directly after `.shims()`, so its line 30 goes with the installer, the `install_section_loop_shim` import at `:16` with it, and the doc at `:23-25` then says the yield shims install `models.loop` too.
- Acceptance: engine `execute/tests/models_loop.rs`, `models_loop_contract.rs`, `models_loop-arguments.rs`, and `unified_pipeline.rs` run through `setup_section_vm` and still pass, and `models-tests.rs:91` still passes. Every section VM still holds `models.loop` and `models.compact` exactly as before.

**Work item 22 (S1-14, S1-15): the test-only VM methods and the "scheduler mode" wording**

- Files: `src/vm/run.rs`, `src/vm/inject.rs`, `src/test_support.rs`, `src/lib.rs`, `src/vm/install.rs`, `src/coro.rs`, `src/test_vm.rs`, `src/__impl_coro.lua`, `src/__impl_fanout.lua`, `src/__impl_tasks.lua`
- Problem: `resume_block_coro` (`vm/run.rs:80-92`) is called only by tests (engine `lua/tests/errors.rs:295`, `lua/tests/coroutine.rs:85`, `src/tests/require.rs:491`), since production resumes through `resume_block_coro_answer` (engine `execute/scheduler/step.rs:158`). `inject_values` (`vm/inject.rs:45-59`) is called only by tests and the bench, since production injects through `inject_values_with_var`. Eleven sites in nine files present "scheduler mode" or a "direct chunk path" as a production mode, though every production block runs through `start_block_coro` and `run_chunk` lives in `test_support`.
- Change: keep `resume_block_coro` where it is and gate it with `#[cfg(any(test, feature = "test-support"))]`. It cannot move to `test_support.rs` as it stands: its body calls `step_block_coro` (`vm/run.rs:143`), a private function in the `vm` module that `test_support` at the crate root cannot reach. The gate is one attribute instead of a move, and `step_block_coro` stays beside `resume_block_coro_answer`, its production sibling.
- Change: move `inject_values` (`vm/inject.rs:45-59`) into a new `impl SectionVm<Uninjected>` in `test_support.rs`; it calls only public API, so the move compiles. Retarget the two doc links that name `resume_block_coro`: in `start_block_coro`'s doc (`vm/run.rs:54`), "[`resume_block_coro`](Self::resume_block_coro)" becomes "[`resume_block_coro_answer`](Self::resume_block_coro_answer)", and in `CoroStep`'s doc (`vm/run.rs:275`), "[`SectionVm::resume_block_coro`]" becomes "[`SectionVm::resume_block_coro_answer`]". The bench already declares `required-features = ["test-support"]`.
- Change: `lib.rs:29-30` ends "which surfaces from the block's coroutine (`SectionVm::start_block_coro` and its resumes) as a Lua-category error."; `lib.rs:32` begins "The VM also holds `tasks`, the shims for spawning, ...".
- Change: `vm/install.rs:111` becomes "Installs the control surface: `jump` and the `prompt` reads as Rust callbacks (none suspends)." In `install_control_globals` (`vm/install.rs:80`), "Called once by `runvil-engine` after Engine injection." becomes "Test-only: production installs `jump` and the `prompt` reads through [`install_scheduler_control_globals`](Self::install_scheduler_control_globals) and `call` as a yield shim."
- Change: `coro.rs:124-126` begins "The coroutine standard library loads for the shim's `yield` capture (a test VM that never installs the shims keeps exactly `STRING | TABLE | MATH | UTF8`);" and keeps the rest of the sentence.
- Change: the `run_chunk` doc (`test_support.rs:154`) becomes "The test path that runs a section's Lua block without the scheduler; production drives blocks through [`start_block_coro`](Self::start_block_coro)."; `test_vm.rs:357` becomes "The control globals, whose prompt reads find no section, and every coroutine yield shim."
- Change: the first comment line of `__impl_coro.lua`, `__impl_fanout.lua`, and `__impl_tasks.lua` says "a section VM" in place of "a scheduler-mode section VM". The eleventh site, "the direct chunk path" at `hardening.rs:104`, already went with step 1's `CancelHook` doc rewrite (work item 2); confirm it is gone.
- Acceptance: the moved and gated methods' callers compile under `cfg(any(test, feature = "test-support"))`, and `broken_intra_doc_links` in the docs commands flags any production doc still linking a moved method. A search for "scheduler mode" and "direct chunk path" in `crates/runvil-internal/lua/src/` finds nothing.

**Work item 17 (S1-6, S4-7): build the `sys` mirror at injection, and one prose installer**

- Files: `src/vm.rs`, `src/vm/inject.rs`, `src/vm/install.rs`, `src/require.rs`, `src/prose.rs`, `src/globals.rs`
- Problem: `sys_live: Arc<Mutex<Option<Json>>>` is created `None` (`vm.rs:146`, `:219`) and set to `Some` inside `install_values` (`vm/inject.rs:126-132`) before `injected()` returns the injected VM; a failed install hands the VM back as `InjectFailure`, which is only torn down (`vm/inject.rs:22-42`). Every reader is a method of the injected `SectionVm` (`install_lazy_prose`, `install_require`, `re_seal_sys`), so every `None` arm is dead, and `NOT_INJECTED` (`vm.rs:59-61`) is read only at `prose.rs:133`. The `Option` spreads into `require::VmShare.sys_live` (`require.rs:55`), `prose_captures` (`require.rs:196-206`), and `prose::{install, swap, lazy_read}` (`prose.rs:73`, `:90`, `:111`). On those same signatures, `globals::install_prose` (`globals.rs:116-124`) writes the slot `replace_prose` (`globals.rs:126-137`) writes, and its one caller, `prose::install` (`prose.rs:63-79`), is `swap` (`prose.rs:90-96`) without the return value.
- Design: the `SectionVm<S>` typestate already guarantees injection before every reader, so the mirror is created when `install_values` succeeds and the `Option` with its dead `None` arms goes.
- Change: in `inject_values_with_var`, once `install_values` succeeds, create `Arc::new(Mutex::new(sys.clone()))` and pass it to `injected()` (`vm/inject.rs:155`) as a field of `Injected` beside `access` (`vm.rs:93-95`), so `SectionVm` loses its `Option` field and `install_values` loses its lock block. Every signature above takes `&Arc<Mutex<Json>>`; delete `NOT_INJECTED` and the `ok_or_else` at `prose.rs:133`, keeping `SYS_LIVE_POISONED`.
- Change: delete `globals::install_prose`; `prose::install` returns `swap(lua, sys_live, render).map(drop)`, as `restore` (`prose.rs:102-104`) already does over `replace_prose`. Its caller at `vm/install.rs:298` is unchanged, and the `# Errors` texts step 1 set on `prose::install`, `swap`, and `lazy_read` stay.
- Change: the sites to update are `vm.rs:59-61`, `:146`, `:219`; `vm/inject.rs:126-132`, `:155-198`; `vm/install.rs:193`, `:274-278`, `:298`; `require.rs:55`, `:85`, `:196-206`; and `prose.rs:31`, `:73`, `:90`, `:111`, `:128-134`. Deleting the lock block also leaves the `SYS_LIVE_POISONED` import at `vm/inject.rs:11` unused; remove it.
- Acceptance: engine `execute/tests/model_and_reply-globals.rs:64` (prose sees `sys.model` after `re_seal_sys`) and `:101` (a fanout arm sees it), engine `execute/tests/suite/lazy_prose.rs`, and `src/tests/require/prose.rs` still pin the lazy read.

**Work item 23 (S1-8, S1-9, S1-10, S1-13, S1-24, S4-28): the VM docs**

- Files: `src/vm.rs`, `src/vm/state.rs`, `src/walk_budget.rs`, `src/argv.rs`
- Scope: docs and one comment only.
- Problem: `apply_lua_limits`'s doc (`vm.rs:279-289`) names two limits and omits the heap ceiling it records for the walk budget (`vm.rs:294`, `walk_budget.rs:56-64`) and the byte budget it resets (`vm.rs:296-297`), and "non-env" (`vm.rs:203`, `:285`) contrasts with an environment configuration removed in commit `545535a10`. `SectionVm::new` says the VM retains no emitter (`vm.rs:183-185`), though the store and `require` globals keep clones. The `Uninjected` doc (`vm.rs:84-85`) groups the ungated `set_cancel` with two stage-gated calls, inviting a move that breaks four tests. `install_tool_call_counts` explains where its delegation lives (`vm/state.rs:77-78`). `walk_budget.rs` says a limit "converts as it does today". The `argv` installers restate the frozen rule and the malformed check the public `Argv` docs state.
- Change: `apply_lua_limits`'s second paragraph becomes "Sets the heap ceiling (`lua_memory_bytes`), records it for the conversion walk budget, and resets the `log()` event budget (`lua_log_events`) and the byte budget derived from it. Called by the executor right after construction, ahead of the Pack preludes and every block, so they spend the run's limits rather than the defaults `SectionVm::new` installs." In the body comment at `vm.rs:203`, "A safe non-env default" becomes "A default".
- Change: in `SectionVm::new` (`vm.rs:183-185`), "Every lifecycle report goes through the emitter the caller hands each step; the VM retains none" becomes "Construction and teardown report through the emitter the caller hands each step; the persistent globals that `install_engine_globals` and `install_require` install keep clones of theirs." The `Uninjected` doc becomes "A section VM's stage before Engine injection: construction, the run's limits, and the raw-model-id opt-in." Leave `set_cancel` where it is.
- Change: delete the paragraph at `vm/state.rs:77-78` ("The installation itself sits in the `tools` module; this method only supplies the VM's own state.").
- Change: `CHECK_DEPTH_LIMIT` (`walk_budget.rs:21-23`) becomes "The most table levels `check_value` descends, the top-level table being level 1. A table nested past it is not counted."; the inline comment at `walk_budget.rs:105-106` becomes "An entry that cannot be read ends the walk of this table, and the conversion that follows reports it."
- Change: `install_frozen` (`argv.rs:53-54`) becomes "Installs `argv` frozen, the mode of every section but H1: the value H1 left behind at the freeze."; `install_writable` (`argv.rs:39-41`) becomes "Installs `argv` as a plain writable global: the H1 pass's mode. `None` (malformed args, or JSON null) installs nil." Keep the module doc and the public `Argv` docs, which engine readers see through the `lib.rs:110` re-export.
- Acceptance: the docs commands pass.

</step-5>

<step-6>

### Step 6: the compaction retry state, the snapshot words, and the models family docs

- Component: VM setup and models loop
- Tests:
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `$env:CARGO_BUILD_WARNINGS = "deny"; cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `cargo fmt --all --check`
  - `$env:RUSTDOCFLAGS = "-D warnings"; cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`
  - `$env:RUSTDOCFLAGS = "-D warnings"; cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`
  - `Remove-Item Env:RUSTDOCFLAGS, Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue; cargo doc --locked --no-deps --all-features -p runvil-lua --document-private-items`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`

- Work items: 7 and 19, in one commit.
- Order: work item 7, then 19. Work item 7 changes the loop machine and the closure in `models_loop.rs`, and work item 19 then rewrites `loop_begin`'s doc in the same file over the closure work item 7 leaves. Work item 19's `install_models` doc names `models.loop` and `models.compact`, which step 5 (work item 21) now installs through the one shim installer.
- Paths: lua paths are under `crates/runvil-internal/lua/`, paths marked "engine" are under `crates/runvil-internal/engine/src/`, and commands run from `c:\Users\Vinnie\cursor\runvil`. Line numbers were read at `7a5341bba`, and earlier steps and earlier work items in this step shift them, so every line number is a hint: locate each site by symbol or quoted text, and where a line and a symbol disagree, the symbol wins.
- Constraint: no author-visible behavior change. Every Lua global, table shape, and message stays as the guide documents it, and every type or signature change stays inside the workspace. Add no dependency and no crate. Every `.rs` file stays at or under 500 lines, or `build_ceiling::check()` fails the build.
- Acceptance: the Tests pass, plus each work item's acceptance below. The `runvil-lua --document-private-items` build is not a CI gate and passes on exit status; read its warnings for the lines this step edited, and treat a warning on an edited line as a failure.
- Loop: between work items, `cargo nextest run --locked -p runvil-lua -p runvil-engine -p runvil-parser --all-features`; the Tests above gate the commit.

**Work item 7 (S4-12, S4-11): the compaction retry state and the snapshot words**

- Files: `src/models_loop/machine.rs`, `src/models_loop.rs`, `src/messages-snapshot.rs`, `src/models_loop/tests.rs`, `src/models_loop/machine-tests.rs`
- Problem: `Machine.snapshot: Option<Snapshot>` (`machine.rs:72-74`) sits beside `Phase::Compacting(OverflowReason)` (`:87`), and the list's compacting flag is set and cleared by hand at five sites (`models_loop.rs:184`, `:266`; `machine.rs:212`, `:319`, `:416`). At `machine.rs:210-214`, a Compacting phase with no snapshot clears the flag and raises `ErrorKind::Internal`, a state the code never builds. The snapshot also outlives the flag by design: after a kept compaction, `step` puts it back (`:216-218`) and `chat()` enters `Phase::Chatting` still holding it, so `judge` and `overflow` (`:276-279`, `:313-316`) can restore the list and raise `context_exhausted` on a second overflow. The snapshot module calls the last send and the frozen sets "rendered tool list" and "send state".
- Design: two phase variants carry the retry state, not one field move: `step` restores the snapshot and `chat` retries holding it, so merging it into `Compacting` alone would drop the retry state. A `Compaction` value owning the flag end to end is deferred.
- Change: `Phase::Compacting(OverflowReason, Snapshot)` and `Phase::Chatting(Option<Snapshot>)`, where `Some` means the round has compacted and holds the retry state. `read_input`'s match (`models_loop.rs:294`, `:298`) becomes `Phase::Compacting(..)` and `Phase::Chatting(_)`.
- Change: delete `Machine.snapshot` (`machine.rs:72-74`), moving its doc ("held until the retried round's answer arrives. Set means the round has compacted") onto the `Chatting` variant, and delete the unreachable internal-error arm at `machine.rs:210-214`. That arm's `machine.rs:212` clear is the fifth hand-paired flag site, so after the deletion four remain: `models_loop.rs:184`, `:266`, and `machine.rs:319`, `:416`. Do not list `machine.rs:212` among the sites to keep.
- Change: two test matches on `Phase::Chatting` as a unit variant stop compiling once it takes a field: `src/models_loop/tests.rs:308` and `src/models_loop/machine-tests.rs:137` become `Phase::Chatting(_)`.
- Change: `messages-snapshot.rs:5-6` ends "exactly. Neither touches the list's last send or its frozen sets."; `:34` becomes "Puts the records back; the last send and the frozen sets stay."; `:39` becomes "Removes every record; the last send and the frozen sets stay."
- Ceiling: `src/models_loop/machine.rs` holds 487 lines; the deleted field and arm free more lines than the variant fields add.
- Acceptance: `models_loop/machine-tests-compaction.rs:58-80` and `:168`, `models_loop/tests.rs:253-259`, and engine `execute/tests/compaction.rs`, `compaction-rules.rs`, and `models_loop_compactors.rs` still pin the flag and the outcomes, including `context_exhausted` on a second overflow.

**Work item 19 (S3-14, S3-15, S3-16, S3-17, S6-7, S3-18): the models family docs and the duplicate getter**

- Files: `src/models.rs`, `src/models-userdata.rs`, `src/models_loop.rs`, `src/tools.rs`
- Problem: `LuaModelHandle::label` (`models-userdata.rs:40-45`) and `name` (`:34-38`) both return `self.binding.alias()` under docs that read as two facts, though the guide promises the two fields always hold the same string (`10-models.md:931`, `:941`). The handle's type doc (`:14-15`) repeats the module doc word for word, and three getters (`:67-69`, `:88-91`, `:101-103`) repeat one boundary paragraph. `RAW_ID_CONTEXT` (`models.rs:39-41`) calls the documented raw-model-id fallback "the hack". `install_models` (`models.rs:244-245`) names two of its module's three suspending shims, and `install_tools` (`tools.rs:103-105`) restates its module doc's shim note. `loop_begin`'s doc (`models_loop.rs:110-115`) cites a plan heading for its check order and quotes a closure body the code does not have; the closure (`:175-187`) also clears `running` and the list's `compacting` flag.
- Change: delete `LuaModelHandle::label` and register the `label` field over `this.name()` (`models-userdata.rs:116`). `name`'s doc becomes "Returns the role label the handle was obtained by; the `name` and `label` fields both read it."
- Change: the type doc becomes "One model handle over a frozen `ModelBinding`. Its getters return raw Lua numbers where the binding keeps validated types (`NonZeroU32`, `Temperature`)." Delete the boundary paragraph from `context`, `temperature`, and `max_tokens`.
- Change: `models.rs:39-41` becomes "The context window a raw-model-id binding records: the fallback has no catalog metadata, so a conservative default keeps the compactor precheck safe rather than refusing the model." `models.rs:244-245` becomes "The coroutine shim layer installs the suspending `models.infer`, `models.loop`, and `models.compact`, because yield cannot cross the Rust callback boundary."
- Change: in `install_tools` (`tools.rs:103-106`), delete the first sentence and keep "Both offer verbs refuse once `list`, the section's conversation, holds the section's frozen sets."
- Change: in `models_loop.rs:110-115`, replace the text from "then that entry's argument checks" through the quoted closure body with "`loop_begin`'s body: the entry flag; for a handle's `loop`, the receiver then the arity, and for `models.loop`, the handle-first refusal then the arity; the compactor's type; the refusal while `running` or the list's `compacting` flag is set; then the machine over the section's list, its step closure, and the first action." Keep the rest from "`running` is set only once".
- Acceptance: `models-tests.rs:103-105` still pins `label` and `name` equal, and engine `execute/tests/models_loop-arguments.rs:117` still pins the argument order the new text states. A handle's `label` field reads the same string as before.

</step-6>

<step-7>

### Step 7: the store module, the program and compactor vestiges, the key order module, and the closing outline check

- Component: Store and module docs
- Tests:
  - `cargo nextest run --locked --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-features`
  - `$env:CARGO_BUILD_WARNINGS = "deny"; cargo clippy --workspace --exclude workshop --exclude workshop-server --exclude workshop-server-api --all-targets --all-features`
  - `cargo fmt --all --check`
  - `$env:RUSTDOCFLAGS = "-D warnings"; cargo doc --workspace --no-deps --all-features --exclude workshop --exclude workshop-server --exclude workshop-server-api`
  - `$env:RUSTDOCFLAGS = "-D warnings"; cargo doc --locked --no-deps --all-features -p runvil-engine --document-private-items`
  - `Remove-Item Env:RUSTDOCFLAGS, Env:CARGO_BUILD_WARNINGS -ErrorAction SilentlyContinue; cargo doc --locked --no-deps --all-features -p runvil-lua --document-private-items`
  - `node --test crates/workshop/ui/test/docs-claims.mjs`
  - `cargo vibelight`
  - `rg -w -e InstructionBudget -e HOOK_BUDGET -e install_instruction_budget -e budget_hook -e lua_quota -e ToolRuntime -e ModelRuntime -e tool_runtime -e model_runtime -e install_section_loop_shim -e LOOP_REGISTRY -e COMPACT_REGISTRY -e NOT_INJECTED -e install_prose -e read_range -e read_range_numbered -e direct_yield -e raw_field -e json_field -e PACKS_LABEL -e bound_tools -e current_tool_bindings -e store_conflicts -e parse_message -e 'src/handles\.rs' -e 'src/collection-order\.rs' .vibelight/runvil_lua.md; if ($LASTEXITCODE -ne 1) { throw "the lua outline still names a removed declaration" }`
  - `foreach ($n in 'CancelHook', 'cancel_poll', 'install_cancel_hook', 'Offers', 'ModelSelection', 'model_selection', 'ModelCall', 'dispatch_body', 'read_only_walk', 'author_refusal', 'set_messages', 'tool_set', 'tool_id', 'current_static_set', 'store_conflict', 'parse_record', 'src/bindings\.rs', 'src/section\.rs', 'src/var\.rs', 'src/order\.rs') { rg -q -w $n .vibelight/runvil_lua.md; if ($LASTEXITCODE -ne 0) { throw "the lua outline lacks $n" } }`

- Work items: 16, 24, 25, and 26, in one commit, plus the product contract's success check that a fresh plain `cargo vibelight` run shows each renamed and deleted declaration.
- Order: work item 16, then 24, then 25, then 26, then the closing check. The four share a cause rather than files: each moves an item, or the one statement of a rule, to the module that owns it, or drops an attribute or derive nothing needs. The closing check goes last because it reads the outline after every rename in steps 1-7.
- Paths: lua paths are under `crates/runvil-internal/lua/`, paths marked "engine" are under `crates/runvil-internal/engine/src/`, and commands run from `c:\Users\Vinnie\cursor\runvil`. Line numbers were read at `7a5341bba`, and earlier steps and earlier work items in this step shift them, so every line number is a hint: locate each site by symbol or quoted text, and where a line and a symbol disagree, the symbol wins.
- Constraint: no author-visible behavior change. Every Lua global, table shape, and message stays as the guide documents it, and every type or signature change stays inside the workspace. Add no dependency and no crate. Every `.rs` file stays at or under 500 lines, or `build_ceiling::check()` fails the build.
- Acceptance: the Tests pass, including the three outline commands, plus each work item's acceptance below. The `runvil-lua --document-private-items` build is not a CI gate and passes on exit status; read its warnings for the lines this step edited, and treat a warning on an edited line as a failure.
- Loop: between work items, `cargo nextest run --locked -p runvil-lua -p runvil-engine -p runvil-parser --all-features`; the Tests above gate the commit.

**Work item 16 (S4-1, S4-2, S4-3, S4-4, S4-5, S4-6, V4-1): the store module**

- Files: `src/engine_globals.rs`, `src/engine_globals-store.rs`, new `src/engine_globals-store-read.rs`, `src/vm/install.rs`, `src/tests/logging.rs`
- Problem: the store operation bodies, the read helpers, and `run_store_op` live in the parent module (`engine_globals.rs:24-31`, `:157-343`) while the store module imports two of them back (`engine_globals-store.rs:12`). `read_range` and `read_range_numbered` (`engine_globals.rs:213-232`) are one-line wrappers that `read_store_bounded` picks between twice. Seven docs narrate closure capture and `mlua::Scope`, an alternative the crate never uses. The store module doc names the "shared library" that `require` replaced, the parent doc lists `ui` as installed in every VM though only a run with a snapshot installs it, `run_store_op`'s doc narrates the other store path, and the log length refusal (`engine_globals.rs:60-63`) hardcodes the 256 its constant holds, so its test (`tests/logging.rs:163-175`) would keep passing after the constant changed.
- Placement: `engine_globals-store.rs` holds 402 lines, and moving every helper into it would bring it to about 575, past the 500-line ceiling. So `run_store_op` moves into `engine_globals-store.rs` itself (about 45 lines, ending near 450), and the read helpers move into a new private child module of the store module, `engine_globals-store-read.rs`, wired at the top of `engine_globals-store.rs` as `#[path = "engine_globals-store-read.rs"] mod read;`, the way `error-value.rs` wires `error-value-store.rs`. The decision record holds this choice.
- Change: move `LineNumbers`, `read_store`, `read_store_numbered`, `read_store_bounded`, `resolve_line_range`, `with_read_range`, and `number_lines_from` into `engine_globals-store-read.rs`, with the module doc "The store's read bodies: whole-file and line-range reads, plain or numbered.", and give the items the store module calls `pub(super)`. Move `run_store_op` into `engine_globals-store.rs`; add `pub use store::run_store_op` beside the existing re-exports (`engine_globals.rs:19`) and delete the `use super::{read_store, read_store_numbered};` import, importing them from `read` instead. Every crate and engine path stays valid.
- Change: delete `read_range` and `read_range_numbered`. In `read_store_bounded`, choose `render: fn(&[&str], usize) -> String` once, `number_lines_from` or `|lines, _| lines.join("\n")` (which coerces because it captures nothing), and call `with_read_range`; keep `view.read_string` for the whole-file unnumbered read, so that read stays verbatim, trailing newline included.
- Change: the store module doc (`engine_globals-store.rs:1-3`) becomes "The `store` table: Lua dispatchers over the direct store closures, and the switch that routes them to the yield shims once the Pack preludes have installed." The parent module doc (`engine_globals.rs:1-2`) becomes "Engine globals installed into section VMs: `log`, `untrusted`, and the `store` table in every one, and `ui` when the run has an application-state snapshot. Each is a persistent global: installed once per section and valid across every chunk the VM runs."
- Change: the second paragraph of `run_store_op`'s doc becomes "The direct closures `install_store_table` builds share these operation bodies. `view` is the store view the caller derived." Keep its first sentence and its `# Errors`.
- Change: the closure narration. Only five docs mention `mlua::Scope`, at `engine_globals.rs:87` and `:116`, `engine_globals-store.rs:215`, and `vm/install.rs:29` and `:82`. Delete those five clauses. From `install_untrusted`, keep only its nonce and type-error facts. The phrase "as a persistent global valid for the section's whole lifecycle" appears in `install_ui` and `install_log`; delete it from both. In `install_require`, the sentence at `vm/install.rs:148-149` is "Installs Runvil's `require` as a persistent global for the section's whole lifecycle", a different wording with no "valid", so shorten it separately to "Installs Runvil's `require` as a persistent global." Keep the fact that the globals keep a clone of the emitter, and keep the store doc's sentence that the view locks per call and holds nothing across an await (`engine_globals-store.rs:228-230`). Step 5 already rewrote the `install_control_globals` doc near `vm/install.rs:82`; delete only its `mlua::Scope` clause if one remains.
- Change: the log length refusal becomes `format!("log message must be at most {LUA_LOG_CHARACTER_LIMIT} characters")`, and `tests/logging.rs:172` builds its expected text the same way, as the input at `:163` already does.
- Acceptance: the refusal still reads "log message must be at most 256 characters". `src/tests/store.rs` and `src/tests/store_reports.rs` still pin both reads' text and bounds, engine `execute/tests/observations.rs` and the fixture `crates/runvil-internal/engine/tests/prompts/execution/store-triad.md` run them end to end, and `tests/logging.rs:163-175` pins the limit through the constant.

**Work item 24 (S1-17, S1-18, S1-19, S1-21, S6-14): the program, prelude, and packs docs**

- Files: `src/program.rs`, `src/prelude.rs`, `src/prelude-tests.rs`, `src/packs.rs`
- Problem: `#[non_exhaustive]` on `LuaProgram` (`program.rs:58`) adds nothing its five private fields do not already prevent, and no workspace lint asks for it. Its type doc calls `load` "a crate-internal step" (`program.rs:40-41`), though `load` is `pub` and two outside tests call it. `compile_internal` (`program.rs:132-133`) says it compiles "the coroutine shim prelude", though six embedded chunks compile through it. The `prelude` module doc calls the prelude's `...` "its Pack's local name" (`prelude.rs:4-5`) while `install_preludes` calls the same value the Pack's id. `install_pack_objects` (`packs.rs:135-137`) restates, in a third wording, the membership rule the module doc and `pack_tools` state.
- Change: delete `#[non_exhaustive]` and its justifying paragraph (`program.rs:43-44`). runvil-parser re-exports `LuaProgram` (parser `crates/runvil-internal/parser/src/lib.rs:25`) and is also `publish = false`.
- Change: `program.rs:40-41` becomes "Loading a program ([`load`](Self::load)) creates a function in the supplied VM but likewise does not call it."
- Change: `compile_internal`'s first line becomes "Compiles one of the crate's embedded Lua chunks without observations."; rename its parameter `location` to `chunk_name` (`program.rs:144`) and replace the parameter's sentence with "`chunk_name` serves as both the chunk name and the display location; an `@`-prefixed name renders as a file path in errors, which the line mapper never rewrites." Callers pass it positionally, so none changes.
- Change: in the `prelude` module doc, "is its Pack's local name" becomes "is its Pack's id"; the assertion message at `prelude-tests.rs:173` says "its own Pack's id" in place of "its own Pack's local name".
- Change: `install_pack_objects`'s doc becomes "Builds one Pack object per Pack `pack_tools` lists and returns their names."
- Acceptance: the full suite covers runvil-parser's re-export of `LuaProgram`, and the docs commands pass.

**Work item 25 (S4-13, S4-14, S4-15, S4-16, S4-17): the compactors and projection**

- Files: `src/projection.rs`, `src/compactors.rs`, `src/compactors-tests.rs`
- Problem: `visible_text` (`projection.rs:260-262`) promises "the concatenation of its text parts" though its `Parts` arm is `unreachable!`; the module doc (`projection.rs:30-32`) implies a per-model projection, though `project_messages` takes only records and builds one shape. `Compactor` derives `Default` with `#[default] Fail` (`compactors.rs:104-107`) for one test whose local `resolve` claims to mirror the loop, which resolves an omitted compactor as a Lua value (`models_loop.rs:154`). The two policy rules ("fail always raises", "clear works only inside a compactor") are stated five times. `UsageAnchor::tokens` (`compactors.rs:202-206`) is a production accessor only tests read.
- Change: `visible_text`'s doc becomes "The text of a plain-text content. Only called on composed system records, where validation refused parts, so the parts arm is unreachable." The comment at `projection.rs:168-169` then repeats it and goes. The sentence at `projection.rs:30-32` becomes "The projection is recomputed per dispatch and is the same for every model."
- Change: drop `Default` and `#[default]` from `Compactor`. In `compactors-tests.rs:65-81`, delete `resolve` and the asserts at `:71-75`, keep the `fail_invoked` assertion, and rename the test from `the_omitted_compactor_defaults_to_fail` to `fail_raises_typed_exhaustion_for_a_provider_overflow`, since it no longer exercises omission; engine `execute/tests/models_loop_compactors.rs:19` pins the real omission path.
- Change: the `Compactor` doc (`compactors.rs:98-103`) becomes "The compactor policies the `compactors` global ships; `Fail` is the one the loop reads when the compactor argument is omitted." From `install_compactors` (`compactors.rs:296-297`), delete "`compactors.clear` empties `list`, and only while a compactor runs over it." Keep the module doc (`:12-16`), the variant docs, `invoke`'s `# Errors`, and `install_compactors`' external-error, downcast, normalization, and install-order sentences.
- Change: gate `UsageAnchor::tokens` behind `#[cfg(any(test, feature = "test-support"))]`, the gate `lib.rs:101` puts on `test_support`. Its readers are `compactors-tests.rs:342`, `:369`, `:375` and engine `execute/scheduler/chain_record/tests.rs`, a `#[cfg(test)]` module whose dev build enables `test-support`.
- Acceptance: `fail_raises_typed_exhaustion_for_a_provider_overflow` passes under its new name, `projection-tests.rs` still pins the composed-parts refusal, engine `execute/tests/models_loop_compactors.rs:19` still pins omission, and a production use of `tokens` would fail to build.

**Work item 26 (S4-18, S4-19, S4-20, S4-23, S4-24, V4-2, S4-25): the key order module and the iteration docs**

- Files: `src/collection.rs`, `src/collection-order.rs` (becomes `src/order.rs`), `src/collection-order-tests.rs` (becomes `src/order-tests.rs`), `src/lib.rs`, `src/iteration.rs`, `src/json/keys.rs`
- Problem: the crate-wide key order sits under `collection`, fanout's module (`collection.rs:16-19`), though `iteration.rs:42` and `json/keys.rs:8` also use it, and its module doc (`collection-order.rs:6-9`) omits `json.encode` among the order's sharers. `sort_key` and `KeyError::NotFinite` say refused keys have no position (`collection-order.rs:105-106`, `:117-121`), though `json::keys::position` places both kinds (`json/keys.rs:143-155`). `render_item` (`collection.rs:97-99`) omits `null`, which it renders as compact JSON. The iteration docs call the trailing segment non-scalar, though non-UTF-8 string keys and infinite number keys, both scalars, land there (`iteration.rs:215-223`, `:272-275`), and `previous_order` and `next_index` repeat the module doc's resume limit.
- Change: add a top-level `mod order;` to `lib.rs`; rename `collection-order.rs` to `order.rs` and `collection-order-tests.rs` to `order-tests.rs`, with the tests wired as `#[cfg(test)] #[path = "order-tests.rs"] mod tests;` (`collection-order.rs:141-143`); delete the `#[path] mod order;` and the re-export at `collection.rs:16-19`; and import `crate::order::{KeyError, SortKey, sort_key}` in `collection.rs`, `iteration.rs`, and `json/keys.rs`. Rename with `git mv` so history follows the files.
- Change: the order module doc's sharer sentence becomes "`SortKey::compare` orders two of them. Fanout's member enumeration, the deterministic `pairs`/`next` installer, and `json.encode`'s key order share both, so a table's order is a function of its contents wherever it is read."
- Change: `sort_key`'s `# Errors` becomes "Returns `KeyError` when the value is not a string, number, or boolean, when a string's bytes are not valid UTF-8 (it has no label), or when a number is not finite. Each caller maps the failure onto its own diagnostic, so this helper names no feature." `KeyError::NotFinite` becomes "A number key that is NaN or infinite." The test comment at `collection-order-tests.rs:53-54` becomes "A non-finite number is refused, and each caller decides its own diagnostic."
- Change: `render_item`'s doc becomes "Renders a fanout arm's item for prose substitution and stub text: strings verbatim, numbers and booleans in their natural string form, and arrays, objects, and null as compact JSON."
- Change: the module doc at `iteration.rs:32-36` becomes "The ordering covers boolean keys, finite number keys, and valid UTF-8 string keys, the only keys a payload the run log stores can hold. Every other key (a table, function, userdata, or thread, a non-UTF-8 string, or an infinite number) is still visited rather than dropped, after every ordered key, in the order Lua's own `next` produced it." At `iteration.rs:27`, "A cleared non-scalar key" becomes "A cleared unordered key".
- Change: `WalkOrder`'s last sentence (`iteration.rs:70-72`) becomes "An unordered key, any key `sort_key` refuses, has no cross-process position, so all of them compare equal and keep the order Lua's own `next` produced."; `compare`'s last sentence (`:91-92`) becomes "All unordered positions compare equal, so a stable sort keeps their raw traversal order."; `ordered_keys` (`:184-186`) becomes "The table's live keys in deterministic walk order: the array part (`1..=#table`) in index order, then the hash part's keys `sort_key` accepts, by [`SortKey`], then every key it refuses."; and in `WalkOrder` (`:67`) and `segment` (`:80-81`), "non-scalar" becomes "unordered".
- Change: `previous_order` (`iteration.rs:255-265`) becomes "The walk position a lone key would occupy, ignoring whether it is still in the table: any integer `index >= 1` is an array key without consulting the live border (the module doc gives the resume limit this sets), a hash key `sort_key` accepts keeps its [`SortKey`], and any other key is unordered." In `next_index` (`:234-236`), delete "; a cleared non-scalar key has no computed position, so the resume falls back to the trailing segment, the best a stateless resume can offer".
- Keep: `iteration-tests.rs:245` ("A cleared non-scalar key") is noticed for the next scan, not this one; leave it.
- Acceptance: `iteration-tests.rs:162-269` still pins the resume behavior, and `collection.rs:186-204` and `iteration-tests.rs:79` still pin the mixed-key order.

**Closing outline check**

- This is the last step in this repository, so its Tests regenerate the outlines with a plain `cargo vibelight` and read `.vibelight/runvil_lua.md`, which is gitignored and adds nothing to the commit.
- The first grep fails if the outline still names a removed declaration (`InstructionBudget`, `HOOK_BUDGET`, `install_instruction_budget`, `budget_hook`, `lua_quota`, `ToolRuntime`, `ModelRuntime`, `tool_runtime`, `model_runtime`, `install_section_loop_shim`, `LOOP_REGISTRY`, `COMPACT_REGISTRY`, `NOT_INJECTED`, `install_prose`, `read_range`, `read_range_numbered`, `direct_yield`, `raw_field`, `json_field`, `PACKS_LABEL`, `bound_tools`, `current_tool_bindings`, `store_conflicts`, `parse_message`, or the old `src/handles.rs` and `src/collection-order.rs` paths); the loop fails if it lacks a new name.
- A failure here names a rename an earlier step missed: fix it in this step's commit if it is in this step's files, and otherwise report it as blocked.

</step-7>

<step-8>

### Step 8: the guide's setup order, in a second repository

- Component: Guide setup order
- Tests:
  - `$env:RUNVIL_DOCS = 'c:\Users\Vinnie\cursor\runvil-docs'; cargo xtask site --books-only`

- Work items: 27 (S5-2). The operator chose to land this fix as a second commit in `../runvil-docs` in this same pass.
- Order: one work item. It runs after step 5, whose work item 21 merged the loop installer this step describes.
- Repository: edit and commit in `c:\Users\Vinnie\cursor\runvil-docs`, not in runvil; this commit shares no rollback with steps 1-7. The Tests command runs from the runvil root `c:\Users\Vinnie\cursor\runvil`, because the `xtask` alias lives in runvil's `.cargo/config.toml`.
- File: `src/language/05-lua-environment.md` in `runvil-docs`
- Problem: `:183` says "Every section VM is set up in one fixed order" and lists ten steps (`:185-194`), but no step installs `untrusted`, which exists from construction (runvil `crates/runvil-internal/lua/src/vm.rs:229-241`), and step 1 omits `tools`, `models`, `messages`, and `compactors`, which injection installs beside `args`, `argv`, `sys`, and `var` (runvil `crates/runvil-internal/lua/src/vm/inject.rs:118-151`). Pack preludes read `tools`, `store`, `untrusted`, and `json` as they install at step 8 (runvil `crates/runvil-internal/lua/src/prelude.rs:4-8`, guide `src/language/12-tools.md:2744-2751`), so a reader of the list finds no step that makes them exist. Confirm the line numbers against the guide file first; they are from the scan, and the guide is hand-edited.
- Change: add a new first entry, "`untrusted`, when the VM is built", and renumber the list; the old step 1 becomes "`args`, `argv`, `sys`, `var`, `tools`, `models`, `messages`, and `compactors`".
- Change: fold the old step 7 into step 6. Step 5 of this plan (work item 21) made `install_coro_shims` install `models.loop` and `models.compact`, so one installer performs what were steps 6 and 7, and the old step 7 has no separate installer left. Step 6 becomes "the suspending calls: `call`, the `tasks` table, `tools.call`, `models.infer`, `models.loop`, and `models.compact`", and the old steps 8, 9, and 10 renumber to 7, 8, and 9. If work item 21 was dropped, keep the old step 7 as its own entry instead.
- Acceptance: the list names a step for every global a Pack prelude reads, and the books build. `cargo xtask site --books-only` checks `RUNVIL_DOCS` first and fails before staging anything without it; `mdbook` must be on `PATH` or named by `MDBOOK` (it is at `C:\Users\Vinnie\.cargo\bin\mdbook.exe`).
- Constraint: guide prose only; the guide must keep describing the behavior the code has.
- Loop: the Tests command.

</step-8>

</execution-plan>
